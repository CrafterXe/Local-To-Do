use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::backup::{self, RestoreReport};
use crate::db::{self, DbStats};
use crate::error::{err, AppResult};
use crate::models::{
    AppInfo, AppSettings, BackupEntry, HistoryEntry, Note, Task, TaskList, TaskQuery,
};
use crate::repo;
use crate::repo::tasks::{SidebarCounts, TaskInput};
use crate::state::AppState;

// ---------------- 清单 ----------------

#[tauri::command]
pub fn list_lists(state: State<'_, Arc<AppState>>) -> AppResult<Vec<TaskList>> {
    let conn = state.db.lock();
    repo::all_lists(&conn)
}

#[tauri::command]
pub fn create_list(
    state: State<'_, Arc<AppState>>,
    name: String,
    color: Option<String>,
    icon: Option<String>,
) -> AppResult<TaskList> {
    let conn = state.db.lock();
    repo::create_list(
        &conn,
        &name,
        color.as_deref().unwrap_or("#2f6fed"),
        icon.as_deref().unwrap_or(""),
    )
}

#[tauri::command]
pub fn update_list(
    state: State<'_, Arc<AppState>>,
    id: String,
    name: String,
    color: String,
    icon: String,
) -> AppResult<TaskList> {
    let conn = state.db.lock();
    repo::update_list(&conn, &id, &name, &color, &icon)
}

#[tauri::command]
pub fn delete_list(
    state: State<'_, Arc<AppState>>,
    id: String,
    mode: String,
    target_list_id: Option<String>,
) -> AppResult<()> {
    let conn = state.db.lock();
    let r = repo::delete_list(&conn, &id, &mode, target_list_id.as_deref());
    drop(conn);
    state.notify_scheduler();
    r
}

#[tauri::command]
pub fn reorder_lists(state: State<'_, Arc<AppState>>, ids: Vec<String>) -> AppResult<()> {
    let conn = state.db.lock();
    repo::reorder_lists(&conn, &ids)
}

// ---------------- 任务 ----------------

#[tauri::command]
pub fn list_tasks(state: State<'_, Arc<AppState>>, query: TaskQuery) -> AppResult<Vec<Task>> {
    let conn = state.db.lock();
    repo::list_tasks(&conn, &query)
}

#[tauri::command]
pub fn get_task(state: State<'_, Arc<AppState>>, id: String) -> AppResult<Task> {
    let conn = state.db.lock();
    repo::get_task(&conn, &id)
}

#[tauri::command]
pub fn create_task(state: State<'_, Arc<AppState>>, input: TaskInput) -> AppResult<Task> {
    let conn = state.db.lock();
    let result = repo::create_task(&conn, &input);
    drop(conn);
    state.notify_scheduler();
    result
}

#[tauri::command]
pub fn update_task(state: State<'_, Arc<AppState>>, id: String, input: TaskInput) -> AppResult<Task> {
    let conn = state.db.lock();
    let result = repo::update_task(&conn, &id, &input);
    drop(conn);
    state.notify_scheduler();
    result
}

#[tauri::command]
pub fn set_important(state: State<'_, Arc<AppState>>, id: String, important: bool) -> AppResult<Task> {
    let conn = state.db.lock();
    repo::set_important(&conn, &id, important)
}

#[tauri::command]
pub fn set_my_day(state: State<'_, Arc<AppState>>, id: String, add: bool) -> AppResult<Task> {
    let conn = state.db.lock();
    repo::set_my_day(&conn, &id, add)
}

#[tauri::command]
pub fn move_task(state: State<'_, Arc<AppState>>, id: String, list_id: String) -> AppResult<Task> {
    let conn = state.db.lock();
    repo::move_task(&conn, &id, &list_id)
}

#[tauri::command]
pub fn complete_task(
    state: State<'_, Arc<AppState>>,
    id: String,
    completed: bool,
) -> AppResult<Option<Task>> {
    let conn = state.db.lock();
    let result = repo::complete_task(&conn, &id, completed);
    drop(conn);
    state.notify_scheduler();
    result
}

#[tauri::command]
pub fn skip_occurrence(state: State<'_, Arc<AppState>>, id: String) -> AppResult<Task> {
    let conn = state.db.lock();
    let result = repo::skip_occurrence(&conn, &id);
    drop(conn);
    state.notify_scheduler();
    result
}

#[tauri::command]
pub fn delete_task(state: State<'_, Arc<AppState>>, id: String, scope: String) -> AppResult<()> {
    let conn = state.db.lock();
    let result = repo::delete_task(&conn, &id, &scope);
    drop(conn);
    state.notify_scheduler();
    result
}

#[tauri::command]
pub fn restore_task(state: State<'_, Arc<AppState>>, id: String) -> AppResult<Task> {
    let conn = state.db.lock();
    let result = repo::restore_task(&conn, &id);
    drop(conn);
    state.notify_scheduler();
    result
}

#[tauri::command]
pub fn purge_task(state: State<'_, Arc<AppState>>, id: String) -> AppResult<()> {
    let conn = state.db.lock();
    repo::purge_task(&conn, &id)
}

#[tauri::command]
pub fn empty_deleted(state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let conn = state.db.lock();
    repo::empty_deleted(&conn)
}

#[tauri::command]
pub fn reorder_tasks(state: State<'_, Arc<AppState>>, ids: Vec<String>) -> AppResult<()> {
    let conn = state.db.lock();
    repo::reorder_tasks(&conn, &ids)
}

#[tauri::command]
pub fn sidebar_counts(state: State<'_, Arc<AppState>>) -> AppResult<SidebarCounts> {
    let conn = state.db.lock();
    repo::sidebar_counts(&conn)
}

#[tauri::command]
pub fn add_step(state: State<'_, Arc<AppState>>, task_id: String, title: String) -> AppResult<Task> {
    let conn = state.db.lock();
    repo::add_step(&conn, &task_id, &title)
}

#[tauri::command]
pub fn update_step(
    state: State<'_, Arc<AppState>>,
    step_id: String,
    title: String,
    completed: bool,
) -> AppResult<()> {
    let conn = state.db.lock();
    repo::update_step(&conn, &step_id, &title, completed)
}

#[tauri::command]
pub fn delete_step(state: State<'_, Arc<AppState>>, step_id: String) -> AppResult<()> {
    let conn = state.db.lock();
    repo::delete_step(&conn, &step_id)
}

#[tauri::command]
pub fn list_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<i64>,
) -> AppResult<Vec<HistoryEntry>> {
    let conn = state.db.lock();
    repo::list_history(&conn, limit.unwrap_or(200))
}

#[tauri::command]
pub fn list_task_history(state: State<'_, Arc<AppState>>, task_id: String) -> AppResult<Vec<HistoryEntry>> {
    let conn = state.db.lock();
    repo::list_task_history(&conn, &task_id)
}

// ---------------- 笔记 ----------------

#[tauri::command]
pub fn list_notes(state: State<'_, Arc<AppState>>, search: Option<String>) -> AppResult<Vec<Note>> {
    let conn = state.db.lock();
    repo::list_notes(&conn, search.as_deref())
}

#[tauri::command]
pub fn get_note(state: State<'_, Arc<AppState>>, id: String) -> AppResult<Note> {
    let conn = state.db.lock();
    repo::get_note(&conn, &id)
}

#[tauri::command]
pub fn create_note(state: State<'_, Arc<AppState>>, title: String, body: String) -> AppResult<Note> {
    let conn = state.db.lock();
    repo::create_note(&conn, &title, &body)
}

#[tauri::command]
pub fn update_note(
    state: State<'_, Arc<AppState>>,
    id: String,
    title: String,
    body: String,
) -> AppResult<Note> {
    let conn = state.db.lock();
    repo::update_note(&conn, &id, &title, &body)
}

#[tauri::command]
pub fn delete_note(state: State<'_, Arc<AppState>>, id: String) -> AppResult<()> {
    let conn = state.db.lock();
    repo::delete_note(&conn, &id)
}

// ---------------- 设置与信息 ----------------

#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> AppResult<AppSettings> {
    let conn = state.db.lock();
    db::load_settings(&conn)
}

fn apply_theme(app: &AppHandle, theme: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let t = if theme == "light" {
            tauri::Theme::Light
        } else {
            tauri::Theme::Dark
        };
        let _ = w.set_theme(Some(t));
    }
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    settings: AppSettings,
) -> AppResult<AppSettings> {
    let mut s = settings;
    s.auto_backup_interval_hours = s.auto_backup_interval_hours.max(1);
    if s.theme != "light" {
        s.theme = "dark".into();
    }
    {
        let conn = state.db.lock();
        db::save_settings(&conn, &s)?;
    }
    apply_theme(&app, &s.theme);
    apply_autostart(&app, s.autostart);
    state.notify_scheduler();
    Ok(s)
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let current = manager.is_enabled().unwrap_or(false);
    if enabled && !current {
        if let Err(e) = manager.enable() {
            crate::logging::warn(format!("开启开机自启失败: {e}"));
        }
    } else if !enabled && current {
        if let Err(e) = manager.disable() {
            crate::logging::warn(format!("关闭开机自启失败: {e}"));
        }
    }
}

#[tauri::command]
pub fn get_app_info(app: AppHandle, state: State<'_, Arc<AppState>>) -> AppResult<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        db_path: state.db.path.to_string_lossy().to_string(),
        backup_dir: state.backup_dir.to_string_lossy().to_string(),
        log_dir: state.log_dir.to_string_lossy().to_string(),
        today: crate::domain::today_string(),
        now: crate::domain::local_now_ms(),
    })
}

#[tauri::command]
pub fn get_db_stats(state: State<'_, Arc<AppState>>) -> AppResult<DbStats> {
    let conn = state.db.lock();
    db::stats(&conn, &state.db.path)
}

#[tauri::command]
pub fn send_test_notification(app: AppHandle) -> AppResult<bool> {
    let ok = crate::scheduler::send_notification(
        &app,
        "Local To Do 测试通知",
        "如果你看到了这条通知，说明系统提醒功能工作正常。",
    );
    Ok(ok)
}

#[tauri::command]
pub fn get_next_reminder(state: State<'_, Arc<AppState>>) -> AppResult<Option<i64>> {
    let conn = state.db.lock();
    repo::next_trigger(&conn)
}

// ---------------- 备份 ----------------

#[tauri::command]
pub fn create_backup(state: State<'_, Arc<AppState>>) -> AppResult<BackupEntry> {
    let conn = state.db.lock();
    backup::backup_now(&state.backup_dir, &conn)
}

#[tauri::command]
pub fn list_backups(state: State<'_, Arc<AppState>>) -> AppResult<Vec<BackupEntry>> {
    backup::list_backups(&state.backup_dir)
}

#[tauri::command]
pub fn export_json_file(state: State<'_, Arc<AppState>>, path: String) -> AppResult<String> {
    let conn = state.db.lock();
    backup::export_json(&conn, std::path::Path::new(&path))?;
    Ok(path)
}

#[tauri::command]
pub fn export_csv_file(state: State<'_, Arc<AppState>>, path: String) -> AppResult<String> {
    let conn = state.db.lock();
    backup::export_csv(&conn, std::path::Path::new(&path))?;
    Ok(path)
}

#[tauri::command]
pub fn restore_backup(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    path: String,
) -> AppResult<RestoreReport> {
    let mut conn = state.db.lock();
    let report = match backup::restore_from_json(&mut conn, std::path::Path::new(&path)) {
        Ok(r) => r,
        Err(e) => return Err(e),
    };
    drop(conn);
    state.notify_scheduler();
    let _ = app.emit("data-restored", report.clone());
    Ok(report)
}

#[tauri::command]
pub fn validate_backup(path: String) -> AppResult<RestoreReport> {
    let text = std::fs::read_to_string(&path)?;
    let bundle: serde_json::Value = serde_json::from_str(&text)?;
    if bundle.get("format").and_then(|v| v.as_str()) != Some("local-to-do-backup") {
        return err("不是有效的 Local To Do 备份文件");
    }
    let data = bundle
        .get("data")
        .ok_or_else(|| crate::error::AppError::Other("备份文件缺少数据".into()))?;
    let count = |name: &str| {
        data.get(name)
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0)
    };
    Ok(RestoreReport {
        lists: count("lists"),
        tasks: count("tasks"),
        notes: count("notes"),
        recurrences: count("recurrences"),
    })
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(state.data_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::AppError::Other(e.to_string()))
}

#[tauri::command]
pub fn open_backup_dir(app: AppHandle, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(state.backup_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::AppError::Other(e.to_string()))
}
