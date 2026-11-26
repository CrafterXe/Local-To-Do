//! Android 同步引擎：配对、首次快照、增量同步、reset 与离线状态机。
//!
//! 只读约束：这里没有任何任务写接口，所有数据库写入均由同步结果驱动；
//! 每次同步都重新执行证书指纹校验，失败时绝不清空本地镜像。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use ltd_core::domain::local_now_ms;
use ltd_core::error::AppError;
use ltd_core::mirror::{MirrorCounts, MirrorDb};
use ltd_core::sync::SyncSnapshot;

use super::http::{ClientError, HttpResponse, PinnedHttpClient};
use super::pairing::{self, PairingPreview};
use super::pinned::normalize_fingerprint;
use super::secrets::SecretStore;
use super::PROTOCOL_VERSION;

/// 单页增量条数（服务端上限 1000，这里取较小值以便及时保存进度）
const PAGE_LIMIT: i64 = 500;
/// 单次同步最多翻页数，避免异常服务端造成死循环
const MAX_PAGES: usize = 1000;

/// 与 Windows 的已配对连接信息（不含令牌）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub protocol_version: String,
    pub device_id: String,
    pub device_name: String,
    pub paired_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncPhase {
    Unpaired,
    Pairing,
    Paired,
    FirstSync,
    Syncing,
    Synced,
    Offline,
    Failed,
    NeedsPairing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncErrorKind {
    NotPaired,
    Offline,
    NeedsPairing,
    CertChanged,
    Protocol,
    Server,
    Busy,
    Store,
    Internal,
}

#[derive(Debug, Clone)]
pub struct SyncError {
    pub kind: SyncErrorKind,
    pub message: String,
}

impl SyncError {
    pub fn new(kind: SyncErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SyncError {}

impl From<AppError> for SyncError {
    fn from(e: AppError) -> Self {
        Self::new(SyncErrorKind::Internal, e.to_string())
    }
}

pub type SyncResult<T> = Result<T, SyncError>;

/// 供移动端 UI 展示的同步状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSyncState {
    pub paired: bool,
    pub connection: Option<ConnectionInfo>,
    pub phase: SyncPhase,
    pub last_sync_at: Option<i64>,
    pub last_rev: i64,
    pub needs_snapshot: bool,
    pub counts: MirrorCounts,
    pub error: Option<String>,
}

struct RuntimeStatus {
    phase: SyncPhase,
    error: Option<String>,
}

pub struct MobileSyncService {
    dir: PathBuf,
    secrets: Arc<dyn SecretStore>,
    mirror: Mutex<Option<Arc<MirrorDb>>>,
    runtime: Mutex<RuntimeStatus>,
    gate: tokio::sync::Mutex<()>,
}

impl MobileSyncService {
    pub fn new(dir: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            dir,
            secrets,
            mirror: Mutex::new(None),
            runtime: Mutex::new(RuntimeStatus {
                phase: SyncPhase::Paired,
                error: None,
            }),
            gate: tokio::sync::Mutex::new(()),
        }
    }

    /// 生成移动端状态。只读，不触发网络或同步。
    pub fn state(&self) -> MobileSyncState {
        let connection = self.load_connection().ok().flatten();
        let mirror = self.mirror().ok();
        let meta = mirror.as_ref().and_then(|m| m.meta().ok());
        let counts = mirror
            .as_ref()
            .and_then(|m| m.counts().ok())
            .unwrap_or(MirrorCounts {
                lists: 0,
                tasks: 0,
                recurrences: 0,
                steps: 0,
            });

        let runtime = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        let paired = connection.is_some();
        let mut phase = if paired {
            runtime.phase
        } else {
            SyncPhase::Unpaired
        };
        let mut error = runtime.error.clone();
        let needs_snapshot = meta.as_ref().map(|m| m.needs_snapshot).unwrap_or(false);
        if paired && needs_snapshot {
            phase = SyncPhase::Failed;
            if error.is_none() {
                error = Some("本地镜像需要重新获取完整快照，请点击「立即同步」".into());
            }
        }

        MobileSyncState {
            paired,
            connection,
            phase,
            last_sync_at: meta.as_ref().and_then(|m| m.last_sync_at),
            last_rev: meta.as_ref().map(|m| m.last_rev).unwrap_or(0),
            needs_snapshot,
            counts,
            error,
        }
    }

    /// 校验二维码并返回服务器信息（不联网）
    pub fn inspect_pairing(&self, payload: &str) -> SyncResult<PairingPreview> {
        pairing::preview(payload, local_now_ms())
            .map_err(|e| SyncError::new(SyncErrorKind::Protocol, e.to_string()))
    }

    /// 完成配对：先校验证书指纹，再用一次性配对码换取设备令牌。
    pub async fn pair(&self, payload_json: &str, device_name: &str) -> SyncResult<MobileSyncState> {
        let now = local_now_ms();
        let payload = pairing::parse_payload(payload_json, now)
            .map_err(|e| SyncError::new(SyncErrorKind::Protocol, e.to_string()))?;

        let _guard = self
            .gate
            .try_lock()
            .map_err(|_| SyncError::new(SyncErrorKind::Busy, "配对或同步正在进行，请稍后重试"))?;

        self.set_phase(SyncPhase::Pairing, None);
        let result = self.pair_inner(&payload, device_name, now).await;
        match &result {
            Ok(state) => {
                self.set_phase(SyncPhase::Paired, None);
                Ok(state.clone())
            }
            Err(e) => {
                let phase = match e.kind {
                    SyncErrorKind::Busy => SyncPhase::Pairing,
                    SyncErrorKind::NeedsPairing | SyncErrorKind::CertChanged => {
                        SyncPhase::NeedsPairing
                    }
                    SyncErrorKind::Offline => SyncPhase::Offline,
                    _ => SyncPhase::Failed,
                };
                self.set_phase(phase, Some(e.message.clone()));
                Err(e.clone())
            }
        }
    }

    async fn pair_inner(
        &self,
        payload: &pairing::PairingPayload,
        device_name: &str,
        now: i64,
    ) -> SyncResult<MobileSyncState> {
        let client = PinnedHttpClient::new(&payload.host, payload.port, &payload.fingerprint)
            .map_err(map_client_error)?;

        let status = client
            .request("GET", "/api/v1/status", None, None)
            .await
            .map_err(map_client_error)?;
        check_response(&status)?;
        let protocol = status
            .body
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if protocol != PROTOCOL_VERSION {
            return Err(SyncError::new(
                SyncErrorKind::Protocol,
                "服务器同步协议版本与配对信息不一致",
            ));
        }

        let name = sanitize_device_name(device_name);
        let body = json!({ "code": payload.code, "deviceName": name });
        let response = client
            .request("POST", "/api/v1/pair", None, Some(&body))
            .await
            .map_err(map_client_error)?;

        if response.status == 401 {
            return Err(SyncError::new(
                SyncErrorKind::NeedsPairing,
                "配对码无效、已过期或已被使用，请在电脑上重新生成二维码",
            ));
        }
        if response.status == 429 {
            return Err(SyncError::new(
                SyncErrorKind::Server,
                "配对尝试过于频繁，请稍后再试",
            ));
        }
        check_response(&response)?;

        let parsed: PairResponse = serde_json::from_value(response.body)
            .map_err(|e| SyncError::new(SyncErrorKind::Protocol, format!("配对响应格式无效：{e}")))?;
        if parsed.protocol_version != PROTOCOL_VERSION {
            return Err(SyncError::new(
                SyncErrorKind::Protocol,
                "服务器返回的协议版本不受支持",
            ));
        }
        let expected_pin = normalize_fingerprint(&payload.fingerprint)
            .map_err(|e| SyncError::new(SyncErrorKind::Protocol, e.to_string()))?;
        let actual_pin = normalize_fingerprint(&parsed.fingerprint)
            .map_err(|_| SyncError::new(SyncErrorKind::Protocol, "服务器返回的证书指纹无效"))?;
        if expected_pin != actual_pin {
            return Err(SyncError::new(
                SyncErrorKind::CertChanged,
                "服务器返回的证书指纹与二维码不一致，已拒绝配对",
            ));
        }
        if parsed.server_host != payload.host || parsed.server_port != payload.port {
            return Err(SyncError::new(
                SyncErrorKind::Protocol,
                "服务器返回的连接信息与二维码不一致，已拒绝配对",
            ));
        }
        let token = parsed.token.trim().to_string();
        if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(SyncError::new(
                SyncErrorKind::Protocol,
                "服务器返回的设备令牌格式无效",
            ));
        }

        self.secrets
            .save_token(&token)
            .map_err(|e| SyncError::new(SyncErrorKind::Store, e))?;

        let connection = ConnectionInfo {
            host: payload.host.clone(),
            port: payload.port,
            fingerprint: payload.fingerprint.clone(),
            protocol_version: parsed.protocol_version,
            device_id: parsed.device_id,
            device_name: parsed.device_name,
            paired_at: now,
        };
        if let Err(e) = self.save_connection(&connection) {
            // 配置保存失败时回滚令牌，避免残留无效配对
            let _ = self.secrets.delete_token();
            return Err(e);
        }

        // 提前建立镜像库，保证状态页能显示计数
        self.mirror()?;
        Ok(self.state())
    }

    /// 手动同步：已有同步进行中时立即返回 Busy，不并发写库。
    pub async fn sync_now(&self) -> SyncResult<MobileSyncState> {
        let _guard = self
            .gate
            .try_lock()
            .map_err(|_| SyncError::new(SyncErrorKind::Busy, "同步正在进行中，请稍候"))?;

        let result = self.sync_inner().await;
        match &result {
            Ok(()) => self.set_phase(SyncPhase::Synced, None),
            Err(e) => {
                let phase = match e.kind {
                    SyncErrorKind::NotPaired => SyncPhase::Unpaired,
                    SyncErrorKind::Offline => SyncPhase::Offline,
                    SyncErrorKind::NeedsPairing | SyncErrorKind::CertChanged => {
                        SyncPhase::NeedsPairing
                    }
                    _ => SyncPhase::Failed,
                };
                self.set_phase(phase, Some(e.message.clone()));
            }
        }
        result?;
        Ok(self.state())
    }

    async fn sync_inner(&self) -> SyncResult<()> {
        let connection = self.load_connection()?.ok_or_else(|| {
            SyncError::new(SyncErrorKind::NotPaired, "尚未配对，请先扫描电脑上的配对二维码")
        })?;
        let token = self
            .secrets
            .load_token()
            .map_err(|e| SyncError::new(SyncErrorKind::Store, e))?
            .filter(|t| !t.is_empty())
            .ok_or_else(|| {
                SyncError::new(SyncErrorKind::NeedsPairing, "设备令牌已丢失，请重新扫码配对")
            })?;
        let mirror = self.mirror()?;
        let client = PinnedHttpClient::new(&connection.host, connection.port, &connection.fingerprint)
            .map_err(map_client_error)?;

        let meta = mirror.meta()?;
        if meta.needs_snapshot || !meta.initialized {
            self.fetch_snapshot(&client, &mirror, &token).await?;
            return Ok(());
        }

        self.set_phase(SyncPhase::Syncing, None);
        for _ in 0..MAX_PAGES {
            let current = mirror.meta()?;
            let path = format!(
                "/api/v1/sync/changes?since={}&limit={}",
                current.last_rev, PAGE_LIMIT
            );
            let response = client
                .request("GET", &path, Some(&token), None)
                .await
                .map_err(map_client_error)?;
            check_response(&response)?;

            let parsed: ChangesResponse = serde_json::from_value(response.body.clone())
                .map_err(|e| SyncError::new(SyncErrorKind::Protocol, format!("增量响应格式无效：{e}")))?;
            if parsed.protocol_version != PROTOCOL_VERSION {
                return Err(SyncError::new(
                    SyncErrorKind::Protocol,
                    "服务器同步协议版本不受支持",
                ));
            }
            if parsed.changes.is_empty() {
                if parsed.has_more {
                    return Err(SyncError::new(
                        SyncErrorKind::Protocol,
                        "服务器分页状态异常，已停止本次同步",
                    ));
                }
                break;
            }

            let outcome = mirror.apply_changes(&parsed.changes)?;
            if outcome.reset {
                // 备份恢复：镜像已在事务内清空并标记 needs_snapshot
                self.fetch_snapshot(&client, &mirror, &token).await?;
                return Ok(());
            }
            if !parsed.has_more {
                break;
            }
        }

        mirror.mark_sync_success(local_now_ms())?;
        Ok(())
    }

    async fn fetch_snapshot(
        &self,
        client: &PinnedHttpClient,
        mirror: &MirrorDb,
        token: &str,
    ) -> SyncResult<()> {
        self.set_phase(SyncPhase::FirstSync, None);
        let response = client
            .request("GET", "/api/v1/sync/snapshot", Some(token), None)
            .await
            .map_err(map_client_error)?;
        check_response(&response)?;

        let parsed: SnapshotResponse = serde_json::from_value(response.body)
            .map_err(|e| SyncError::new(SyncErrorKind::Protocol, format!("快照响应格式无效：{e}")))?;
        if parsed.protocol_version != PROTOCOL_VERSION {
            return Err(SyncError::new(
                SyncErrorKind::Protocol,
                "服务器同步协议版本不受支持",
            ));
        }
        mirror.apply_snapshot(&parsed.snapshot, local_now_ms())?;
        Ok(())
    }

    /// 解除配对：删除令牌与连接配置，保留最近一次同步的镜像供离线查看。
    pub fn unpair(&self) -> SyncResult<MobileSyncState> {
        self.secrets
            .delete_token()
            .map_err(|e| SyncError::new(SyncErrorKind::Store, e))?;
        match std::fs::remove_file(self.connection_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(SyncError::new(
                    SyncErrorKind::Store,
                    format!("删除连接配置失败：{e}"),
                ))
            }
        }
        self.set_phase(SyncPhase::Unpaired, None);
        Ok(self.state())
    }

    fn set_phase(&self, phase: SyncPhase, error: Option<String>) {
        let mut runtime = self.runtime.lock().unwrap_or_else(|e| e.into_inner());
        runtime.phase = phase;
        runtime.error = error;
    }

    fn mirror(&self) -> SyncResult<Arc<MirrorDb>> {
        let mut guard = self.mirror.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(db) = guard.as_ref() {
            return Ok(db.clone());
        }
        let path = self.dir.join("mirror").join("mirror.db");
        let db = Arc::new(MirrorDb::open(&path)?);
        *guard = Some(db.clone());
        Ok(db)
    }

    fn connection_path(&self) -> PathBuf {
        self.dir.join("connection.json")
    }

    fn load_connection(&self) -> SyncResult<Option<ConnectionInfo>> {
        let path = self.connection_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(SyncError::new(
                    SyncErrorKind::Store,
                    format!("读取连接配置失败：{e}"),
                ))
            }
        };
        let info: ConnectionInfo = serde_json::from_str(&text).map_err(|e| {
            SyncError::new(
                SyncErrorKind::Store,
                format!("连接配置已损坏，请重新配对：{e}"),
            )
        })?;
        Ok(Some(info))
    }

    fn save_connection(&self, info: &ConnectionInfo) -> SyncResult<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            SyncError::new(SyncErrorKind::Store, format!("创建同步目录失败：{e}"))
        })?;
        let path = self.connection_path();
        let text = serde_json::to_vec_pretty(info)
            .map_err(|e| SyncError::new(SyncErrorKind::Store, format!("序列化连接配置失败：{e}")))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &text)
            .map_err(|e| SyncError::new(SyncErrorKind::Store, format!("写入连接配置失败：{e}")))?;
        if std::fs::rename(&tmp, &path).is_err() {
            // Windows 不允许 rename 覆盖已有文件
            let _ = std::fs::remove_file(&path);
            std::fs::rename(&tmp, &path).map_err(|e| {
                SyncError::new(SyncErrorKind::Store, format!("保存连接配置失败：{e}"))
            })?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairResponse {
    device_id: String,
    device_name: String,
    token: String,
    protocol_version: String,
    server_host: String,
    server_port: u16,
    fingerprint: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotResponse {
    protocol_version: String,
    #[serde(default, rename = "serverTime")]
    _server_time: Option<i64>,
    snapshot: SyncSnapshot,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangesResponse {
    protocol_version: String,
    #[serde(default, rename = "latestRev")]
    _latest_rev: i64,
    has_more: bool,
    changes: Vec<ltd_core::sync::SyncChange>,
}

fn check_response(response: &HttpResponse) -> SyncResult<()> {
    if response.status == 200 {
        return Ok(());
    }
    let detail = response
        .body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match response.status {
        401 | 403 => Err(SyncError::new(
            SyncErrorKind::NeedsPairing,
            "设备未授权或令牌已失效，请重新扫码配对",
        )),
        429 => Err(SyncError::new(
            SyncErrorKind::Server,
            "同步请求过于频繁，请稍后重试",
        )),
        status if status >= 500 => Err(SyncError::new(
            SyncErrorKind::Server,
            if detail.is_empty() {
                format!("Windows 同步服务出错（HTTP {status}）")
            } else {
                format!("Windows 同步服务出错：{detail}")
            },
        )),
        status => Err(SyncError::new(
            SyncErrorKind::Server,
            if detail.is_empty() {
                format!("同步请求失败（HTTP {status}）")
            } else {
                detail
            },
        )),
    }
}

fn map_client_error(error: ClientError) -> SyncError {
    match error {
        ClientError::Connect(message) | ClientError::Timeout(message) => {
            SyncError::new(SyncErrorKind::Offline, message)
        }
        ClientError::TlsPin(message) => SyncError::new(SyncErrorKind::CertChanged, message),
        // 非指纹类握手失败通常是连接被中断或服务未就绪，按离线处理
        ClientError::Tls(message) => SyncError::new(SyncErrorKind::Offline, message),
        ClientError::Io(message) => SyncError::new(SyncErrorKind::Offline, message),
        ClientError::Protocol(message) => SyncError::new(SyncErrorKind::Protocol, message),
    }
}

fn sanitize_device_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Android 手机".into()
    } else {
        trimmed.chars().take(64).collect()
    }
}
