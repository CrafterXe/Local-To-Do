//! Android 只读镜像库。
//!
//! 镜像库与 Windows 权威数据库完全独立：这里只接受同步程序写入，
//! 不暴露任何用户可调用的任务写接口，也不写入 `sync_log`。
//!
//! 设计要点：
//! - 表列与 `ltd_core::sync` 的行模型一一对应（camelCase JSON → 列）；
//! - 不设外键，允许按 rev 顺序独立应用，避免镜像侧约束影响同步；
//! - 每批增量与其对应的 `last_rev` 在同一个事务内提交，失败整批回滚；
//! - `reset` 先持久化 `needs_snapshot` 再清空数据，进程崩溃后仍能识别。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{err, AppError, AppResult};
use crate::sync::{
    self, ListRow, RecurrenceRow, StepRow, SyncChange, SyncSnapshot, TaskRow,
};

const SCHEMA_VERSION: &str = "1";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS lists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '',
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
    list_id TEXT NOT NULL,
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
    recurrence_id TEXT,
    series_id TEXT,
    parent_task_id TEXT,
    recurrence_count INTEGER NOT NULL DEFAULT 0,
    my_day TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER
);

CREATE TABLE IF NOT EXISTS task_steps (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    title TEXT NOT NULL,
    is_completed INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_mirror_tasks_list ON tasks(list_id);
CREATE INDEX IF NOT EXISTS idx_mirror_steps_task ON task_steps(task_id);

CREATE TABLE IF NOT EXISTS mirror_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// 镜像同步状态（只读展示用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorMeta {
    /// 是否已完成过至少一次完整快照
    pub initialized: bool,
    /// 是否需要重新获取快照（reset 后或快照中断）
    pub needs_snapshot: bool,
    pub last_rev: i64,
    pub last_sync_at: Option<i64>,
}

/// 镜像行数统计
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorCounts {
    pub lists: i64,
    pub tasks: i64,
    pub recurrences: i64,
    pub steps: i64,
}

/// 单批增量的应用结果
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub reset: bool,
    pub applied: usize,
    pub last_rev: i64,
}

/// 只读镜像数据库句柄（单连接 + 互斥锁）
pub struct MirrorDb {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl MirrorDb {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;

        let db = Self {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        };
        {
            let c = db.lock();
            init_meta(&c)?;
        }
        Ok(db)
    }

    /// 内存数据库（测试用，需开启 test-utils 特性）
    #[cfg(any(test, feature = "test-utils"))]
    pub fn open_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let db = Self {
            path: PathBuf::from(":memory:"),
            conn: Mutex::new(conn),
        };
        {
            let c = db.lock();
            init_meta(&c)?;
        }
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn meta(&self) -> AppResult<MirrorMeta> {
        let conn = self.lock();
        read_meta(&conn)
    }

    pub fn counts(&self) -> AppResult<MirrorCounts> {
        let conn = self.lock();
        Ok(MirrorCounts {
            lists: count(&conn, "lists")?,
            tasks: count(&conn, "tasks")?,
            recurrences: count(&conn, "recurrences")?,
            steps: count(&conn, "task_steps")?,
        })
    }

    /// 应用完整快照：清空旧镜像、写入全部数据并推进游标，全部在同一个事务中完成。
    pub fn apply_snapshot(&self, snapshot: &SyncSnapshot, now: i64) -> AppResult<()> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        clear_tables(&tx)?;
        for list in &snapshot.lists {
            if list.deleted_at.is_none() {
                upsert_list(&tx, list)?;
            }
        }
        for recurrence in &snapshot.recurrences {
            upsert_recurrence(&tx, recurrence)?;
        }
        for task in &snapshot.tasks {
            if task.deleted_at.is_none() {
                upsert_task(&tx, task)?;
            }
        }
        for step in &snapshot.steps {
            upsert_step(&tx, step)?;
        }
        set_meta(&tx, "initialized", "1")?;
        set_meta(&tx, "needs_snapshot", "0")?;
        set_meta(&tx, "last_rev", &snapshot.rev.to_string())?;
        set_meta(&tx, "last_sync_at", &now.to_string())?;
        tx.commit()?;
        Ok(())
    }

    /// 应用一批增量：数据与 `last_rev` 同事务提交；重复批次因 rev 已推进而幂等跳过。
    pub fn apply_changes(&self, changes: &[SyncChange]) -> AppResult<ApplyOutcome> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let meta = read_meta(&tx)?;
        if !meta.initialized {
            return err("本地镜像尚未完成首次同步，无法应用增量");
        }

        let mut last_rev = meta.last_rev;
        let mut applied = 0usize;
        for change in changes {
            // 已应用过的记录（重复请求或断点重试）直接跳过
            if change.rev <= last_rev {
                continue;
            }
            if change.entity == "*" && change.op == sync::OP_RESET {
                // 先落盘「需要快照」标记，再清空镜像；即使随后进程退出也能恢复
                set_meta(&tx, "needs_snapshot", "1")?;
                set_meta(&tx, "initialized", "0")?;
                set_meta(&tx, "last_rev", "0")?;
                clear_tables(&tx)?;
                tx.commit()?;
                return Ok(ApplyOutcome {
                    reset: true,
                    applied,
                    last_rev: 0,
                });
            }
            apply_change(&tx, change)?;
            last_rev = change.rev;
            set_meta(&tx, "last_rev", &last_rev.to_string())?;
            applied += 1;
        }
        tx.commit()?;
        Ok(ApplyOutcome {
            reset: false,
            applied,
            last_rev,
        })
    }

    /// 同步完全成功后记录时间
    pub fn mark_sync_success(&self, now: i64) -> AppResult<()> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        set_meta(&tx, "last_sync_at", &now.to_string())?;
        tx.commit()?;
        Ok(())
    }
}

fn init_meta(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO mirror_meta (key, value) VALUES
            ('schema_version', ?1), ('initialized', '0'), ('needs_snapshot', '0'), ('last_rev', '0')",
        [SCHEMA_VERSION],
    )?;
    Ok(())
}

fn read_meta(conn: &Connection) -> AppResult<MirrorMeta> {
    let mut stmt = conn.prepare("SELECT key, value FROM mirror_meta")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut map: HashMap<String, String> = HashMap::new();
    for row in rows {
        let (key, value) = row?;
        map.insert(key, value);
    }
    Ok(MirrorMeta {
        initialized: map.get("initialized").map(|v| v == "1").unwrap_or(false),
        needs_snapshot: map.get("needs_snapshot").map(|v| v == "1").unwrap_or(false),
        last_rev: map
            .get("last_rev")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0),
        last_sync_at: map.get("last_sync_at").and_then(|v| v.parse::<i64>().ok()),
    })
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO mirror_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn count(conn: &Connection, table: &str) -> AppResult<i64> {
    // 表名固定为内部常量，不接受外部输入
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(conn.query_row(&sql, [], |r| r.get(0))?)
}

fn clear_tables(conn: &Connection) -> AppResult<()> {
    conn.execute("DELETE FROM task_steps", [])?;
    conn.execute("DELETE FROM tasks", [])?;
    conn.execute("DELETE FROM recurrences", [])?;
    conn.execute("DELETE FROM lists", [])?;
    Ok(())
}

fn decode_payload<T: DeserializeOwned>(change: &SyncChange) -> AppResult<T> {
    let payload = change
        .payload
        .as_ref()
        .ok_or_else(|| AppError::Other(format!("同步记录 rev={} 缺少实体数据", change.rev)))?;
    Ok(serde_json::from_value(payload.clone())?)
}

fn apply_change(conn: &Connection, change: &SyncChange) -> AppResult<()> {
    match change.entity.as_str() {
        sync::ENTITY_LIST => match change.op.as_str() {
            sync::OP_UPSERT => {
                let row: ListRow = decode_payload(change)?;
                if row.deleted_at.is_some() {
                    delete_list(conn, &row.id)
                } else {
                    upsert_list(conn, &row)
                }
            }
            sync::OP_DELETE => delete_list(conn, &change.entity_id),
            other => err(format!("未知的清单同步操作：{other}")),
        },
        sync::ENTITY_RECURRENCE => match change.op.as_str() {
            sync::OP_UPSERT => {
                let row: RecurrenceRow = decode_payload(change)?;
                upsert_recurrence(conn, &row)
            }
            sync::OP_DELETE => {
                conn.execute("DELETE FROM recurrences WHERE id = ?1", [&change.entity_id])?;
                Ok(())
            }
            other => err(format!("未知的重复规则同步操作：{other}")),
        },
        sync::ENTITY_TASK => match change.op.as_str() {
            sync::OP_UPSERT => {
                let row: TaskRow = decode_payload(change)?;
                if row.deleted_at.is_some() {
                    delete_task(conn, &row.id)
                } else {
                    upsert_task(conn, &row)
                }
            }
            sync::OP_DELETE => delete_task(conn, &change.entity_id),
            other => err(format!("未知的任务同步操作：{other}")),
        },
        sync::ENTITY_STEP => match change.op.as_str() {
            sync::OP_UPSERT => {
                let row: StepRow = decode_payload(change)?;
                upsert_step(conn, &row)
            }
            sync::OP_DELETE => {
                conn.execute("DELETE FROM task_steps WHERE id = ?1", [&change.entity_id])?;
                Ok(())
            }
            other => err(format!("未知的任务步骤同步操作：{other}")),
        },
        other => err(format!("未知的同步实体：{other}")),
    }
}

fn upsert_list(conn: &Connection, row: &ListRow) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO lists (id, name, color, icon, sort_order, created_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            row.id,
            row.name,
            row.color,
            row.icon,
            row.sort_order,
            row.created_at,
            row.deleted_at
        ],
    )?;
    Ok(())
}

fn upsert_recurrence(conn: &Connection, row: &RecurrenceRow) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO recurrences
            (id, kind, interval, weekdays, month_day, base_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            row.id,
            row.kind,
            row.interval,
            row.weekdays,
            row.month_day,
            row.base_date,
            row.created_at
        ],
    )?;
    Ok(())
}

fn upsert_task(conn: &Connection, row: &TaskRow) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO tasks
            (id, list_id, title, note, is_important, is_completed, completed_at,
             due_date, due_time, reminder_offset, reminder_enabled, reminder_at,
             recurrence_id, series_id, parent_task_id, recurrence_count, my_day,
             sort_order, created_at, updated_at, deleted_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                 ?17, ?18, ?19, ?20, ?21)",
        params![
            row.id,
            row.list_id,
            row.title,
            row.note,
            row.is_important as i64,
            row.is_completed as i64,
            row.completed_at,
            row.due_date,
            row.due_time,
            row.reminder_offset,
            row.reminder_enabled as i64,
            row.reminder_at,
            row.recurrence_id,
            row.series_id,
            row.parent_task_id,
            row.recurrence_count,
            row.my_day,
            row.sort_order,
            row.created_at,
            row.updated_at,
            row.deleted_at
        ],
    )?;
    Ok(())
}

fn upsert_step(conn: &Connection, row: &StepRow) -> AppResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO task_steps (id, task_id, title, is_completed, sort_order)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            row.id,
            row.task_id,
            row.title,
            row.is_completed as i64,
            row.sort_order
        ],
    )?;
    Ok(())
}

/// 删除清单：同时清理该清单下的任务与步骤，避免镜像出现孤儿数据。
fn delete_list(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM task_steps WHERE task_id IN (SELECT id FROM tasks WHERE list_id = ?1)",
        [id],
    )?;
    conn.execute("DELETE FROM tasks WHERE list_id = ?1", [id])?;
    conn.execute("DELETE FROM lists WHERE id = ?1", [id])?;
    Ok(())
}

/// 删除任务：同时清理其步骤（服务端也会单独发步骤墓碑，重复删除是幂等的）。
fn delete_task(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM task_steps WHERE task_id = ?1", [id])?;
    conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::{SyncChange, SyncSnapshot};

    fn snapshot(rev: i64) -> SyncSnapshot {
        SyncSnapshot {
            rev,
            lists: vec![ListRow {
                id: "l1".into(),
                name: "任务".into(),
                color: "#fff".into(),
                icon: String::new(),
                sort_order: 0,
                created_at: 1,
                deleted_at: None,
            }],
            recurrences: vec![],
            tasks: vec![TaskRow {
                id: "t1".into(),
                list_id: "l1".into(),
                title: "任务一".into(),
                note: String::new(),
                is_important: false,
                is_completed: false,
                completed_at: None,
                due_date: None,
                due_time: None,
                reminder_offset: None,
                reminder_enabled: false,
                reminder_at: None,
                recurrence_id: None,
                series_id: None,
                parent_task_id: None,
                recurrence_count: 0,
                my_day: None,
                sort_order: 0,
                created_at: 1,
                updated_at: 1,
                deleted_at: None,
            }],
            steps: vec![],
        }
    }

    #[test]
    fn snapshot_and_cursor_in_one_transaction() {
        let db = MirrorDb::open_memory().unwrap();
        assert!(!db.meta().unwrap().initialized);
        db.apply_snapshot(&snapshot(7), 100).unwrap();
        let meta = db.meta().unwrap();
        assert!(meta.initialized);
        assert!(!meta.needs_snapshot);
        assert_eq!(meta.last_rev, 7);
        assert_eq!(meta.last_sync_at, Some(100));
        assert_eq!(db.counts().unwrap().tasks, 1);
    }

    #[test]
    fn snapshot_failure_rolls_back_everything() {
        let db = MirrorDb::open_memory().unwrap();
        db.apply_snapshot(&snapshot(3), 100).unwrap();
        {
            let conn = db.lock();
            conn.execute_batch(
                "CREATE TRIGGER fail_tasks BEFORE INSERT ON tasks
                 BEGIN SELECT RAISE(ABORT, 'boom'); END;",
            )
            .unwrap();
        }
        let mut next = snapshot(9);
        next.lists[0].name = "改名".into();
        assert!(db.apply_snapshot(&next, 200).is_err());
        let meta = db.meta().unwrap();
        assert_eq!(meta.last_rev, 3);
        assert_eq!(meta.last_sync_at, Some(100));
        let conn = db.lock();
        let name: String = conn
            .query_row("SELECT name FROM lists WHERE id='l1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "任务");
    }

    #[test]
    fn changes_are_idempotent_and_ordered() {
        let db = MirrorDb::open_memory().unwrap();
        db.apply_snapshot(&snapshot(1), 100).unwrap();
        let update = SyncChange {
            rev: 2,
            entity: "task".into(),
            entity_id: "t1".into(),
            op: "upsert".into(),
            payload: Some(serde_json::to_value(&{
                let mut t = snapshot(1).tasks.remove(0);
                t.title = "已更新".into();
                t
            })
            .unwrap()),
            changed_at: 2,
        };
        let batch = vec![update.clone()];
        let first = db.apply_changes(&batch).unwrap();
        assert_eq!(first.applied, 1);
        assert_eq!(first.last_rev, 2);
        let second = db.apply_changes(&batch).unwrap();
        assert_eq!(second.applied, 0);
        assert_eq!(second.last_rev, 2);
        let conn = db.lock();
        let title: String = conn
            .query_row("SELECT title FROM tasks WHERE id='t1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "已更新");
    }

    #[test]
    fn reset_marks_needs_snapshot_before_clearing() {
        let db = MirrorDb::open_memory().unwrap();
        db.apply_snapshot(&snapshot(5), 100).unwrap();
        let reset = SyncChange {
            rev: 6,
            entity: "*".into(),
            entity_id: "*".into(),
            op: "reset".into(),
            payload: None,
            changed_at: 6,
        };
        let outcome = db.apply_changes(&[reset]).unwrap();
        assert!(outcome.reset);
        let meta = db.meta().unwrap();
        assert!(meta.needs_snapshot);
        assert!(!meta.initialized);
        assert_eq!(meta.last_rev, 0);
        assert_eq!(db.counts().unwrap().tasks, 0);
    }
}
