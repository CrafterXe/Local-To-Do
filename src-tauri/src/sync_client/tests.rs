//! 客户端集成测试：临时证书 + 临时数据库 + 本地 mock HTTPS 服务端。
//!
//! 不触碰用户真实数据库，不访问外网。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use ltd_core::domain::local_now_ms;
use ltd_core::mirror::MirrorDb;
use ltd_core::sync::SyncChange;

use super::engine::{MobileSyncService, SyncErrorKind};
use super::http::{ClientError, PinnedHttpClient};
use super::pairing;
use super::pinned::{fingerprint_of_der, format_fingerprint};
use super::secrets::MemorySecretStore;

// ---------------- 临时证书与 mock 服务端 ----------------

struct TestCerts {
    fingerprint: String,
    config: Arc<rustls::ServerConfig>,
}

impl TestCerts {
    fn generate() -> Self {
        let key = rcgen::KeyPair::generate().expect("生成测试私钥失败");
        let mut params =
            rcgen::CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
                .expect("构造测试证书参数失败");
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Local To Do Test");
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        let cert = params.self_signed(&key).expect("签发测试证书失败");
        let der = cert.der().to_vec();
        let fingerprint = format_fingerprint(&fingerprint_of_der(&der));

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("TLS 版本配置失败")
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(der)],
                rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
            )
            .expect("测试服务端 TLS 配置失败");

        Self {
            fingerprint,
            config: Arc::new(config),
        }
    }
}

#[derive(Clone)]
enum ChangesStep {
    Json(Value),
    Status(u16, Value),
    Close,
}

struct MockState {
    token: String,
    offline: bool,
    reject_token: bool,
    delay_ms: u64,
    pair_status: u16,
    pair_body: Value,
    snapshot: Value,
    changes: Vec<ChangesStep>,
    changes_cursor: usize,
    requests: Vec<String>,
}

struct MockServer {
    addr: SocketAddr,
    fingerprint: String,
    state: Arc<Mutex<MockState>>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MockServer {
    fn start() -> Self {
        let certs = TestCerts::generate();
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定测试端口失败");
        std_listener.set_nonblocking(true).expect("设置非阻塞失败");
        let addr = std_listener.local_addr().expect("读取测试端口失败");
        let token = "a".repeat(64);
        let state = Arc::new(Mutex::new(MockState {
            token: token.clone(),
            offline: false,
            reject_token: false,
            delay_ms: 0,
            pair_status: 200,
            pair_body: json!({
                "deviceId": "dev-test",
                "deviceName": "测试手机",
                "token": token,
                "protocolVersion": "1",
                "serverHost": "127.0.0.1",
                "serverPort": addr.port(),
                "fingerprint": certs.fingerprint.clone(),
            }),
            snapshot: empty_snapshot_body(0),
            changes: Vec::new(),
            changes_cursor: 0,
            requests: Vec::new(),
        }));

        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let handler_state = state.clone();
        let config = certs.config.clone();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("创建测试运行时失败");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(std_listener).expect("接管测试端口失败");
                let acceptor = tokio_rustls::TlsAcceptor::from(config);
                loop {
                    tokio::select! {
                        _ = &mut rx => break,
                        accepted = listener.accept() => {
                            let Ok((tcp, _)) = accepted else { continue };
                            // 模拟服务不可达：不完成 TLS 握手，直接断开
                            if handler_state.lock().unwrap().offline {
                                drop(tcp);
                                continue;
                            }
                            let acceptor = acceptor.clone();
                            let state = handler_state.clone();
                            tokio::spawn(async move {
                                if let Ok(mut tls) = acceptor.accept(tcp).await {
                                    let _ = handle_connection(&mut tls, state).await;
                                }
                            });
                        }
                    }
                }
            });
        });

        Self {
            addr,
            fingerprint: certs.fingerprint.clone(),
            state,
            shutdown: Some(tx),
            thread: Some(thread),
        }
    }

    fn port(&self) -> u16 {
        self.addr.port()
    }

    fn fingerprint(&self) -> String {
        self.fingerprint.clone()
    }

    fn token(&self) -> String {
        self.state.lock().unwrap().token.clone()
    }

    fn set_snapshot(&self, body: Value) {
        self.state.lock().unwrap().snapshot = body;
    }

    fn push_changes(&self, step: ChangesStep) {
        self.state.lock().unwrap().changes.push(step);
    }

    fn set_offline(&self, offline: bool) {
        self.state.lock().unwrap().offline = offline;
    }

    fn set_reject_token(&self, reject: bool) {
        self.state.lock().unwrap().reject_token = reject;
    }

    fn set_pair_status(&self, status: u16, body: Value) {
        let mut state = self.state.lock().unwrap();
        state.pair_status = status;
        state.pair_body = body;
    }

    fn set_delay(&self, delay_ms: u64) {
        self.state.lock().unwrap().delay_ms = delay_ms;
    }

    fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().requests.clone()
    }

    fn qr_payload(&self, code: &str, ttl_ms: i64) -> String {
        json!({
            "type": "ltd-pair",
            "v": 1,
            "protocol": "1",
            "host": "127.0.0.1",
            "port": self.port(),
            "fingerprint": self.fingerprint,
            "code": code,
            "expiresAt": local_now_ms() + ttl_ms,
        })
        .to_string()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct MockRequest {
    method: String,
    path: String,
    auth: Option<String>,
}

async fn handle_connection(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    state: Arc<Mutex<MockState>>,
) -> std::io::Result<()> {
    let Some(request) = read_request(stream).await? else {
        return Ok(());
    };

    let (offline, delay_ms, reject, snapshot, pair_status, pair_body, step) = {
        let mut guard = state.lock().unwrap();
        guard
            .requests
            .push(format!("{} {}", request.method, request.path));
        let expected = format!("Bearer {}", guard.token);
        let authorized = request.auth.as_deref() == Some(expected.as_str());
        let step = if request.path.starts_with("/api/v1/sync/changes") {
            if guard.changes_cursor < guard.changes.len() {
                let step = guard.changes[guard.changes_cursor].clone();
                guard.changes_cursor += 1;
                Some(step)
            } else {
                None
            }
        } else {
            None
        };
        (
            guard.offline,
            guard.delay_ms,
            guard.reject_token || !authorized,
            guard.snapshot.clone(),
            guard.pair_status,
            guard.pair_body.clone(),
            step,
        )
    };

    // 模拟服务不可达：直接关闭连接，不返回任何数据
    if offline {
        return Ok(());
    }
    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    let protected = request.path.starts_with("/api/v1/sync/");
    let (status, body) = if request.path.starts_with("/api/v1/status") {
        (
            200,
            json!({
                "protocolVersion": "1",
                "serverName": "Local To Do",
                "time": 0,
                "pairingAvailable": true,
            }),
        )
    } else if request.path.starts_with("/api/v1/pair") {
        (pair_status, pair_body)
    } else if protected && reject {
        (401, unauthorized_body())
    } else if request.path.starts_with("/api/v1/sync/snapshot") {
        (200, snapshot)
    } else if request.path.starts_with("/api/v1/sync/changes") {
        match step {
            Some(ChangesStep::Json(value)) => (200, value),
            Some(ChangesStep::Status(status, value)) => (status, value),
            Some(ChangesStep::Close) => return Ok(()),
            None => (200, empty_changes_body()),
        }
    } else {
        (404, json!({"error": "not_found", "message": "未知接口"}))
    };

    write_response(stream, status, &body).await
}

async fn read_request(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
) -> std::io::Result<Option<MockRequest>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > 64 * 1024 {
            return Ok(None);
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();

    let mut content_length = 0usize;
    let mut auth = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            if name == "content-length" {
                content_length = value.trim().parse().unwrap_or(0);
            } else if name == "authorization" {
                auth = Some(value.trim().to_string());
            }
        }
    }
    let mut received = buf.len() - (head_end + 4);
    while received < content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        received += n;
    }
    Ok(Some(MockRequest { method, path, auth }))
}

async fn write_response(
    stream: &mut tokio_rustls::server::TlsStream<tokio::net::TcpStream>,
    status: u16,
    body: &Value,
) -> std::io::Result<()> {
    let text = body.to_string();
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(text.as_bytes()).await?;
    stream.flush().await?;
    let _ = stream.shutdown().await;
    Ok(())
}

fn unauthorized_body() -> Value {
    json!({"error": "unauthorized", "message": "设备未授权或令牌已失效"})
}

fn empty_changes_body() -> Value {
    json!({
        "protocolVersion": "1",
        "serverTime": 0,
        "latestRev": 0,
        "hasMore": false,
        "changes": [],
    })
}

// ---------------- 数据构造 ----------------

fn list_json(id: &str, name: &str) -> Value {
    json!({
        "id": id, "name": name, "color": "#2f6fed", "icon": "",
        "sortOrder": 0, "createdAt": 1, "deletedAt": null,
    })
}

fn recurrence_json(id: &str) -> Value {
    json!({
        "id": id, "kind": "daily", "interval": 1, "weekdays": "",
        "monthDay": null, "baseDate": "2026-01-01", "createdAt": 1,
    })
}

fn task_json(id: &str, list_id: &str, title: &str, recurrence: Option<&str>) -> Value {
    json!({
        "id": id,
        "listId": list_id,
        "title": title,
        "note": "",
        "isImportant": false,
        "isCompleted": false,
        "completedAt": null,
        "dueDate": null,
        "dueTime": null,
        "reminderOffset": null,
        "reminderEnabled": false,
        "reminderAt": null,
        "recurrenceId": recurrence,
        "seriesId": null,
        "parentTaskId": null,
        "recurrenceCount": 0,
        "myDay": null,
        "sortOrder": 0,
        "createdAt": 1,
        "updatedAt": 1,
        "deletedAt": null,
    })
}

fn step_json(id: &str, task_id: &str, title: &str) -> Value {
    json!({
        "id": id, "taskId": task_id, "title": title,
        "isCompleted": false, "sortOrder": 0,
    })
}

fn snapshot_body(rev: i64, lists: Vec<Value>, tasks: Vec<Value>) -> Value {
    json!({
        "protocolVersion": "1",
        "serverTime": 0,
        "snapshot": {
            "rev": rev,
            "lists": lists,
            "recurrences": [recurrence_json("r1")],
            "tasks": tasks,
            "steps": [step_json("s1", "t1", "步骤一")],
        },
    })
}

fn empty_snapshot_body(rev: i64) -> Value {
    json!({
        "protocolVersion": "1",
        "serverTime": 0,
        "snapshot": {"rev": rev, "lists": [], "recurrences": [], "tasks": [], "steps": []},
    })
}

fn sample_snapshot(rev: i64) -> Value {
    snapshot_body(
        rev,
        vec![list_json("l1", "任务")],
        vec![
            task_json("t1", "l1", "任务一", Some("r1")),
            task_json("t2", "l1", "任务二", None),
        ],
    )
}

fn change_json(rev: i64, entity: &str, entity_id: &str, op: &str, payload: Option<Value>) -> Value {
    json!({
        "rev": rev,
        "entity": entity,
        "entityId": entity_id,
        "op": op,
        "payload": payload,
        "changedAt": rev,
    })
}

fn changes_body(changes: Vec<Value>, has_more: bool) -> Value {
    json!({
        "protocolVersion": "1",
        "serverTime": 0,
        "latestRev": 9999,
        "hasMore": has_more,
        "changes": changes,
    })
}

// ---------------- 测试脚手架 ----------------

struct Harness {
    server: MockServer,
    work: PathBuf,
    secrets: Arc<MemorySecretStore>,
    service: Arc<MobileSyncService>,
}

impl Harness {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("ltd-android-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("创建临时目录失败");
        let work = dir.join("mobile-sync");
        let secrets = Arc::new(MemorySecretStore::new());
        let service = Arc::new(MobileSyncService::new(work.clone(), secrets.clone()));
        let server = MockServer::start();
        Self {
            server,
            work,
            secrets,
            service,
        }
    }

    async fn pair_ok(&self) {
        let payload = self.server.qr_payload("12345678", 600_000);
        self.service
            .pair(&payload, "测试手机")
            .await
            .expect("配对应成功");
    }

    async fn sync_ok(&self) {
        self.service.sync_now().await.expect("同步应成功");
    }

    fn mirror_path(&self) -> PathBuf {
        self.work.join("mirror").join("mirror.db")
    }

    fn config_path(&self) -> PathBuf {
        self.work.join("connection.json")
    }
}

fn mirror_count(path: &Path, table: &str) -> i64 {
    let conn = rusqlite::Connection::open(path).expect("打开镜像库失败");
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .expect("查询镜像计数失败")
}

fn mirror_scalar_text(path: &Path, sql: &str) -> Option<String> {
    use rusqlite::OptionalExtension;
    let conn = rusqlite::Connection::open(path).expect("打开镜像库失败");
    conn.query_row(sql, [], |r| r.get(0)).optional().unwrap()
}

fn mirror_scalar_i64(path: &Path, sql: &str) -> Option<i64> {
    use rusqlite::OptionalExtension;
    let conn = rusqlite::Connection::open(path).expect("打开镜像库失败");
    conn.query_row(sql, [], |r| r.get(0)).optional().unwrap()
}

fn mirror_meta(path: &Path) -> ltd_core::mirror::MirrorMeta {
    MirrorDb::open(path).expect("打开镜像库失败").meta().unwrap()
}

fn rewrite_fingerprint(path: &Path, fingerprint: &str) {
    let mut value: Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).expect("读取连接配置失败");
    value["fingerprint"] = json!(fingerprint);
    std::fs::write(path, value.to_string()).unwrap();
}

fn qr_with(
    kind: &str,
    version: i64,
    protocol: &str,
    host: &str,
    port: u16,
    fingerprint: &str,
    code: &str,
    expires_at: i64,
) -> String {
    json!({
        "type": kind,
        "v": version,
        "protocol": protocol,
        "host": host,
        "port": port,
        "fingerprint": fingerprint,
        "code": code,
        "expiresAt": expires_at,
    })
    .to_string()
}

// ---------------- 配对与安全 ----------------

#[test]
fn qr_parse_valid() {
    let fp = "AB".repeat(32);
    let payload = qr_with("ltd-pair", 1, "1", "192.168.1.5", 38421, &fp, "12345678", 10_000);
    let parsed = pairing::parse_payload(&payload, 1_000).expect("合法二维码应解析成功");
    assert_eq!(parsed.host, "192.168.1.5");
    assert_eq!(parsed.port, 38421);
    assert_eq!(parsed.code, "12345678");
    assert_eq!(parsed.expires_at, 10_000);
    assert_eq!(parsed.fingerprint, format_fingerprint(&[0xAB; 32]));
}

#[test]
fn qr_rejects_invalid() {
    let fp = "AB".repeat(32);
    let now = 1_000;
    let cases = vec![
        qr_with("other", 1, "1", "192.168.1.5", 38421, &fp, "12345678", 10_000),
        qr_with("ltd-pair", 2, "1", "192.168.1.5", 38421, &fp, "12345678", 10_000),
        qr_with("ltd-pair", 1, "2", "192.168.1.5", 38421, &fp, "12345678", 10_000),
        qr_with("ltd-pair", 1, "1", "https://evil.example", 38421, &fp, "12345678", 10_000),
        qr_with("ltd-pair", 1, "1", "8.8.8.8", 38421, &fp, "12345678", 10_000),
        qr_with("ltd-pair", 1, "1", "192.168.1.5", 0, &fp, "12345678", 10_000),
        qr_with("ltd-pair", 1, "1", "192.168.1.5", 38421, "zz", "12345678", 10_000),
        qr_with("ltd-pair", 1, "1", "192.168.1.5", 38421, &fp, "123", 10_000),
    ];
    for case in cases {
        assert!(pairing::parse_payload(&case, now).is_err(), "应拒绝：{case}");
    }
}

#[test]
fn qr_rejects_expired() {
    let fp = "AB".repeat(32);
    let payload = qr_with("ltd-pair", 1, "1", "192.168.1.5", 38421, &fp, "12345678", 999);
    assert!(pairing::parse_payload(&payload, 1_000).is_err());
    let preview = pairing::preview(&payload, 1_000).expect("结构仍应可预览");
    assert!(preview.expired);
}

#[tokio::test]
async fn pin_correct_fingerprint_succeeds() {
    let server = MockServer::start();
    let client =
        PinnedHttpClient::new("127.0.0.1", server.port(), &server.fingerprint()).expect("客户端构建失败");
    let response = client
        .request("GET", "/api/v1/status", None, None)
        .await
        .expect("正确指纹应完成握手");
    assert_eq!(response.status, 200);
    assert_eq!(response.body["protocolVersion"], "1");
}

#[tokio::test]
async fn pin_wrong_fingerprint_fails() {
    let server = MockServer::start();
    let other = TestCerts::generate();
    let client = PinnedHttpClient::new("127.0.0.1", server.port(), &other.fingerprint)
        .expect("客户端构建失败");
    let error = client
        .request("GET", "/api/v1/status", None, None)
        .await
        .expect_err("错误指纹必须握手失败");
    assert!(matches!(error, ClientError::TlsPin(_)), "实际错误：{error}");
}

#[tokio::test]
async fn pair_success_persists_connection() {
    let harness = Harness::new("pair-ok");
    harness.pair_ok().await;

    let state = harness.service.state();
    assert!(state.paired);
    let connection = state.connection.expect("应保存连接信息");
    assert_eq!(connection.host, "127.0.0.1");
    assert_eq!(connection.port, harness.server.port());
    assert_eq!(connection.device_id, "dev-test");
    assert_eq!(harness.secrets.peek().unwrap(), harness.server.token());
    assert!(harness.config_path().exists());
}

#[tokio::test]
async fn pair_wrong_code_fails() {
    let harness = Harness::new("pair-bad");
    harness
        .server
        .set_pair_status(401, unauthorized_body());
    let payload = harness.server.qr_payload("00000000", 600_000);
    let error = harness
        .service
        .pair(&payload, "测试手机")
        .await
        .expect_err("错误配对码应失败");
    assert_eq!(error.kind, SyncErrorKind::NeedsPairing);
    assert!(harness.secrets.peek().is_none());
    assert!(!harness.config_path().exists());
}

#[tokio::test]
async fn token_secret_not_in_config() {
    let harness = Harness::new("pair-secret");
    harness.pair_ok().await;
    let config = std::fs::read_to_string(harness.config_path()).unwrap();
    let token = harness.server.token();
    assert!(!config.contains(&token), "连接配置中不得出现明文令牌");
    assert!(!config.contains("token"));
    assert_eq!(harness.secrets.peek().unwrap(), token);
}

#[tokio::test]
async fn invalid_token_marks_needs_pairing() {
    let harness = Harness::new("bad-token");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(7));
    harness.sync_ok().await;

    harness.server.set_reject_token(true);
    let error = harness.service.sync_now().await.expect_err("应立即失效");
    assert_eq!(error.kind, SyncErrorKind::NeedsPairing);
    assert_eq!(mirror_count(&harness.mirror_path(), "tasks"), 2);
    assert_eq!(mirror_meta(&harness.mirror_path()).last_rev, 7);
}

#[tokio::test]
async fn cert_change_rejected_after_pairing() {
    let harness = Harness::new("cert-change");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(3));
    harness.sync_ok().await;

    let decoy = TestCerts::generate();
    rewrite_fingerprint(&harness.config_path(), &decoy.fingerprint);
    let error = harness
        .service
        .sync_now()
        .await
        .expect_err("证书变化必须拒绝连接");
    assert_eq!(error.kind, SyncErrorKind::CertChanged);
    assert_eq!(mirror_count(&harness.mirror_path(), "tasks"), 2);
}

// ---------------- 数据库与同步 ----------------

#[tokio::test]
async fn snapshot_writes_all_entities() {
    let harness = Harness::new("snap-all");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(5));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "lists"), 1);
    assert_eq!(mirror_count(&path, "recurrences"), 1);
    assert_eq!(mirror_count(&path, "tasks"), 2);
    assert_eq!(mirror_count(&path, "task_steps"), 1);
    assert_eq!(
        mirror_scalar_text(&path, "SELECT title FROM tasks WHERE id='t1'").as_deref(),
        Some("任务一")
    );
}

#[tokio::test]
async fn snapshot_rev_saved() {
    let harness = Harness::new("snap-rev");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(42));
    harness.sync_ok().await;

    let meta = mirror_meta(&harness.mirror_path());
    assert!(meta.initialized);
    assert!(!meta.needs_snapshot);
    assert_eq!(meta.last_rev, 42);
    assert!(meta.last_sync_at.is_some());
}

#[tokio::test]
async fn snapshot_failure_rolls_back() {
    let harness = Harness::new("snap-fail");
    harness.pair_ok().await;
    // 先创建镜像库文件，再注入写入失败
    let _ = harness.service.state();
    {
        let conn = rusqlite::Connection::open(harness.mirror_path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_tasks BEFORE INSERT ON tasks
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    }
    harness.server.set_snapshot(sample_snapshot(9));
    assert!(harness.service.sync_now().await.is_err());

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "lists"), 0);
    assert_eq!(mirror_count(&path, "tasks"), 0);
    let meta = mirror_meta(&path);
    assert!(!meta.initialized);
    assert_eq!(meta.last_rev, 0);
    assert!(meta.last_sync_at.is_none());
}

#[tokio::test]
async fn incremental_task_upsert() {
    let harness = Harness::new("inc-task");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![
            change_json(2, "task", "t1", "upsert", Some(task_json("t1", "l1", "已改名", Some("r1")))),
            change_json(3, "task", "t3", "upsert", Some(task_json("t3", "l1", "新任务", None))),
        ],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(
        mirror_scalar_text(&path, "SELECT title FROM tasks WHERE id='t1'").as_deref(),
        Some("已改名")
    );
    assert_eq!(mirror_count(&path, "tasks"), 3);
    assert_eq!(mirror_meta(&path).last_rev, 3);
}

#[tokio::test]
async fn incremental_list_upsert() {
    let harness = Harness::new("inc-list");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "list", "l1", "upsert", Some(list_json("l1", "新清单名")))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(
        mirror_scalar_text(&path, "SELECT name FROM lists WHERE id='l1'").as_deref(),
        Some("新清单名")
    );
}

#[tokio::test]
async fn incremental_recurrence_upsert() {
    let harness = Harness::new("inc-recurrence");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    let mut recurrence = recurrence_json("r1");
    recurrence["kind"] = json!("weekly");
    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "recurrence", "r1", "upsert", Some(recurrence))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(
        mirror_scalar_text(&path, "SELECT kind FROM recurrences WHERE id='r1'").as_deref(),
        Some("weekly")
    );
}

#[tokio::test]
async fn incremental_step_upsert() {
    let harness = Harness::new("inc-step");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "step", "s1", "upsert", Some(step_json("s1", "t1", "新步骤")))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(
        mirror_scalar_text(&path, "SELECT title FROM task_steps WHERE id='s1'").as_deref(),
        Some("新步骤")
    );
}

#[tokio::test]
async fn incremental_delete() {
    let harness = Harness::new("inc-delete");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "task", "t1", "delete", None)],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 1);
    assert_eq!(
        mirror_scalar_i64(&path, "SELECT COUNT(*) FROM tasks WHERE id='t1'"),
        Some(0)
    );
    assert_eq!(
        mirror_scalar_i64(&path, "SELECT COUNT(*) FROM task_steps WHERE task_id='t1'"),
        Some(0)
    );
}

#[tokio::test]
async fn multi_page_incremental_no_loss() {
    let harness = Harness::new("multi-page");
    harness.pair_ok().await;
    harness.server.set_snapshot(snapshot_body(
        1,
        vec![list_json("l1", "任务")],
        Vec::new(),
    ));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(10, "task", "t1", "upsert", Some(task_json("t1", "l1", "第一页", None)))],
        true,
    )));
    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(11, "task", "t2", "upsert", Some(task_json("t2", "l1", "第二页", None)))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 2);
    assert_eq!(mirror_meta(&path).last_rev, 11);
    let changes_requests: Vec<String> = harness
        .server
        .requests()
        .into_iter()
        .filter(|r| r.contains("/sync/changes"))
        .collect();
    assert_eq!(changes_requests.len(), 2);
    assert!(changes_requests[1].contains("since=10"), "实际：{:?}", changes_requests);
}

#[tokio::test]
async fn duplicate_batch_idempotent() {
    let harness = Harness::new("dup-batch");
    harness.pair_ok().await;
    harness.server.set_snapshot(snapshot_body(
        1,
        vec![list_json("l1", "任务")],
        Vec::new(),
    ));
    harness.sync_ok().await;

    let page = changes_body(
        vec![change_json(10, "task", "t1", "upsert", Some(task_json("t1", "l1", "任务", None)))],
        true,
    );
    harness.server.push_changes(ChangesStep::Json(page.clone()));
    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(10, "task", "t1", "upsert", Some(task_json("t1", "l1", "任务", None)))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 1);
    assert_eq!(mirror_meta(&path).last_rev, 10);
}

#[tokio::test]
async fn cursor_advances_only_after_commit() {
    let harness = Harness::new("cursor-tx");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;
    {
        let conn = rusqlite::Connection::open(harness.mirror_path()).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER fail_tasks BEFORE INSERT ON tasks
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();
    }
    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(20, "task", "t9", "upsert", Some(task_json("t9", "l1", "失败任务", None)))],
        false,
    )));
    assert!(harness.service.sync_now().await.is_err());

    let path = harness.mirror_path();
    assert_eq!(mirror_meta(&path).last_rev, 1);
    assert_eq!(mirror_count(&path, "tasks"), 2);
}

#[tokio::test]
async fn reset_clears_mirror() {
    let harness = Harness::new("reset-clear");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(50, "*", "*", "reset", None)],
        false,
    )));
    harness.server.set_snapshot(snapshot_body(
        60,
        vec![list_json("l9", "恢复后的清单")],
        vec![task_json("t9", "l9", "恢复后的任务", None)],
    ));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(
        mirror_scalar_i64(&path, "SELECT COUNT(*) FROM tasks WHERE id='t1'"),
        Some(0)
    );
    assert_eq!(
        mirror_scalar_text(&path, "SELECT title FROM tasks WHERE id='t9'").as_deref(),
        Some("恢复后的任务")
    );
}

#[tokio::test]
async fn reset_then_snapshot() {
    let harness = Harness::new("reset-snap");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(50, "*", "*", "reset", None)],
        false,
    )));
    harness.server.set_snapshot(snapshot_body(
        60,
        vec![list_json("l9", "恢复后的清单")],
        vec![task_json("t9", "l9", "恢复后的任务", None)],
    ));
    harness.sync_ok().await;

    let meta = mirror_meta(&harness.mirror_path());
    assert!(meta.initialized);
    assert!(!meta.needs_snapshot);
    assert_eq!(meta.last_rev, 60);
    assert_eq!(mirror_count(&harness.mirror_path(), "tasks"), 1);
}

#[tokio::test]
async fn reset_crash_recovery() {
    let harness = Harness::new("reset-crash");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    // 模拟备份恢复：只完成「清空 + 标记」就崩溃
    {
        let mirror = MirrorDb::open(&harness.mirror_path()).unwrap();
        mirror
            .apply_changes(&[SyncChange {
                rev: 50,
                entity: "*".into(),
                entity_id: "*".into(),
                op: "reset".into(),
                payload: None,
                changed_at: 50,
            }])
            .unwrap();
    }
    assert!(mirror_meta(&harness.mirror_path()).needs_snapshot);

    // 重新创建服务，模拟应用重启
    let restarted =
        MobileSyncService::new(harness.work.clone(), harness.secrets.clone());
    let state = restarted.state();
    assert!(state.needs_snapshot);
    assert_eq!(state.counts.tasks, 0);

    harness
        .server
        .set_snapshot(snapshot_body(70, vec![list_json("l7", "新清单")], Vec::new()));
    restarted.sync_now().await.expect("重启后应能恢复快照");

    let path = harness.mirror_path();
    let meta = mirror_meta(&path);
    assert!(!meta.needs_snapshot);
    assert!(meta.initialized);
    assert_eq!(meta.last_rev, 70);
    assert_eq!(mirror_count(&path, "lists"), 1);
}

#[tokio::test]
async fn changes_after_snapshot_catch_up() {
    let harness = Harness::new("catch-up");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(5));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(6, "task", "t6", "upsert", Some(task_json("t6", "l1", "快照之后", None)))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 3);
    assert_eq!(mirror_meta(&path).last_rev, 6);
    assert_eq!(
        mirror_scalar_text(&path, "SELECT title FROM tasks WHERE id='t6'").as_deref(),
        Some("快照之后")
    );
}

// ---------------- 网络与离线 ----------------

#[tokio::test]
async fn offline_keeps_mirror_readable() {
    let harness = Harness::new("offline");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(2));
    harness.sync_ok().await;

    harness.server.set_offline(true);
    let error = harness.service.sync_now().await.expect_err("离线应失败");
    assert_eq!(error.kind, SyncErrorKind::Offline);

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 2);
    assert_eq!(mirror_meta(&path).last_rev, 2);
    assert!(mirror_meta(&path).initialized);
    let state = harness.service.state();
    assert!(state.paired);
    assert_eq!(state.counts.tasks, 2);
}

#[tokio::test]
async fn network_interrupt_resume() {
    let harness = Harness::new("interrupt");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.push_changes(ChangesStep::Close);
    assert!(harness.service.sync_now().await.is_err());
    assert_eq!(mirror_meta(&harness.mirror_path()).last_rev, 1);

    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "task", "t2", "upsert", Some(task_json("t2", "l1", "续传任务", None)))],
        false,
    )));
    harness.sync_ok().await;

    let path = harness.mirror_path();
    assert_eq!(mirror_meta(&path).last_rev, 2);
    assert_eq!(
        mirror_scalar_text(&path, "SELECT title FROM tasks WHERE id='t2'").as_deref(),
        Some("续传任务")
    );
    let last_changes = harness
        .server
        .requests()
        .into_iter()
        .filter(|r| r.contains("/sync/changes"))
        .last()
        .unwrap();
    assert!(last_changes.contains("since=1"), "实际：{last_changes}");
}

#[tokio::test]
async fn server_restart_resume() {
    let harness = Harness::new("restart");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.set_offline(true);
    assert!(harness.service.sync_now().await.is_err());
    assert_eq!(mirror_meta(&harness.mirror_path()).last_rev, 1);

    harness.server.set_offline(false);
    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "task", "t2", "upsert", Some(task_json("t2", "l1", "重启后任务", None)))],
        false,
    )));
    harness.sync_ok().await;
    assert_eq!(mirror_meta(&harness.mirror_path()).last_rev, 2);
}

#[tokio::test]
async fn invalid_token_no_data_loss() {
    let harness = Harness::new("token-loss");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(3));
    harness.sync_ok().await;

    harness.server.set_reject_token(true);
    let error = harness.service.sync_now().await.expect_err("令牌失效应失败");
    assert_eq!(error.kind, SyncErrorKind::NeedsPairing);

    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 2);
    assert_eq!(mirror_count(&path, "lists"), 1);
    assert_eq!(mirror_meta(&path).last_rev, 3);
    assert!(mirror_meta(&path).initialized);
}

#[tokio::test]
async fn concurrent_sync_rejected() {
    let harness = Harness::new("concurrent");
    harness.pair_ok().await;
    harness.server.set_snapshot(sample_snapshot(1));
    harness.sync_ok().await;

    harness.server.set_delay(300);
    harness.server.push_changes(ChangesStep::Json(changes_body(
        vec![change_json(2, "task", "t2", "upsert", Some(task_json("t2", "l1", "并发任务", None)))],
        false,
    )));

    let service = harness.service.clone();
    let first = tokio::spawn(async move { service.sync_now().await });
    tokio::time::sleep(Duration::from_millis(80)).await;

    let error = harness
        .service
        .sync_now()
        .await
        .expect_err("并发触发应被拒绝");
    assert_eq!(error.kind, SyncErrorKind::Busy);

    first.await.expect("任务不应崩溃").expect("首次同步应成功");
    let path = harness.mirror_path();
    assert_eq!(mirror_count(&path, "tasks"), 2);
    assert_eq!(
        harness
            .server
            .requests()
            .iter()
            .filter(|r| r.contains("/sync/changes"))
            .count(),
        1
    );
}
