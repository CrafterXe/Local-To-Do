//! 局域网同步 REST API（版本 v1）。
//!
//! 接口：
//! - `GET  /api/v1/status`           服务公开信息（无需令牌）
//! - `POST /api/v1/pair`             使用一次性配对码换取设备令牌（无需令牌）
//! - `GET  /api/v1/sync/status`      当前设备与同步进度（需令牌）
//! - `GET  /api/v1/sync/snapshot`    全量快照（需令牌）
//! - `GET  /api/v1/sync/changes`     按 rev 升序的增量变更（需令牌）
//!
//! 所有接口只读业务数据，不接受任何写操作或 SQL 参数。

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

use ltd_core::db::Db;
use ltd_core::domain::local_now_ms;
use ltd_core::sync;

use super::{
    new_device_token, store, DeviceView, PairingSession, PROTOCOL_VERSION,
};

/// 配对失败限流：60 秒内最多 5 次失败
const MAX_PAIR_FAILURES: u32 = 5;
const PAIR_FAILURE_WINDOW: Duration = Duration::from_secs(60);
/// 单次增量拉取的默认/最大条数
const DEFAULT_CHANGE_LIMIT: i64 = 200;
const MAX_CHANGE_LIMIT: i64 = 1000;

pub(crate) struct ServerContext {
    db: Arc<Db>,
    devices: Arc<store::DeviceStore>,
    pairing: Arc<Mutex<Option<PairingSession>>>,
    pair_failures: Arc<PairFailureWindow>,
    bind_addr: std::net::SocketAddr,
    fingerprint: String,
    semaphore: Arc<Semaphore>,
}

impl ServerContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        db: Arc<Db>,
        devices: Arc<store::DeviceStore>,
        pairing: Arc<Mutex<Option<PairingSession>>>,
        pair_failures: Arc<PairFailureWindow>,
        bind_addr: std::net::SocketAddr,
        fingerprint: String,
        _started_at: i64,
        max_concurrent: usize,
    ) -> Self {
        Self {
            db,
            devices,
            pairing,
            pair_failures,
            bind_addr,
            fingerprint,
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }
}

/// 配对失败窗口（防止暴力尝试一次性配对码）
pub(crate) struct PairFailureWindow {
    inner: Mutex<PairWindow>,
}

struct PairWindow {
    failures: u32,
    since: Instant,
}

impl PairFailureWindow {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(PairWindow {
                failures: 0,
                since: Instant::now(),
            }),
        }
    }

    fn is_limited(&self) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.failures >= MAX_PAIR_FAILURES && guard.since.elapsed() < PAIR_FAILURE_WINDOW
    }

    fn record_failure(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if guard.since.elapsed() >= PAIR_FAILURE_WINDOW {
            guard.failures = 0;
            guard.since = Instant::now();
        }
        guard.failures += 1;
    }

    fn reset(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.failures = 0;
        guard.since = Instant::now();
    }
}

pub(crate) fn router(ctx: Arc<ServerContext>) -> Router {
    let public = Router::new()
        .route("/status", get(status_handler))
        .route("/pair", post(pair_handler));

    let protected = Router::new()
        .route("/sync/status", get(sync_status_handler))
        .route("/sync/snapshot", get(snapshot_handler))
        .route("/sync/changes", get(changes_handler))
        .route_layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            require_token,
        ));

    Router::new()
        .nest("/api/v1", public.merge(protected))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .layer(axum::middleware::from_fn_with_state(
            ctx.clone(),
            concurrency_guard,
        ))
        .with_state(ctx)
}

// ---------------- 中间件 ----------------

async fn concurrency_guard(
    State(ctx): State<Arc<ServerContext>>,
    request: Request,
    next: Next,
) -> Response {
    let permit = match ctx.semaphore.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return ApiError::busy().into_response(),
    };
    let response = next.run(request).await;
    drop(permit);
    response
}

async fn require_token(
    State(ctx): State<Arc<ServerContext>>,
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let Some(token) = token else {
        return ApiError::unauthorized("缺少设备令牌").into_response();
    };

    match ctx.devices.authorize(token, local_now_ms()) {
        Ok(Some(device)) => {
            request.extensions_mut().insert(device);
            next.run(request).await
        }
        Ok(None) => ApiError::unauthorized("设备未授权或令牌已失效").into_response(),
        Err(e) => {
            crate::logging::warn(format!("读取设备列表失败: {e}"));
            ApiError::internal().into_response()
        }
    }
}

// ---------------- 公开接口 ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicStatus {
    protocol_version: &'static str,
    server_name: &'static str,
    time: i64,
    pairing_available: bool,
}

async fn status_handler(State(ctx): State<Arc<ServerContext>>) -> Json<PublicStatus> {
    let now = local_now_ms();
    let pairing_available = {
        let mut guard = ctx.pairing.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(session) if session.is_valid(now) => true,
            Some(_) => {
                *guard = None;
                false
            }
            None => false,
        }
    };
    Json(PublicStatus {
        protocol_version: PROTOCOL_VERSION,
        server_name: "Local To Do",
        time: now,
        pairing_available,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairRequest {
    code: String,
    device_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairResponse {
    device_id: String,
    device_name: String,
    token: String,
    protocol_version: &'static str,
    server_host: String,
    server_port: u16,
    fingerprint: String,
}

async fn pair_handler(
    State(ctx): State<Arc<ServerContext>>,
    Json(body): Json<PairRequest>,
) -> Result<Json<PairResponse>, ApiError> {
    if ctx.pair_failures.is_limited() {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "pairing_rate_limited",
            "配对尝试过于频繁，请稍后再试",
        ));
    }

    let code = body.code.trim();
    if code.len() != super::PAIRING_CODE_LEN || !code.chars().all(|c| c.is_ascii_digit()) {
        ctx.pair_failures.record_failure();
        return Err(ApiError::invalid_pairing());
    }

    let accepted = {
        let mut guard = ctx.pairing.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(session)
                if session.is_valid(local_now_ms())
                    && bool::from(session.code.as_bytes().ct_eq(code.as_bytes())) =>
            {
                // 一次性使用：匹配成功后立即失效
                *guard = None;
                true
            }
            _ => false,
        }
    };

    if !accepted {
        ctx.pair_failures.record_failure();
        return Err(ApiError::invalid_pairing());
    }
    ctx.pair_failures.reset();

    let device_name = sanitize_device_name(body.device_name.as_deref());
    let token = new_device_token();
    let token_hash = store::hash_token(&token);
    let device = ctx
        .devices
        .add(&device_name, &token_hash, local_now_ms())
        .map_err(|e| {
            crate::logging::warn(format!("保存配对设备失败: {e}"));
            ApiError::internal()
        })?;
    crate::logging::info(format!("新设备已配对：{}", device.name));

    Ok(Json(PairResponse {
        device_id: device.id,
        device_name: device.name,
        token,
        protocol_version: PROTOCOL_VERSION,
        server_host: ctx.bind_addr.ip().to_string(),
        server_port: ctx.bind_addr.port(),
        fingerprint: ctx.fingerprint.clone(),
    }))
}

// ---------------- 同步接口（需令牌） ----------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncStatusResponse {
    protocol_version: &'static str,
    device_id: String,
    device_name: String,
    latest_rev: i64,
    server_time: i64,
}

async fn sync_status_handler(
    State(ctx): State<Arc<ServerContext>>,
    Extension(device): Extension<DeviceView>,
) -> Result<Json<SyncStatusResponse>, ApiError> {
    let latest_rev = run_db(ctx.db.clone(), |conn| sync::current_rev(conn)).await?;
    Ok(Json(SyncStatusResponse {
        protocol_version: PROTOCOL_VERSION,
        device_id: device.id,
        device_name: device.name,
        latest_rev,
        server_time: local_now_ms(),
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotResponse {
    protocol_version: &'static str,
    server_time: i64,
    snapshot: sync::SyncSnapshot,
}

async fn snapshot_handler(
    State(ctx): State<Arc<ServerContext>>,
) -> Result<Json<SnapshotResponse>, ApiError> {
    let snapshot = run_db(ctx.db.clone(), |conn| sync::snapshot(conn)).await?;
    Ok(Json(SnapshotResponse {
        protocol_version: PROTOCOL_VERSION,
        server_time: local_now_ms(),
        snapshot,
    }))
}

#[derive(Deserialize)]
struct ChangesQuery {
    since: Option<String>,
    limit: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangesResponse {
    protocol_version: &'static str,
    server_time: i64,
    latest_rev: i64,
    has_more: bool,
    changes: Vec<sync::SyncChange>,
}

async fn changes_handler(
    State(ctx): State<Arc<ServerContext>>,
    Query(query): Query<ChangesQuery>,
) -> Result<Json<ChangesResponse>, ApiError> {
    let since = match query.since.as_deref() {
        None => 0,
        Some(v) => v
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|v| *v >= 0)
            .ok_or_else(|| ApiError::bad_request("since 必须是非负整数"))?,
    };

    let limit = match query.limit.as_deref() {
        None => DEFAULT_CHANGE_LIMIT,
        Some(v) => {
            let parsed = v
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|v| *v >= 1)
                .ok_or_else(|| ApiError::bad_request("limit 必须是正整数"))?;
            parsed.min(MAX_CHANGE_LIMIT)
        }
    };

    let changes = run_db(ctx.db.clone(), move |conn| {
        sync::changes_since(conn, since, Some(limit))
    })
    .await?;

    Ok(Json(ChangesResponse {
        protocol_version: PROTOCOL_VERSION,
        server_time: local_now_ms(),
        latest_rev: changes.latest_rev,
        has_more: changes.has_more,
        changes: changes.changes,
    }))
}

// ---------------- 辅助 ----------------

/// 在阻塞线程池中访问 SQLite，避免占用异步运行时线程。
async fn run_db<T, F>(db: Arc<Db>, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&rusqlite::Connection) -> ltd_core::error::AppResult<T> + Send + 'static,
{
    let result = tokio::task::spawn_blocking(move || {
        let conn = db.lock();
        f(&conn)
    })
    .await
    .map_err(|e| {
        crate::logging::error(format!("同步请求执行失败: {e}"));
        ApiError::internal()
    })?;

    result.map_err(|e| {
        crate::logging::error(format!("同步数据读取失败: {e}"));
        ApiError::internal()
    })
}

fn sanitize_device_name(name: Option<&str>) -> String {
    let name = name.unwrap_or("").trim();
    if name.is_empty() {
        return "未命名设备".into();
    }
    name.chars().take(64).collect()
}

struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn internal() -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", "服务内部错误")
    }

    fn busy() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "server_busy",
            "同步服务繁忙，请稍后重试",
        )
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", message)
    }

    fn invalid_pairing() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "invalid_pairing_code",
            "配对码无效、已过期或已被使用",
        )
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.code,
                "message": self.message,
            })),
        )
            .into_response()
    }
}
