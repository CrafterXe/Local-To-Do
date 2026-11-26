//! 移动端同步命令。仅在 Android 构建中注册，界面上不提供任何任务写入能力。

use std::sync::Arc;

use tauri::State;

use ltd_core::mirror_read::MirrorTaskQuery;
use ltd_core::models::{CalendarDay, Task, TaskList};

use crate::state::AppState;

use super::engine::MobileSyncState;
use super::pairing::PairingPreview;
use super::settings::MobileSettings;

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
    service
        .pair(&payload, &device_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mobile_sync_now(
    state: State<'_, Arc<AppState>>,
) -> Result<MobileSyncState, String> {
    let service = state.mobile_sync.clone();
    service.sync_now().await.map_err(|e| e.to_string())
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
    state
        .mobile_sync
        .update_local_settings(settings)
        .map_err(|e| e.to_string())
}
