//! 移动端同步命令。仅在 Android 构建中注册，界面上不提供任何任务写入能力。

use std::sync::Arc;

use tauri::State;

use crate::state::AppState;

use super::engine::MobileSyncState;
use super::pairing::PairingPreview;

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
