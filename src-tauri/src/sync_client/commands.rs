//! 移动端同步命令。仅在 Android 构建中注册，界面上不提供任何任务写入能力。

use std::sync::Arc;

use tauri::State;

use ltd_core::mirror_read::MirrorTaskQuery;
use ltd_core::models::{CalendarDay, Task, TaskList};

use crate::state::AppState;

use super::engine::MobileSyncState;
use super::pairing::PairingPreview;
use super::reminders;
use super::settings::MobileSettings;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderStatus {
    pub enabled: bool,
    pub lead_minutes: i64,
    pub scheduled: usize,
    pub exact_alarm_allowed: bool,
    pub notifications_allowed: bool,
    /// 是否已加入电池优化白名单
    pub ignoring_battery_optimizations: bool,
    pub skipped_past: usize,
    pub skipped_far: usize,
    pub error: Option<String>,
}

/// 依据镜像数据与本地设置重建系统提醒（失败只记录在状态里，不影响同步）
fn resync_reminders(state: &AppState) -> ReminderStatus {
    let settings = state.mobile_sync.local_settings();
    let native = reminders::status().unwrap_or_default();
    let mut status = ReminderStatus {
        enabled: settings.notifications_enabled,
        lead_minutes: settings.reminder_lead_minutes,
        scheduled: 0,
        exact_alarm_allowed: native.exact_alarm_allowed,
        notifications_allowed: native.notifications_allowed,
        ignoring_battery_optimizations: native.ignoring_battery_optimizations,
        skipped_past: 0,
        skipped_far: 0,
        error: None,
    };
    if !settings.notifications_enabled {
        if let Err(e) = reminders::cancel_all() {
            status.error = Some(e);
        }
        return status;
    }
    let rows = match state.mobile_sync.reminder_candidates() {
        Ok(rows) => rows,
        Err(e) => {
            status.error = Some(e.to_string());
            return status;
        }
    };
    let now = chrono::Utc::now().timestamp_millis();
    let (plan, stats) = reminders::build_plan(&rows, settings.reminder_lead_minutes, now);
    status.skipped_past = stats.skipped_past;
    status.skipped_far = stats.skipped_far;
    if let Err(e) = reminders::schedule(&plan) {
        status.error = Some(e);
    }
    if let Ok(native) = reminders::status() {
        status.scheduled = native.scheduled;
        status.exact_alarm_allowed = native.exact_alarm_allowed;
        status.notifications_allowed = native.notifications_allowed;
        status.ignoring_battery_optimizations = native.ignoring_battery_optimizations;
    }
    status
}


#[tauri::command]
pub fn mobile_sync_state(state: State<'_, Arc<AppState>>) -> MobileSyncState {
    state.mobile_sync.state()
}

#[tauri::command]
pub fn mobile_inspect_pairing(
    state: State<'_, Arc<AppState>>,
    payload: String,
) -> Result<PairingPreview, String> {
    state
        .mobile_sync
        .inspect_pairing(&payload)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mobile_pair(
    state: State<'_, Arc<AppState>>,
    payload: String,
    device_name: String,
) -> Result<MobileSyncState, String> {
    let service = state.mobile_sync.clone();
    let result = service
        .pair(&payload, &device_name)
        .await
        .map_err(|e| e.to_string())?;
    let _ = resync_reminders(&state);
    Ok(result)
}

#[tauri::command]
pub async fn mobile_sync_now(
    state: State<'_, Arc<AppState>>,
) -> Result<MobileSyncState, String> {
    let service = state.mobile_sync.clone();
    let result = service.sync_now().await.map_err(|e| e.to_string())?;
    // 同步后按最新镜像重建提醒：新增、修改、完成、删除的任务都会在此处反映
    let _ = resync_reminders(&state);
    Ok(result)
}

#[tauri::command]
pub fn mobile_unpair(state: State<'_, Arc<AppState>>) -> Result<MobileSyncState, String> {
    state.mobile_sync.unpair().map_err(|e| e.to_string())
}

// ---- 只读数据命令：只查询镜像库，不提供任何业务写入能力 ----

#[tauri::command]
pub fn mobile_lists(state: State<'_, Arc<AppState>>) -> Result<Vec<TaskList>, String> {
    state.mobile_sync.lists().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_tasks(
    state: State<'_, Arc<AppState>>,
    query: MirrorTaskQuery,
) -> Result<Vec<Task>, String> {
    state.mobile_sync.tasks(&query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_task(state: State<'_, Arc<AppState>>, id: String) -> Result<Task, String> {
    state.mobile_sync.task(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_tasks_for_day(
    state: State<'_, Arc<AppState>>,
    date: String,
) -> Result<Vec<Task>, String> {
    state
        .mobile_sync
        .tasks_for_day(&date)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_calendar(
    state: State<'_, Arc<AppState>>,
    start: String,
    end: String,
) -> Result<Vec<CalendarDay>, String> {
    state
        .mobile_sync
        .calendar(&start, &end)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_settings_get(state: State<'_, Arc<AppState>>) -> MobileSettings {
    state.mobile_sync.local_settings()
}

#[tauri::command]
pub fn mobile_settings_update(
    state: State<'_, Arc<AppState>>,
    settings: MobileSettings,
) -> Result<MobileSettings, String> {
    let saved = state
        .mobile_sync
        .update_local_settings(settings)
        .map_err(|e| e.to_string())?;
    // 开关或提前量变化后立即重建调度
    let _ = resync_reminders(&state);
    Ok(saved)
}

// ---- 后台提醒：只读取镜像并注册系统闹钟，不写任何任务数据 ----

#[tauri::command]
pub fn mobile_reminders_resync(state: State<'_, Arc<AppState>>) -> ReminderStatus {
    resync_reminders(&state)
}

#[tauri::command]
pub fn mobile_reminder_status(
    state: State<'_, Arc<AppState>>,
) -> Result<ReminderStatus, String> {
    let settings = state.mobile_sync.local_settings();
    let native = reminders::status()?;
    Ok(ReminderStatus {
        enabled: settings.notifications_enabled,
        lead_minutes: settings.reminder_lead_minutes,
        scheduled: native.scheduled,
        exact_alarm_allowed: native.exact_alarm_allowed,
        notifications_allowed: native.notifications_allowed,
        ignoring_battery_optimizations: native.ignoring_battery_optimizations,
        skipped_past: 0,
        skipped_far: 0,
        error: None,
    })
}

#[tauri::command]
pub fn mobile_reminder_open_exact_alarm_settings() -> Result<(), String> {
    reminders::open_exact_alarm_settings()
}

#[tauri::command]
pub fn mobile_reminder_open_battery_settings() -> Result<(), String> {
    reminders::open_battery_settings()
}
