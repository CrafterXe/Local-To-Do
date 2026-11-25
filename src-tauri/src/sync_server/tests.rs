//! 局域网同步服务集成测试。
//!
//! 全部使用临时目录中的独立数据库与临时证书，不触碰用户真实数据。
//! 测试客户端直接基于 rustls 搭建，只信任服务端自签名 CA，用于验证
//! Android 端未来采用的「证书指纹锁定」行为。

use std::fs;
use std::io::BufReader;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use ltd_core::db::Db;
use ltd_core::repo;
use ltd_core::repo::tasks::TaskInput;
use ltd_core::sync;

use super::{SyncService, PROTOCOL_VERSION};

// ---------------- 基础设施 ----------------

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ltd-sync-{tag}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir
}

struct Env {
    dir: PathBuf,
    db: Arc<Db>,
    svc: SyncService,
}

fn env(tag: &str) -> Env {
    let dir = temp_dir(tag);
    let db = Arc::new(Db::open(&dir.join("test.db")).expect("临时数据库初始化失败"));
    let svc = SyncService::new(db.clone(), dir.join("sync"));
    Env { dir, db, svc }
}

impl Env {
    fn start(&self) -> SocketAddr {
        self.svc
            .start_with(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
            .expect("同步服务启动失败");
        let status = self.svc.status();
        let host: IpAddr = status.host.expect("缺少监听地址").parse().unwrap();
        SocketAddr::new(host, status.port.expect("缺少监听端口"))
    }

    fn client(&self, addr: SocketAddr) -> TestClient {
        TestClient::with_ca_path(addr, &self.svc.ca_cert_path())
    }

    fn add_task(&self, title: &str) -> String {
        let conn = self.db.lock();
        let list_id = repo::all_lists(&conn).unwrap()[0].id.clone();
        let input = TaskInput {
            title: title.into(),
            list_id: Some(list_id),
            note: None,
            is_important: None,
            due_date: None,
            due_time: None,
            reminder_offset: None,
            reminder_enabled: None,
            my_day: None,
            recurrence: None,
        };
        repo::create_task(&conn, &input).unwrap().id
    }

    fn current_rev(&self) -> i64 {
        let conn = self.db.lock();
        sync::current_rev(&conn).unwrap()
    }
}

fn decoy_ca_pem() -> Vec<u8> {
    let key = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::default();
    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Decoy CA");
    params.self_signed(&key).unwrap().pem().into_bytes()
}

#[derive(Clone)]
struct TestClient {
    addr: SocketAddr,
    config: Arc<rustls::ClientConfig>,
    server_name: rustls::pki_types::ServerName<'static>,
}

impl TestClient {
    fn with_ca_path(addr: SocketAddr, ca: &Path) -> Self {
        let pem = fs::read(ca).expect("读取测试 CA 失败");
        Self::with_ca_pem(addr, &pem)
    }

    fn with_ca_pem(addr: SocketAddr, ca_pem: &[u8]) -> Self {
        let mut roots = rustls::RootCertStore::empty();
        let mut reader = BufReader::new(ca_pem);
        for cert in rustls_pemfile::certs(&mut reader) {
            roots.add(cert.expect("解析 CA 证书失败")).unwrap();
        }
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        Self {
            addr,
            config: Arc::new(config),
            server_name: rustls::pki_types::ServerName::try_from("localhost")
                .unwrap()
                .to_owned(),
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&Value>,
    ) -> Result<HttpResponse, String> {
        let tcp = TcpStream::connect(self.addr)
            .await
            .map_err(|e| format!("TCP 连接失败: {e}"))?;
        let connector = tokio_rustls::TlsConnector::from(self.config.clone());
        let mut tls = connector
            .connect(self.server_name.clone(), tcp)
            .await
            .map_err(|e| format!("TLS 握手失败: {e}"))?;

        let body_bytes = body.map(|v| v.to_string().into_bytes()).unwrap_or_default();
        let mut request = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n");
        if let Some(token) = token {
            request.push_str(&format!("Authorization: Bearer {token}\r\n"));
        }
        if body.is_some() {
            request.push_str("Content-Type: application/json\r\n");
        }
        request.push_str(&format!("Content-Length: {}\r\n\r\n", body_bytes.len()));

        tls.write_all(request.as_bytes())
            .await
            .map_err(|e| format!("写入请求失败: {e}"))?;
        tls.write_all(&body_bytes)
            .await
            .map_err(|e| format!("写入请求体失败: {e}"))?;
        tls.flush().await.map_err(|e| e.to_string())?;

        let mut raw = Vec::new();
        tls.read_to_end(&mut raw)
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?;
        parse_response(&raw)
    }
}

struct HttpResponse {
    status: u16,
    body: Value,
}

fn parse_response(raw: &[u8]) -> Result<HttpResponse, String> {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "HTTP 响应不完整".to_string())?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| "响应头不是 UTF-8".to_string())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| "缺少状态码".to_string())?;

    let mut body = raw[split + 4..].to_vec();
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        body = decode_chunked(&body)?;
    }
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).map_err(|e| format!("响应 JSON 解析失败: {e}"))?
    };
    Ok(HttpResponse { status, body })
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let pos = data
            .windows(2)
            .position(|w| w == b"\r\n")
            .ok_or_else(|| "chunk 头不完整".to_string())?;
        let size_text = std::str::from_utf8(&data[..pos]).map_err(|_| "chunk 头非法".to_string())?;
        let size = usize::from_str_radix(size_text.trim().split(';').next().unwrap_or(""), 16)
            .map_err(|_| "chunk 长度非法".to_string())?;
        data = &data[pos + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size + 2 {
            return Err("chunk 数据不完整".into());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
    Ok(out)
}

async fn pair(env: &Env, client: &TestClient, name: &str) -> (String, String) {
    let info = env.svc.create_pairing().expect("生成配对码失败");
    let resp = client
        .request(
            "POST",
            "/api/v1/pair",
            None,
            Some(&json!({ "code": info.code, "deviceName": name })),
        )
        .await
        .expect("配对请求失败");
    assert_eq!(resp.status, 200, "配对应成功: {:?}", resp.body);
    let token = resp.body["token"].as_str().expect("缺少 token").to_string();
    let device_id = resp.body["deviceId"]
        .as_str()
        .expect("缺少 deviceId")
        .to_string();
    (token, device_id)
}

// ---------------- 生命周期与 TLS ----------------

#[tokio::test]
async fn start_stop_and_release_port() {
    let env = env("start-stop");
    let addr = env.start();
    assert!(env.svc.status().running);

    let client = env.client(addr);
    let resp = client
        .request("GET", "/api/v1/status", None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["protocolVersion"], PROTOCOL_VERSION);

    env.svc.stop().expect("停止失败");
    assert!(!env.svc.status().running);

    // 端口应已释放，可被重新绑定
    std::net::TcpListener::bind(addr).expect("停止后端口未释放");
}

#[tokio::test]
async fn https_uses_pinned_certificate_fingerprint() {
    let env = env("tls-ok");
    let addr = env.start();
    let client = env.client(addr);
    let resp = client
        .request("GET", "/api/v1/status", None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);

    let fingerprint = env.svc.status().fingerprint.expect("缺少证书指纹");
    assert_eq!(fingerprint.split(':').count(), 32, "指纹应为 SHA-256");
    assert!(fingerprint.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
}

#[tokio::test]
async fn wrong_certificate_authority_is_rejected() {
    let env = env("tls-bad");
    let addr = env.start();
    // 只信任另一套 CA：必须无法完成握手
    let client = TestClient::with_ca_pem(addr, &decoy_ca_pem());
    let result = client.request("GET", "/api/v1/status", None, None).await;
    assert!(result.is_err(), "错误证书必须被拒绝，实际: {:?}", result.map(|r| r.status));
}

#[tokio::test]
async fn plaintext_http_is_rejected() {
    let env = env("tls-plain");
    let addr = env.start();
    let mut tcp = TcpStream::connect(addr).await.unwrap();
    tcp.write_all(b"GET /api/v1/status HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut buffer = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(2), tcp.read_to_end(&mut buffer)).await;
    let got_http = buffer.starts_with(b"HTTP/");
    assert!(read.is_err() || !got_http, "明文 HTTP 不应获得响应");
}

// ---------------- 配对 ----------------

#[tokio::test]
async fn pairing_success_and_token_hashed_on_disk() {
    let env = env("pair-ok");
    let addr = env.start();
    let client = env.client(addr);
    let (token, device_id) = pair(&env, &client, "我的手机").await;

    assert_eq!(token.len(), 64);
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(env.svc.list_devices().unwrap().len(), 1);

    let raw = fs::read_to_string(env.dir.join("sync").join("devices.json")).unwrap();
    assert!(!raw.contains(&token), "令牌不能以明文落盘");
    assert!(raw.contains(&super::store::hash_token(&token)), "应保存令牌哈希");

    let resp = client
        .request("GET", "/api/v1/sync/status", Some(&token), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["deviceId"], device_id);
}

#[tokio::test]
async fn pairing_wrong_code_rejected() {
    let env = env("pair-wrong");
    let addr = env.start();
    let client = env.client(addr);
    let info = env.svc.create_pairing().unwrap();
    let wrong = if info.code == "00000000" { "00000001" } else { "00000000" };
    let resp = client
        .request(
            "POST",
            "/api/v1/pair",
            None,
            Some(&json!({ "code": wrong, "deviceName": "错误设备" })),
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 401);
    assert!(env.svc.list_devices().unwrap().is_empty());
}

#[tokio::test]
async fn pairing_expired_code_rejected() {
    let env = env("pair-expired");
    let addr = env.start();
    let client = env.client(addr);
    let info = env.svc.create_pairing_with_ttl(1).unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    let resp = client
        .request(
            "POST",
            "/api/v1/pair",
            None,
            Some(&json!({ "code": info.code, "deviceName": "慢手机" })),
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 401);
}

#[tokio::test]
async fn pairing_code_is_single_use() {
    let env = env("pair-once");
    let addr = env.start();
    let client = env.client(addr);
    let info = env.svc.create_pairing().unwrap();
    let body = json!({ "code": info.code, "deviceName": "手机" });

    let first = client
        .request("POST", "/api/v1/pair", None, Some(&body))
        .await
        .unwrap();
    assert_eq!(first.status, 200);

    let second = client
        .request("POST", "/api/v1/pair", None, Some(&body))
        .await
        .unwrap();
    assert_eq!(second.status, 401);
}

#[tokio::test]
async fn pairing_is_rate_limited() {
    let env = env("pair-limit");
    let addr = env.start();
    let client = env.client(addr);
    env.svc.create_pairing().unwrap();

    for _ in 0..5 {
        let resp = client
            .request(
                "POST",
                "/api/v1/pair",
                None,
                Some(&json!({ "code": "00000000", "deviceName": "攻击者" })),
            )
            .await
            .unwrap();
        assert_eq!(resp.status, 401);
    }
    let blocked = client
        .request(
            "POST",
            "/api/v1/pair",
            None,
            Some(&json!({ "code": "00000000", "deviceName": "攻击者" })),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status, 429);
}

#[tokio::test]
async fn pairing_info_contains_qr_payload() {
    let env = env("pair-qr");
    env.start();
    let info = env.svc.create_pairing().unwrap();
    assert_eq!(info.code.len(), 8);
    assert!(info.code.chars().all(|c| c.is_ascii_digit()));
    assert!(info.expires_at > 0);
    assert!(info.qr_svg.contains("<svg"));
    let payload: Value = serde_json::from_str(&info.payload).unwrap();
    assert_eq!(payload["protocol"], PROTOCOL_VERSION);
    assert_eq!(payload["code"], info.code);
    assert_eq!(payload["port"], info.port);
    assert_eq!(payload["fingerprint"], info.fingerprint);
}

#[tokio::test]
async fn stopping_server_invalidates_pairing_code() {
    let env = env("pair-stop");
    env.start();
    let info = env.svc.create_pairing().unwrap();
    env.svc.stop().unwrap();
    let addr2 = env.start();
    let client2 = env.client(addr2);
    let resp = client2
        .request(
            "POST",
            "/api/v1/pair",
            None,
            Some(&json!({ "code": info.code, "deviceName": "手机" })),
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 401);
}

#[tokio::test]
async fn unauthorized_and_revoked_tokens_rejected() {
    let env = env("auth");
    let addr = env.start();
    let client = env.client(addr);

    for path in ["/api/v1/sync/status", "/api/v1/sync/snapshot", "/api/v1/sync/changes"] {
        let none = client.request("GET", path, None, None).await.unwrap();
        assert_eq!(none.status, 401, "缺少令牌应被拒绝: {path}");
        let bad = client.request("GET", path, Some("deadbeef"), None).await.unwrap();
        assert_eq!(bad.status, 401, "无效令牌应被拒绝: {path}");
    }

    let (token, device_id) = pair(&env, &client, "手机").await;
    let ok = client
        .request("GET", "/api/v1/sync/status", Some(&token), None)
        .await
        .unwrap();
    assert_eq!(ok.status, 200);

    let devices = env.svc.revoke_device(&device_id).unwrap();
    assert!(devices.is_empty());

    let revoked = client
        .request("GET", "/api/v1/sync/status", Some(&token), None)
        .await
        .unwrap();
    assert_eq!(revoked.status, 401, "撤销后令牌必须立即失效");
}

// ---------------- 快照与增量 ----------------

#[tokio::test]
async fn snapshot_contains_live_data_and_rev() {
    let env = env("snapshot");
    env.add_task("任务一");
    env.add_task("任务二");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;

    let resp = client
        .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    let snapshot = &resp.body["snapshot"];
    assert_eq!(snapshot["rev"], env.current_rev());
    assert_eq!(snapshot["tasks"].as_array().unwrap().len(), 2);
    assert!(!snapshot["lists"].as_array().unwrap().is_empty());

    let titles: Vec<&str> = snapshot["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"任务一") && titles.contains(&"任务二"));
}

#[tokio::test]
async fn incremental_changes_are_ordered_and_complete() {
    let env = env("changes");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;

    let before = env.current_rev();
    env.add_task("增量一");
    env.add_task("增量二");
    env.add_task("增量三");
    let after = env.current_rev();

    let resp = client
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={before}&limit=100"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["latestRev"], after);

    let changes = resp.body["changes"].as_array().unwrap();
    assert_eq!(changes.len() as i64, after - before);
    let mut last = 0;
    for change in changes {
        let rev = change["rev"].as_i64().unwrap();
        assert!(rev > last, "rev 必须严格升序");
        last = rev;
        assert_eq!(change["entity"], "task");
        assert_eq!(change["op"], "upsert");
        assert!(change["payload"]["title"].is_string());
    }
}

#[tokio::test]
async fn pagination_has_no_gap_or_duplicate() {
    let env = env("paging");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;

    let start_rev = env.current_rev();
    for i in 0..7 {
        env.add_task(&format!("分页任务 {i}"));
    }
    let end_rev = env.current_rev();

    let mut seen = Vec::new();
    let mut cursor = start_rev;
    loop {
        let resp = client
            .request(
                "GET",
                &format!("/api/v1/sync/changes?since={cursor}&limit=3"),
                Some(&token),
                None,
            )
            .await
            .unwrap();
        assert_eq!(resp.status, 200);
        let changes = resp.body["changes"].as_array().unwrap();
        for change in changes {
            seen.push(change["rev"].as_i64().unwrap());
        }
        let has_more = resp.body["hasMore"].as_bool().unwrap();
        if !has_more {
            break;
        }
        cursor = *seen.last().expect("has_more 为真但本页为空");
    }

    let expected: Vec<i64> = (start_rev + 1..=end_rev).collect();
    assert_eq!(seen, expected, "分页不得遗漏或重复");
}

#[tokio::test]
async fn repeated_page_requests_are_idempotent() {
    let env = env("idempotent");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;
    let since = env.current_rev();
    env.add_task("幂等任务");

    let path = format!("/api/v1/sync/changes?since={since}&limit=50");
    let first = client
        .request("GET", &path, Some(&token), None)
        .await
        .unwrap();
    let second = client
        .request("GET", &path, Some(&token), None)
        .await
        .unwrap();
    assert_eq!(first.status, 200);
    assert_eq!(second.status, 200);
    assert_eq!(first.body["changes"], second.body["changes"]);
    assert_eq!(first.body["latestRev"], second.body["latestRev"]);
}

#[tokio::test]
async fn has_more_matches_remaining_changes() {
    let env = env("has-more");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;
    let since = env.current_rev();
    for i in 0..4 {
        env.add_task(&format!("剩余 {i}"));
    }

    let first = client
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={since}&limit=2"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    assert_eq!(first.body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(first.body["hasMore"], true);

    let cursor = first.body["changes"][1]["rev"].as_i64().unwrap();
    let second = client
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={cursor}&limit=2"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    assert_eq!(second.body["changes"].as_array().unwrap().len(), 2);
    assert_eq!(second.body["hasMore"], false);
}

#[tokio::test]
async fn oversized_limit_is_clamped() {
    let env = env("limit");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;
    let since = env.current_rev();
    env.add_task("限制任务");

    let resp = client
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={since}&limit=100000"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["changes"].as_array().unwrap().len(), 1);
    assert_eq!(resp.body["hasMore"], false);
}

#[tokio::test]
async fn invalid_query_parameters_are_rejected() {
    let env = env("params");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;

    for path in [
        "/api/v1/sync/changes?since=abc",
        "/api/v1/sync/changes?since=-1",
        "/api/v1/sync/changes?limit=0",
        "/api/v1/sync/changes?limit=-5",
    ] {
        let resp = client.request("GET", path, Some(&token), None).await.unwrap();
        assert_eq!(resp.status, 400, "非法参数应被拒绝: {path}");
    }
}

#[tokio::test]
async fn reset_marker_requires_new_snapshot() {
    let env = env("reset");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;

    let resp = client
        .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
        .await
        .unwrap();
    let old_rev = resp.body["snapshot"]["rev"].as_i64().unwrap();

    // 模拟「备份恢复」后的整体替换
    {
        let conn = env.db.lock();
        sync::record_reset(&conn).unwrap();
    }
    env.add_task("恢复后的任务");

    let changes = client
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={old_rev}&limit=100"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    assert_eq!(changes.status, 200);
    let ops: Vec<&str> = changes.body["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["op"].as_str().unwrap())
        .collect();
    assert!(ops.contains(&"reset"), "必须返回 reset 标记");

    let fresh = client
        .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
        .await
        .unwrap();
    let new_rev = fresh.body["snapshot"]["rev"].as_i64().unwrap();
    assert!(new_rev > old_rev);
}

#[tokio::test]
async fn snapshot_then_writes_have_no_gap() {
    let env = env("no-gap");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;

    let first = client
        .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
        .await
        .unwrap();
    let snapshot_rev = first.body["snapshot"]["rev"].as_i64().unwrap();

    for i in 0..20 {
        env.add_task(&format!("并发写入 {i}"));
    }

    let second = client
        .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
        .await
        .unwrap();
    let later_rev = second.body["snapshot"]["rev"].as_i64().unwrap();

    let changes = client
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={snapshot_rev}&limit=1000"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    let changes = changes.body["changes"].as_array().unwrap();
    assert_eq!(
        changes.len() as i64,
        later_rev - snapshot_rev,
        "快照之后的新写入不能遗漏"
    );
    assert_eq!(changes.last().unwrap()["rev"], later_rev);
}

#[tokio::test]
async fn restart_resumes_with_same_certificate() {
    let env = env("restart");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;
    let fingerprint = env.svc.status().fingerprint.clone().unwrap();

    let snapshot = client
        .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
        .await
        .unwrap();
    let cursor = snapshot.body["snapshot"]["rev"].as_i64().unwrap();

    env.svc.stop().unwrap();
    let addr2 = env.start();
    let status = env.svc.status();
    assert_eq!(status.fingerprint.as_deref(), Some(fingerprint.as_str()), "重启不应更换证书");

    let client2 = env.client(addr2);
    env.add_task("重启后任务");
    let resp = client2
        .request(
            "GET",
            &format!("/api/v1/sync/changes?since={cursor}&limit=50"),
            Some(&token),
            None,
        )
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body["changes"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn multiple_devices_can_sync_concurrently() {
    let env = env("multi");
    let addr = env.start();
    let client = env.client(addr);
    let (token_a, _) = pair(&env, &client, "手机 A").await;
    let (token_b, _) = pair(&env, &client, "手机 B").await;
    env.add_task("并发任务");

    let mut handles = Vec::new();
    for i in 0..6 {
        let client = client.clone();
        let token = if i % 2 == 0 { token_a.clone() } else { token_b.clone() };
        handles.push(tokio::spawn(async move {
            client
                .request("GET", "/api/v1/sync/snapshot", Some(&token), None)
                .await
        }));
    }

    let mut revs = Vec::new();
    for handle in handles {
        let resp = handle.await.unwrap().expect("并发请求失败");
        assert_eq!(resp.status, 200);
        revs.push(resp.body["snapshot"]["rev"].as_i64().unwrap());
    }
    assert!(revs.windows(2).all(|w| w[0] == w[1]), "并发快照应一致");
}

#[tokio::test]
async fn reset_pairing_rotates_certificate_and_clears_devices() {
    let env = env("reset-pair");
    let addr = env.start();
    let client = env.client(addr);
    let (token, _) = pair(&env, &client, "手机").await;
    let old_fingerprint = env.svc.status().fingerprint.unwrap();

    let status = env.svc.reset_pairing().unwrap();
    assert!(status.running, "重置后服务应继续运行");
    assert_eq!(status.device_count, 0);
    assert_ne!(
        status.fingerprint.as_deref(),
        Some(old_fingerprint.as_str()),
        "重置配对必须更换证书"
    );
    assert!(env.svc.list_devices().unwrap().is_empty());

    let addr2 = SocketAddr::new(status.host.unwrap().parse().unwrap(), status.port.unwrap());
    let client2 = env.client(addr2);
    let resp = client2
        .request("GET", "/api/v1/sync/status", Some(&token), None)
        .await
        .unwrap();
    assert_eq!(resp.status, 401, "旧令牌在重置后必须失效");
}

#[tokio::test]
async fn port_conflict_reports_clear_error_and_recovers() {
    let env = env("port");
    let blocker = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = blocker.local_addr().unwrap().port();

    let result = env
        .svc
        .start_with(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let message = result.expect_err("端口冲突必须报错").to_string();
    assert!(message.contains("端口"), "错误信息应明确指出端口冲突: {message}");
    assert!(!env.svc.status().running);

    drop(blocker);
    // 失败启动不应影响后续使用
    let addr = env.start();
    let client = env.client(addr);
    let resp = client
        .request("GET", "/api/v1/status", None, None)
        .await
        .unwrap();
    assert_eq!(resp.status, 200);
}
