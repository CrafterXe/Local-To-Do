//! Android 本地提醒与旧同步客户端命令。
//!
//! 独立 Android 版只注册本文件底部的本地提醒/备份命令；旧配对函数保留源码，
//! 但不进入 mobile invoke handler。提醒数据始终来自权威 `local-to-do.db`。

use std::sync::Arc;

use rusqlite::Connection;
use tauri::State;

use ltd_core::error::AppResult;
use ltd_core::mirror_read::MirrorTaskQuery;
use ltd_core::models::{CalendarDay, Task, TaskList};

use crate::state::AppState;

use super::engine::MobileSyncState;
use super::pairing::PairingPreview;
use super::reminders::{self, FiredReminder};
use super::settings::MobileSettings;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderStatus {
    pub enabled: bool,
    pub lead_minutes: i64,
    pub scheduled: usize,
    pub exact_alarm_allowed: bool,
    pub notifications_allowed: bool,
    /// 是否已加入电池优化白名单。
    pub ignoring_battery_optimizations: bool,
    pub skipped_past: usize,
    /// 保留给旧前端的兼容字段；不再按日期范围跳过未来提醒。
    pub skipped_far: usize,
    /// 超过 MAX_ALARMS 后本次未注册的数量。
    pub truncated: usize,
    pub error: Option<String>,
}

fn set_error(status: &mut ReminderStatus, error: impl Into<String>) {
    if status.error.is_none() {
        status.error = Some(error.into());
    }
}

fn apply_native_status(status: &mut ReminderStatus, native: reminders::NativeReminderStatus) {
    status.scheduled = native.scheduled;
    status.exact_alarm_allowed = native.exact_alarm_allowed;
    status.notifications_allowed = native.notifications_allowed;
    status.ignoring_battery_optimizations = native.ignoring_battery_optimizations;
}

/// 把原生已展示通知对应的原子计划标为 fired；事务成功后才允许原生层消费记录。
fn mark_native_fired(conn: &Connection, items: &[FiredReminder], fired_at: i64) -> AppResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    for item in items {
        tx.execute(
            "UPDATE reminder_schedules
             SET status='fired', fired_at=?3
             WHERE task_id=?1 AND trigger_at=?2 AND status='pending'",
            rusqlite::params![item.task_id, item.trigger_at, fired_at],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// 自动备份失败只记录日志，不阻断提醒或启动。
pub(crate) fn auto_backup_if_due(state: &AppState) {
    let conn = state.db.lock();
    if let Err(error) = crate::backup::auto_backup_if_due(&conn, &state.backup_dir, false) {
        crate::logging::warn(format!("Android 自动备份失败：{error}"));
    }
}

/// 从权威数据库重建全部系统闹钟，并同步已展示通知和原生状态。
pub(crate) fn resync_from_db(state: &AppState) -> ReminderStatus {
    let mut status = ReminderStatus {
        enabled: false,
        lead_minutes: 0,
        scheduled: 0,
        exact_alarm_allowed: false,
        notifications_allowed: false,
        ignoring_battery_optimizations: false,
        skipped_past: 0,
        skipped_far: 0,
        truncated: 0,
        error: None,
    };
    match reminders::status() {
        Ok(native) => apply_native_status(&mut status, native),
        Err(error) => set_error(&mut status, error),
    }

    let enabled = {
        let conn = state.db.lock();
        match ltd_core::db::load_settings(&conn) {
            Ok(settings) => settings.notifications_enabled,
            Err(error) => {
                set_error(&mut status, error.to_string());
                return status;
            }
        }
    };
    status.enabled = enabled;

    let fired = match reminders::fired() {
        Ok(items) => items,
        Err(error) => {
            set_error(&mut status, error);
            Vec::new()
        }
    };
    if !fired.is_empty() {
        let marked = {
            let conn = state.db.lock();
            mark_native_fired(&conn, &fired, ltd_core::domain::local_now_ms())
        };
        match marked {
            Ok(()) => {
                if let Err(error) = reminders::consume_fired(&fired) {
                    set_error(&mut status, error);
                }
            }
            Err(error) => set_error(&mut status, error.to_string()),
        }
    }

    if !enabled {
        if let Err(error) = reminders::cancel_all() {
            set_error(&mut status, error);
        }
        status.scheduled = 0;
        return status;
    }

    let (rows, active_task_ids) = {
        let conn = state.db.lock();
        let rows = match reminders::reminder_candidates(&conn) {
            Ok(rows) => rows,
            Err(error) => {
                set_error(&mut status, error.to_string());
                return status;
            }
        };
        let active = match reminders::active_reminder_task_ids(&conn) {
            Ok(ids) => ids,
            Err(error) => {
                set_error(&mut status, error.to_string());
                return status;
            }
        };
        (rows, active)
    };
    let (plan, stats) = reminders::build_plan(&rows, ltd_core::domain::local_now_ms());
    status.skipped_past = stats.skipped_past;
    status.skipped_far = stats.skipped_far;
    status.truncated = stats.truncated;
    if stats.truncated > 0 {
        crate::logging::warn(format!(
            "Android 提醒达到单次注册上限，{} 条较远提醒暂未注册",
            stats.truncated
        ));
    }
    if let Err(error) = reminders::schedule(&plan, &active_task_ids) {
        set_error(&mut status, error);
    }
    match reminders::status() {
        Ok(native) => apply_native_status(&mut status, native),
        Err(error) => set_error(&mut status, error),
    }
    status
}

#[tauri::command]
pub fn mobile_sync_state(state: State<'_, Arc<AppState>>) -> MobileSyncState {
    state.mobile_sync().state()
}

#[tauri::command]
pub fn mobile_inspect_pairing(
    state: State<'_, Arc<AppState>>,
    payload: String,
) -> Result<PairingPreview, String> {
    state
        .mobile_sync()
        .inspect_pairing(&payload)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mobile_pair(
    state: State<'_, Arc<AppState>>,
    payload: String,
    device_name: String,
) -> Result<MobileSyncState, String> {
    let service = state.mobile_sync().clone();
    let result = service
        .pair(&payload, &device_name)
        .await
        .map_err(|e| e.to_string())?;
    let _ = resync_from_db(&state);
    Ok(result)
}

#[tauri::command]
pub async fn mobile_sync_now(state: State<'_, Arc<AppState>>) -> Result<MobileSyncState, String> {
    let service = state.mobile_sync().clone();
    let result = service.sync_now().await.map_err(|e| e.to_string())?;
    // 同步后按最新镜像重建提醒：新增、修改、完成、删除的任务都会在此处反映
    let _ = resync_from_db(&state);
    Ok(result)
}

#[tauri::command]
pub fn mobile_unpair(state: State<'_, Arc<AppState>>) -> Result<MobileSyncState, String> {
    state.mobile_sync().unpair().map_err(|e| e.to_string())
}

// ---- 只读数据命令：只查询镜像库，不提供任何业务写入能力 ----

#[tauri::command]
pub fn mobile_lists(state: State<'_, Arc<AppState>>) -> Result<Vec<TaskList>, String> {
    state.mobile_sync().lists().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_tasks(
    state: State<'_, Arc<AppState>>,
    query: MirrorTaskQuery,
) -> Result<Vec<Task>, String> {
    state.mobile_sync().tasks(&query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_task(state: State<'_, Arc<AppState>>, id: String) -> Result<Task, String> {
    state.mobile_sync().task(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_tasks_for_day(
    state: State<'_, Arc<AppState>>,
    date: String,
) -> Result<Vec<Task>, String> {
    state
        .mobile_sync()
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
        .mobile_sync()
        .calendar(&start, &end)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn mobile_settings_get(state: State<'_, Arc<AppState>>) -> MobileSettings {
    state.mobile_sync().local_settings()
}

#[tauri::command]
pub fn mobile_settings_update(
    state: State<'_, Arc<AppState>>,
    settings: MobileSettings,
) -> Result<MobileSettings, String> {
    let saved = state
        .mobile_sync()
        .update_local_settings(settings)
        .map_err(|e| e.to_string())?;
    // 开关或提前量变化后立即重建调度
    let _ = resync_from_db(&state);
    Ok(saved)
}

// ---- 独立 Android 版后台提醒与离线备份分享 ----

#[tauri::command]
pub fn mobile_reminders_resync(state: State<'_, Arc<AppState>>) -> ReminderStatus {
    auto_backup_if_due(&state);
    resync_from_db(&state)
}

#[tauri::command]
pub fn mobile_reminder_status(state: State<'_, Arc<AppState>>) -> Result<ReminderStatus, String> {
    let (enabled, stats) = {
        let conn = state.db.lock();
        let enabled = ltd_core::db::load_settings(&conn)
            .map_err(|error| error.to_string())?
            .notifications_enabled;
        let rows = reminders::reminder_candidates(&conn).map_err(|error| error.to_string())?;
        let (_, stats) = reminders::build_plan(&rows, ltd_core::domain::local_now_ms());
        (enabled, stats)
    };
    let native = reminders::status()?;
    Ok(ReminderStatus {
        enabled,
        lead_minutes: 0,
        scheduled: native.scheduled,
        exact_alarm_allowed: native.exact_alarm_allowed,
        notifications_allowed: native.notifications_allowed,
        ignoring_battery_optimizations: native.ignoring_battery_optimizations,
        skipped_past: stats.skipped_past,
        skipped_far: stats.skipped_far,
        truncated: stats.truncated,
        error: None,
    })
}

#[tauri::command]
pub fn mobile_reminder_request_notifications() -> Result<bool, String> {
    reminders::request_notifications()
}

#[tauri::command]
pub fn mobile_backup_share(path: String) -> Result<(), String> {
    reminders::share_backup(&path)
}

#[tauri::command]
pub fn mobile_reminder_open_exact_alarm_settings() -> Result<(), String> {
    reminders::open_exact_alarm_settings()
}

#[tauri::command]
pub fn mobile_reminder_open_battery_settings() -> Result<(), String> {
    reminders::open_battery_settings()
}

#[cfg(test)]
mod local_reminder_tests {
    use super::*;

    #[test]
    fn native_fired_marks_only_matching_pending_schedule() {
        let db = ltd_core::db::Db::open_memory().unwrap();
        let conn = db.lock();
        let list_id: String = conn
            .query_row("SELECT id FROM lists LIMIT 1", [], |row| row.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO tasks (id, list_id, title, reminder_at, created_at, updated_at)
             VALUES (?1, ?2, '重复任务', 5000, 1, 1)",
            rusqlite::params!["task-1", list_id],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO reminder_schedules
             (id, task_id, trigger_at, status, created_at)
             VALUES ('matching', 'task-1', 5000, 'pending', 1);
             INSERT INTO reminder_schedules
             (id, task_id, trigger_at, status, created_at)
             VALUES ('other', 'task-1', 6000, 'pending', 1);",
        )
        .unwrap();
        let fired = vec![FiredReminder {
            task_id: "task-1".into(),
            trigger_at: 5000,
        }];

        mark_native_fired(&conn, &fired, 7000).unwrap();
        mark_native_fired(&conn, &fired, 8000).unwrap();

        let matching: (String, i64) = conn
            .query_row(
                "SELECT status, fired_at FROM reminder_schedules WHERE id='matching'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let other: String = conn
            .query_row(
                "SELECT status FROM reminder_schedules WHERE id='other'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(matching, ("fired".into(), 7000));
        assert_eq!(other, "pending");
    }
}
