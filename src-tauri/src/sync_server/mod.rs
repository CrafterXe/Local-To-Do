//! 局域网同步服务（Windows 桌面端）。
//!
//! 设计要点：
//! - 仅监听当前家庭局域网的私有 IPv4 地址，拒绝绑定公网地址；
//! - 所有请求经 TLS（自签名 CA + 服务器证书）加密，Android 端做证书指纹锁定；
//! - 服务运行在独立后台线程的 Tokio 运行时中，不阻塞 Tauri 主线程；
//! - 数据读取复用 `ltd_core::sync` 的快照/增量接口，不暴露任何 SQL 能力。

mod routes;
mod store;
#[cfg(test)]
mod tests;
mod tls;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::RngCore;
use serde::Serialize;

use ltd_core::db::Db;
use ltd_core::domain::local_now_ms;
use ltd_core::error::{err, AppError, AppResult};

use crate::logging;
use store::DeviceStore;

pub use store::DeviceView;

/// 同步协议版本（与第三阶段 A 的 sync_log 协议配套）
pub const PROTOCOL_VERSION: &str = "1";
/// 默认监听端口
pub const DEFAULT_PORT: u16 = 38421;
/// 配对码有效期：5 分钟
const PAIRING_TTL_MS: i64 = 5 * 60 * 1000;
/// 配对码长度（纯数字）
const PAIRING_CODE_LEN: usize = 8;
/// 同时处理的请求上限
const MAX_CONCURRENT_REQUESTS: usize = 32;
/// 优雅关闭等待上限
const GRACEFUL_SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

const FIREWALL_HINT: &str = "若手机无法连接：请确认电脑与手机连接同一 Wi-Fi，并在 Windows 防火墙中允许本程序访问「专用网络」。本程序不会自动修改防火墙规则。";

/// 当前服务状态（供桌面端展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub running: bool,
    pub protocol_version: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub fingerprint: Option<String>,
    pub started_at: Option<i64>,
    pub device_count: usize,
    pub pairing_active: bool,
    pub pairing_expires_at: Option<i64>,
    pub firewall_hint: String,
    /// 设备文件等非致命错误
    pub error: Option<String>,
}

/// 桌面端展示的一次性配对信息（含二维码）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub code: String,
    pub expires_at: i64,
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub protocol_version: String,
    /// 二维码内容（JSON，供 Android 端解析）
    pub payload: String,
    /// 二维码 SVG
    pub qr_svg: String,
}

/// 一次性配对码。保存在内存中，服务停止或重新生成即失效。
#[derive(Debug, Clone)]
pub(crate) struct PairingSession {
    pub code: String,
    pub expires_at: i64,
}

impl PairingSession {
    pub(crate) fn is_valid(&self, now: i64) -> bool {
        now <= self.expires_at
    }
}

struct RunningServer {
    addr: SocketAddr,
    fingerprint: String,
    started_at: i64,
    alive: Arc<AtomicBool>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// 同步服务句柄，由 `AppState` 持有。
pub struct SyncService {
    db: Arc<Db>,
    dir: PathBuf,
    devices: Arc<DeviceStore>,
    pairing: Arc<Mutex<Option<PairingSession>>>,
    pair_failures: Arc<routes::PairFailureWindow>,
    running: Mutex<Option<RunningServer>>,
}

impl SyncService {
    pub fn new(db: Arc<Db>, dir: PathBuf) -> Self {
        let devices = Arc::new(DeviceStore::new(dir.join("devices.json")));
        Self {
            db,
            dir,
            devices,
            pairing: Arc::new(Mutex::new(None)),
            pair_failures: Arc::new(routes::PairFailureWindow::new()),
            running: Mutex::new(None),
        }
    }

    pub fn status(&self) -> SyncStatus {
        let now = local_now_ms();
        let running = {
            let mut guard = self.lock_running();
            match guard.as_ref() {
                Some(r) if r.alive.load(Ordering::SeqCst) => {
                    Some((r.addr, r.fingerprint.clone(), r.started_at))
                }
                Some(_) => {
                    // 线程已退出（异常退出/端口被系统回收），清理句柄
                    *guard = None;
                    None
                }
                None => None,
            }
        };

        let pairing_expires_at = {
            let mut guard = self.lock_pairing();
            match guard.as_ref() {
                Some(s) if s.is_valid(now) => Some(s.expires_at),
                Some(_) => {
                    *guard = None;
                    None
                }
                None => None,
            }
        };

        let (device_count, error) = match self.devices.count() {
            Ok(n) => (n, None),
            Err(e) => {
                logging::warn(format!("读取已配对设备失败: {e}"));
                (0, Some(e.to_string()))
            }
        };

        SyncStatus {
            running: running.is_some(),
            protocol_version: PROTOCOL_VERSION.to_string(),
            host: running.as_ref().map(|(addr, _, _)| addr.ip().to_string()),
            port: running.as_ref().map(|(addr, _, _)| addr.port()),
            fingerprint: running.as_ref().map(|(_, fp, _)| fp.clone()),
            started_at: running.as_ref().map(|(_, _, at)| *at),
            device_count,
            pairing_active: pairing_expires_at.is_some(),
            pairing_expires_at,
            firewall_hint: FIREWALL_HINT.to_string(),
            error,
        }
    }

    /// 在自动检测到的局域网地址上启动服务
    pub fn start(&self) -> AppResult<SyncStatus> {
        let ip = default_bind_ip()?;
        self.start_with(ip, DEFAULT_PORT)
    }

    /// 在指定地址/端口启动（端口 0 由系统分配，仅测试使用）。
    pub fn start_with(&self, bind_ip: IpAddr, port: u16) -> AppResult<SyncStatus> {
        let already = {
            let guard = self.lock_running();
            guard
                .as_ref()
                .map(|r| r.alive.load(Ordering::SeqCst))
                .unwrap_or(false)
        };
        if already {
            return Ok(self.status());
        }
        {
            let mut guard = self.lock_running();
            *guard = None;
        }

        // 证书：缺失时生成；若重新生成则旧设备的证书锁定会失效，需要重新配对
        let tls = tls::load_or_create(&self.dir.join("tls"), bind_ip)?;
        if tls.created {
            match self.devices.count() {
                Ok(n) if n > 0 => {
                    self.devices.clear()?;
                    logging::warn("同步证书已重新生成，原有已配对设备已清除，需要重新配对");
                }
                _ => {}
            }
        }
        // 提前暴露设备文件解析错误，避免服务启动后才发现
        self.devices.list()?;

        // 先同步绑定端口，端口冲突立即返回明确错误
        let addr = SocketAddr::new(bind_ip, port);
        let std_listener = std::net::TcpListener::bind(addr).map_err(|e| map_bind_error(e, addr))?;
        std_listener.set_nonblocking(true)?;
        let addr = std_listener.local_addr()?;

        let now = local_now_ms();
        let ctx = Arc::new(routes::ServerContext::new(
            self.db.clone(),
            self.devices.clone(),
            self.pairing.clone(),
            self.pair_failures.clone(),
            addr,
            tls.fingerprint.clone(),
            now,
            MAX_CONCURRENT_REQUESTS,
        ));
        let router = routes::router(ctx);

        let tls_config = tls.config.clone();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let alive = Arc::new(AtomicBool::new(true));
        let alive_thread = alive.clone();
        let thread = std::thread::Builder::new()
            .name("lan-sync-server".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        logging::error(format!("创建同步服务运行时失败: {e}"));
                        alive_thread.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                runtime.block_on(async move {
                    let listener = match tokio::net::TcpListener::from_std(std_listener) {
                        Ok(l) => l,
                        Err(e) => {
                            logging::error(format!("接管同步监听端口失败: {e}"));
                            return;
                        }
                    };
                    let tls_listener = TlsListener {
                        inner: listener,
                        acceptor: tokio_rustls::TlsAcceptor::from(tls_config),
                    };
                    let (close_tx, close_rx) = tokio::sync::oneshot::channel::<()>();
                    let serve = axum::serve(tls_listener, router).with_graceful_shutdown(async move {
                        let _ = shutdown_rx.await;
                        let _ = close_tx.send(());
                    });
                    tokio::select! {
                        result = serve => {
                            if let Err(e) = result {
                                logging::warn(format!("局域网同步服务异常退出: {e}"));
                            }
                        }
                        _ = async move {
                            // 收到停止信号后再等待有限时间，避免慢速连接长期占用端口
                            let _ = close_rx.await;
                            tokio::time::sleep(GRACEFUL_SHUTDOWN_WAIT).await;
                        } => {
                            logging::warn("局域网同步服务未能优雅关闭，已强制释放监听端口");
                        }
                    }
                });
                alive_thread.store(false, Ordering::SeqCst);
            })
            .map_err(|e| AppError::Other(format!("启动同步服务线程失败: {e}")))?;

        {
            let mut guard = self.lock_running();
            *guard = Some(RunningServer {
                addr,
                fingerprint: tls.fingerprint.clone(),
                started_at: now,
                alive,
                shutdown: Some(shutdown_tx),
                thread: Some(thread),
            });
        }
        logging::info(format!(
            "局域网同步服务已启动：https://{}（协议 v{}，证书指纹 {}）",
            addr, PROTOCOL_VERSION, tls.fingerprint
        ));
        Ok(self.status())
    }

    pub fn stop(&self) -> AppResult<SyncStatus> {
        self.stop_inner();
        Ok(self.status())
    }

    fn stop_inner(&self) {
        let running = { self.lock_running().take() };
        if let Some(mut running) = running {
            if let Some(tx) = running.shutdown.take() {
                let _ = tx.send(());
            }
            if let Some(thread) = running.thread.take() {
                let _ = thread.join();
            }
            *self.lock_pairing() = None;
            logging::info("局域网同步服务已停止");
        }
    }

    /// 生成一次性配对码与二维码（服务必须处于运行状态）
    pub fn create_pairing(&self) -> AppResult<PairingInfo> {
        self.create_pairing_with_ttl(PAIRING_TTL_MS)
    }

    #[doc(hidden)]
    pub fn create_pairing_with_ttl(&self, ttl_ms: i64) -> AppResult<PairingInfo> {
        let running = {
            let guard = self.lock_running();
            match guard.as_ref() {
                Some(r) if r.alive.load(Ordering::SeqCst) => {
                    Some((r.addr, r.fingerprint.clone()))
                }
                _ => None,
            }
        };
        let Some((addr, fingerprint)) = running else {
            return err("请先开启局域网同步服务");
        };

        let code = new_pairing_code();
        let now = local_now_ms();
        let expires_at = now.saturating_add(ttl_ms.max(1));
        *self.lock_pairing() = Some(PairingSession {
            code: code.clone(),
            expires_at,
        });

        let host = addr.ip().to_string();
        let port = addr.port();
        let payload = serde_json::json!({
            "type": "ltd-pair",
            "v": 1,
            "protocol": PROTOCOL_VERSION,
            "host": host,
            "port": port,
            "fingerprint": fingerprint,
            "code": code,
            "expiresAt": expires_at,
        });
        let payload = serde_json::to_string(&payload)?;
        let qr_svg = render_qr_svg(&payload)?;

        Ok(PairingInfo {
            code,
            expires_at,
            host,
            port,
            fingerprint,
            protocol_version: PROTOCOL_VERSION.to_string(),
            payload,
            qr_svg,
        })
    }

    pub fn cancel_pairing(&self) {
        *self.lock_pairing() = None;
    }

    pub fn list_devices(&self) -> AppResult<Vec<DeviceView>> {
        self.devices.list()
    }

    pub fn revoke_device(&self, id: &str) -> AppResult<Vec<DeviceView>> {
        if !self.devices.revoke(id)? {
            return err("未找到该设备");
        }
        logging::info("已撤销一个同步设备授权");
        self.devices.list()
    }

    /// 重置配对：清除全部设备、删除并重新生成证书；若服务原本在运行则自动重启。
    pub fn reset_pairing(&self) -> AppResult<SyncStatus> {
        let previous = {
            let guard = self.lock_running();
            guard.as_ref().map(|r| r.addr)
        };
        self.stop_inner();
        *self.lock_pairing() = None;
        self.devices.clear()?;
        tls::remove_all(&self.dir.join("tls"))?;
        logging::info("已重置同步配对：清除设备并删除旧证书");
        if let Some(addr) = previous {
            self.start_with(addr.ip(), addr.port())?;
        }
        Ok(self.status())
    }

    #[cfg(test)]
    pub(crate) fn ca_cert_path(&self) -> PathBuf {
        self.dir.join("tls").join("ca.crt")
    }

    #[allow(dead_code)]
    pub(crate) fn device_store_path(&self) -> PathBuf {
        self.devices.path().to_path_buf()
    }

    fn lock_running(&self) -> std::sync::MutexGuard<'_, Option<RunningServer>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_pairing(&self) -> std::sync::MutexGuard<'_, Option<PairingSession>> {
        self.pairing.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for SyncService {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.running.lock() {
            if let Some(mut running) = guard.take() {
                if let Some(tx) = running.shutdown.take() {
                    let _ = tx.send(());
                }
                if let Some(thread) = running.thread.take() {
                    let _ = thread.join();
                }
            }
        }
    }
}

/// 生成 32 字节密码学安全随机令牌（十六进制表示）
pub(crate) fn new_device_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn new_pairing_code() -> String {
    let modulus: u32 = 100_000_000;
    let limit = u32::MAX - (u32::MAX % modulus);
    let mut rng = rand::rngs::OsRng;
    loop {
        let n = rng.next_u32();
        if n < limit {
            return format!("{:08}", n % modulus);
        }
    }
}

fn render_qr_svg(payload: &str) -> AppResult<String> {
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|e| AppError::Other(format!("生成配对二维码失败: {e}")))?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(240, 240)
        .build())
}

/// 检测默认路由使用的局域网 IPv4 地址（UDP 连接不会真正发包）
pub fn detect_lan_ip() -> Option<IpAddr> {
    let local = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
    let remote = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 80);
    let socket = std::net::UdpSocket::bind(local).ok()?;
    socket.connect(remote).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn default_bind_ip() -> AppResult<IpAddr> {
    let ip = detect_lan_ip().ok_or_else(|| {
        AppError::Other("未检测到可用的局域网 IPv4 地址，请先连接家庭 Wi-Fi 或有线网络".into())
    })?;
    if !is_private_ip(ip) {
        return err(format!(
            "检测到的地址 {ip} 不是家庭局域网地址，已拒绝在公网地址上监听"
        ));
    }
    Ok(ip)
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(_) => false,
    }
}

fn map_bind_error(e: std::io::Error, addr: SocketAddr) -> AppError {
    match e.kind() {
        std::io::ErrorKind::AddrInUse => AppError::Other(format!(
            "端口 {} 已被占用，无法启动局域网同步服务（{}）",
            addr.port(),
            addr.ip()
        )),
        std::io::ErrorKind::PermissionDenied => AppError::Other(format!(
            "没有权限监听 {}:{}，请检查系统权限或防火墙设置",
            addr.ip(),
            addr.port()
        )),
        _ => AppError::Other(format!("无法监听 {}:{}：{e}", addr.ip(), addr.port())),
    }
}

/// 在 TCP 监听之上完成 TLS 握手，交给 `axum::serve` 处理。
struct TlsListener {
    inner: tokio::net::TcpListener,
    acceptor: tokio_rustls::TlsAcceptor,
}

impl axum::serve::Listener for TlsListener {
    type Io = tokio_rustls::server::TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.inner.accept().await {
                Ok((stream, addr)) => match self.acceptor.accept(stream).await {
                    Ok(tls) => return (tls, addr),
                    // 握手失败（扫描、错误指纹等）直接丢弃连接，继续接受新连接
                    Err(_) => continue,
                },
                Err(e) => {
                    // 避免 accept 持续失败时忙等
                    logging::warn(format!("局域网同步服务接受连接失败: {e}"));
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}
