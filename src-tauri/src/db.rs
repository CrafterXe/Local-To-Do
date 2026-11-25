use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{AppResult, AppError};
use crate::models::AppSettings;

/// SQLite 连接封装。使用单连接 + 互斥锁，保证本地单用户场景下的一致性。
pub struct Db {
    pub path: PathBuf,
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let db = Self {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        };
        {
            let c = db.lock();
            migrate(&c)?;
            seed(&c)?;
            // 启动时先确定生效时区，再重建提醒计划，保证提醒按正确时区计算
            let settings = load_settings(&c)?;
            crate::domain::set_timezone(if settings.timezone_auto {
                None
            } else {
                settings.timezone.clone()
            });
            refresh_all_reminders(&c)?;
        }
        Ok(db)
    }

    /// 内存数据库（仅测试用）
    #[cfg(test)]
    pub fn open_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self {
            path: PathBuf::from(":memory:"),
            conn: Mutex::new(conn),
        };
        {
            let c = db.lock();
            migrate(&c)?;
            seed(&c)?;
        }
        Ok(db)
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

const SCHEMA_VERSION: i64 = 4;

fn migrate(conn: &Connection) -> AppResult<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(
            r#"
        CREATE TABLE IF NOT EXISTS lists (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            color TEXT NOT NULL DEFAULT '#2f6fed',
            icon TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            deleted_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS recurrences (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            interval INTEGER NOT NULL DEFAULT 1,
            weekdays TEXT NOT NULL DEFAULT '',
            month_day INTEGER,
            base_date TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            list_id TEXT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            note TEXT NOT NULL DEFAULT '',
            is_important INTEGER NOT NULL DEFAULT 0,
            is_completed INTEGER NOT NULL DEFAULT 0,
            completed_at INTEGER,
            due_date TEXT,
            due_time TEXT,
            reminder_offset INTEGER,
            reminder_enabled INTEGER NOT NULL DEFAULT 1,
            reminder_at INTEGER,
            recurrence_id TEXT REFERENCES recurrences(id) ON DELETE SET NULL,
            series_id TEXT,
            parent_task_id TEXT,
            my_day TEXT,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            deleted_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_list ON tasks(list_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_reminder ON tasks(reminder_at);
        CREATE INDEX IF NOT EXISTS idx_tasks_series ON tasks(series_id);
        CREATE INDEX IF NOT EXISTS idx_tasks_deleted ON tasks(deleted_at);

        CREATE TABLE IF NOT EXISTS task_steps (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            is_completed INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_steps_task ON task_steps(task_id);

        CREATE TABLE IF NOT EXISTS reminder_schedules (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            trigger_at INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            fired_at INTEGER,
            created_at INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_rem_status ON reminder_schedules(status, trigger_at);

        CREATE TABLE IF NOT EXISTS task_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            task_title TEXT NOT NULL,
            action TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT '',
            at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS notes (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL DEFAULT '',
            body TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            deleted_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
        )?;
    }
    if version < 2 {
        // v2：清单支持软删除（最近删除），保留清单中的任务与归属关系
        let has_column: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('lists') WHERE name = 'deleted_at'",
            [],
            |r| r.get(0),
        )?;
        if has_column == 0 {
            conn.execute_batch(
                "ALTER TABLE lists ADD COLUMN deleted_at INTEGER;
                 CREATE INDEX IF NOT EXISTS idx_lists_deleted ON lists(deleted_at);",
            )?;
        }
    }
    if version < 3 {
        // v3：任务级提醒开关，与提醒方案分离。历史任务保持原有提醒行为（默认开启）
        let has_column: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'reminder_enabled'",
            [],
            |r| r.get(0),
        )?;
        if has_column == 0 {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN reminder_enabled INTEGER NOT NULL DEFAULT 1;",
            )?;
        }
    }
    if version < 4 {
        // v4：重复任务改为单条记录 + 已重复次数
        let has_column: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('tasks') WHERE name = 'recurrence_count'",
            [],
            |r| r.get(0),
        )?;
        if has_column == 0 {
            conn.execute_batch(
                "ALTER TABLE tasks ADD COLUMN recurrence_count INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        consolidate_recurrences(conn)?;
    }
    if version < SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}

/// 旧版本通过复制任务实现重复，这里把同一系列的记录合并为一条：
/// 保留未完成（或日期最新）的一条，其余软删除，并按规则换算已重复次数。
fn consolidate_recurrences(conn: &Connection) -> AppResult<()> {
    use crate::domain::recurrence::{parse_date, Rule};

    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(series_id, id) AS sid, COALESCE(due_date, '') AS due_date,
                COALESCE(is_completed, 0) AS done, COALESCE(created_at, 0) AS created
         FROM tasks
         WHERE recurrence_id IS NOT NULL AND deleted_at IS NULL
         ORDER BY created ASC",
    )?;
    let rows: Vec<(String, String, String, i64, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let mut groups: std::collections::HashMap<String, Vec<(String, String, i64, i64)>> =
        std::collections::HashMap::new();
    for (id, sid, due, done, created) in rows {
        groups
            .entry(sid)
            .or_default()
            .push((id, due, done, created));
    }

    let now = crate::domain::local_now_ms();
    for (_sid, group) in groups {
        if group.len() < 2 {
            continue;
        }
        // 优先保留未完成记录；否则保留到期日期最新的记录
        let mut keep = group[0].clone();
        for item in &group {
            let better = (item.2 == 0 && keep.2 != 0) || (item.2 == keep.2 && item.1 > keep.1);
            if better {
                keep = item.clone();
            }
        }

        let rec: Option<(String, i64, String, Option<i64>, String)> = conn
            .query_row(
                "SELECT kind, interval, weekdays, month_day, base_date FROM recurrences
                 WHERE id = (SELECT recurrence_id FROM tasks WHERE id=?1)",
                [&keep.0],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .ok();
        let mut count = 0i64;
        let mut due = keep.1.clone();
        if let Some((kind, interval, weekdays, month_day, base_date)) = rec {
            if let (Some(base), Some(target)) = (parse_date(&base_date), parse_date(&keep.1)) {
                let wd: Vec<u32> = weekdays
                    .split(',')
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect();
                let rule = Rule::new(&kind, interval.max(1), wd, month_day.map(|m| m as u32), base);
                count = rule.index_of(target);
                if let Some(d) = rule.nth(count) {
                    due = d.format("%Y-%m-%d").to_string();
                }
            }
        }
        conn.execute(
            "UPDATE tasks SET recurrence_count=?1, due_date=?2 WHERE id=?3",
            rusqlite::params![count, due, keep.0],
        )?;

        for item in group {
            if item.0 == keep.0 {
                continue;
            }
            conn.execute(
                "UPDATE tasks SET deleted_at=?1 WHERE id=?2",
                rusqlite::params![now, item.0],
            )?;
            conn.execute(
                "UPDATE reminder_schedules SET status='cancelled' WHERE task_id=?1 AND status='pending'",
                [&item.0],
            )?;
        }
    }
    Ok(())
}

fn seed(conn: &Connection) -> AppResult<()> {
    let now = crate::domain::local_now_ms();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lists WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    if count == 0 {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO lists (id, name, color, icon, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, "任务", "#2f6fed", "", 0, now],
        )?;
        let defaults = AppSettings {
            default_list_id: Some(id),
            ..Default::default()
        };
        save_settings(conn, &defaults)?;
    } else {
        let has_settings: i64 =
            conn.query_row("SELECT COUNT(*) FROM app_settings WHERE key='theme'", [], |r| r.get(0))?;
        if has_settings == 0 {
            save_settings(conn, &AppSettings::default())?;
        }
    }
    Ok(())
}

/// 启动时重建所有未完成任务的提醒计划（应对系统时间/时区变化）
pub fn refresh_all_reminders(conn: &Connection) -> AppResult<()> {
    use crate::domain::compute_reminder_at;
    let mut stmt = conn.prepare(
        "SELECT id, due_date, due_time, reminder_offset, reminder_enabled FROM tasks
         WHERE is_completed = 0 AND deleted_at IS NULL",
    )?;
    let rows: Vec<(String, Option<String>, Option<String>, Option<i64>, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<Result<_, _>>()?;
    for (id, date, time, offset, enabled) in rows {
        let at = if enabled != 0 {
            // 开关开启但没有历史方案时按「准时提醒」处理
            compute_reminder_at(date.as_deref(), time.as_deref(), offset.or(Some(0)))
        } else {
            None
        };
        conn.execute(
            "UPDATE tasks SET reminder_at = ?1 WHERE id = ?2",
            rusqlite::params![at, id],
        )?;
        super::repo::sync_schedule(conn, &id, at)?;
    }
    Ok(())
}

/// 设置读写（避免循环依赖，放在 db 模块）
pub fn save_settings(conn: &Connection, s: &AppSettings) -> AppResult<()> {
    let pairs: Vec<(&str, String)> = vec![
        ("theme", s.theme.clone()),
        ("notifications_enabled", bool_str(s.notifications_enabled)),
        ("sound_enabled", bool_str(s.sound_enabled)),
        ("sound_path", s.sound_path.clone().unwrap_or_default()),
        ("close_to_tray", bool_str(s.close_to_tray)),
        ("autostart", bool_str(s.autostart)),
        ("start_minimized", bool_str(s.start_minimized)),
        ("auto_backup_enabled", bool_str(s.auto_backup_enabled)),
        (
            "auto_backup_interval_hours",
            s.auto_backup_interval_hours.to_string(),
        ),
        (
            "last_backup_at",
            s.last_backup_at.map(|v| v.to_string()).unwrap_or_default(),
        ),
        (
            "default_list_id",
            s.default_list_id.clone().unwrap_or_default(),
        ),
        ("timezone_auto", bool_str(s.timezone_auto)),
        ("timezone", s.timezone.clone().unwrap_or_default()),
    ];
    for (k, v) in pairs {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![k, v],
        )?;
    }
    Ok(())
}

fn bool_str(b: bool) -> String {
    if b {
        "1".into()
    } else {
        "0".into()
    }
}

/// 重置应用级设置为默认值。
/// 只操作 app_settings 表，不触碰任务、清单、备忘录、历史与最近删除数据。
pub fn reset_settings(conn: &Connection) -> AppResult<AppSettings> {
    let s = AppSettings::default();
    conn.execute("DELETE FROM app_settings", [])?;
    save_settings(conn, &s)?;
    crate::domain::set_timezone(None);
    Ok(s)
}

pub fn load_settings(conn: &Connection) -> AppResult<AppSettings> {
    let mut s = AppSettings::default();
    let mut stmt = conn.prepare("SELECT key, value FROM app_settings")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (k, v) in rows {
        match k.as_str() {
            "theme" => s.theme = if v == "light" { "light".into() } else { "dark".into() },
            "notifications_enabled" => s.notifications_enabled = v == "1",
            "sound_enabled" => s.sound_enabled = v == "1",
            "sound_path" => {
                s.sound_path = if v.trim().is_empty() { None } else { Some(v) }
            }
            "close_to_tray" => s.close_to_tray = v == "1",
            "autostart" => s.autostart = v == "1",
            "start_minimized" => s.start_minimized = v == "1",
            "auto_backup_enabled" => s.auto_backup_enabled = v == "1",
            "auto_backup_interval_hours" => {
                s.auto_backup_interval_hours = v.parse().unwrap_or(24).max(1)
            }
            "last_backup_at" => s.last_backup_at = v.parse::<i64>().ok(),
            "default_list_id" => {
                s.default_list_id = if v.is_empty() { None } else { Some(v) }
            }
            "timezone_auto" => s.timezone_auto = v != "0",
            "timezone" => s.timezone = if v.trim().is_empty() { None } else { Some(v) },
            _ => {}
        }
    }
    Ok(s)
}

/// 备份文件条目（db 模块内部复用）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DbStats {
    pub tasks: i64,
    pub lists: i64,
    pub notes: i64,
    pub pending_reminders: i64,
    pub database_bytes: i64,
}

pub fn stats(conn: &Connection, path: &Path) -> AppResult<DbStats> {
    let tasks: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    let lists: i64 = conn.query_row("SELECT COUNT(*) FROM lists", [], |r| r.get(0))?;
    let notes: i64 = conn.query_row(
        "SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reminder_schedules WHERE status='pending'",
        [],
        |r| r.get(0),
    )?;
    let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
    Ok(DbStats {
        tasks,
        lists,
        notes,
        pending_reminders: pending,
        database_bytes: size,
    })
}

impl From<AppError> for String {
    fn from(e: AppError) -> String {
        e.to_string()
    }
}
