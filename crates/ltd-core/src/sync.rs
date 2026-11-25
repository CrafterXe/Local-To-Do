//! 单向同步基础（Windows → Android）。
//!
//! 数据模型：
//! - `sync_log` 使用全局自增 `rev` 作为唯一顺序，`changed_at` 只用于记录时间；
//! - 业务写入与同步记录在同一个 SQLite 事务中提交；
//! - upsert 记录携带实体行的 JSON 快照，delete 记录不携带数据（墓碑）；
//! - Android 首次通过 `snapshot()` 取得与 `rev` 一致的全量快照，之后用
//!   `changes_since(last_rev)` 获取增量。
//!
//! 本模块只处理数据，不涉及网络与设备管理。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

use crate::error::AppResult;

pub const ENTITY_LIST: &str = "list";
pub const ENTITY_TASK: &str = "task";
pub const ENTITY_RECURRENCE: &str = "recurrence";
pub const ENTITY_STEP: &str = "step";

pub const OP_UPSERT: &str = "upsert";
pub const OP_DELETE: &str = "delete";
/// 全量重建标记：客户端收到后应丢弃本地游标并重新拉取快照
pub const OP_RESET: &str = "reset";

const DEFAULT_CHANGE_LIMIT: i64 = 500;

/// 同步日志中的清单行（不含统计字段）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRow {
    pub id: String,
    pub name: String,
    pub color: String,
    pub icon: String,
    pub sort_order: i64,
    pub created_at: i64,
    pub deleted_at: Option<i64>,
}

/// 同步日志中的重复规则行
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRow {
    pub id: String,
    pub kind: String,
    pub interval: i64,
    pub weekdays: String,
    pub month_day: Option<i64>,
    pub base_date: String,
    pub created_at: i64,
}

/// 同步日志中的任务行（步骤与重复规则作为独立实体同步）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub id: String,
    pub list_id: String,
    pub title: String,
    pub note: String,
    pub is_important: bool,
    pub is_completed: bool,
    pub completed_at: Option<i64>,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub reminder_offset: Option<i64>,
    pub reminder_enabled: bool,
    pub reminder_at: Option<i64>,
    pub recurrence_id: Option<String>,
    pub series_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub recurrence_count: i64,
    pub my_day: Option<String>,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

/// 同步日志中的任务步骤行
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRow {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub is_completed: bool,
    pub sort_order: i64,
}

/// 一次性全量快照。`rev` 与所有数据来自同一个只读事务。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSnapshot {
    pub rev: i64,
    pub lists: Vec<ListRow>,
    pub recurrences: Vec<RecurrenceRow>,
    pub tasks: Vec<TaskRow>,
    pub steps: Vec<StepRow>,
}

/// 单条增量变更
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncChange {
    pub rev: i64,
    pub entity: String,
    pub entity_id: String,
    pub op: String,
    pub payload: Option<Value>,
    pub changed_at: i64,
}

/// 增量查询结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncChanges {
    /// 查询时数据库中的最大 rev，不代表已全部返回
    pub latest_rev: i64,
    /// 是否还有更多变更（客户端应用完本批后继续拉取）
    pub has_more: bool,
    pub changes: Vec<SyncChange>,
}

fn to_json<T: Serialize>(row: &T) -> AppResult<String> {
    Ok(serde_json::to_string(row)?)
}

fn insert_log(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    op: &str,
    payload: Option<String>,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO sync_log (entity, entity_id, op, payload, changed_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![entity, entity_id, op, payload, crate::domain::local_now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

fn read_list(conn: &Connection, id: &str) -> AppResult<Option<ListRow>> {
    Ok(conn
        .query_row(
            "SELECT id, name, color, icon, sort_order, created_at, deleted_at
             FROM lists WHERE id = ?1",
            [id],
            |r| {
                Ok(ListRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    color: r.get(2)?,
                    icon: r.get(3)?,
                    sort_order: r.get(4)?,
                    created_at: r.get(5)?,
                    deleted_at: r.get(6)?,
                })
            },
        )
        .optional()?)
}

fn read_recurrence(conn: &Connection, id: &str) -> AppResult<Option<RecurrenceRow>> {
    Ok(conn
        .query_row(
            "SELECT id, kind, interval, weekdays, month_day, base_date, created_at
             FROM recurrences WHERE id = ?1",
            [id],
            |r| {
                Ok(RecurrenceRow {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    interval: r.get(2)?,
                    weekdays: r.get(3)?,
                    month_day: r.get(4)?,
                    base_date: r.get(5)?,
                    created_at: r.get(6)?,
                })
            },
        )
        .optional()?)
}

fn read_task(conn: &Connection, id: &str) -> AppResult<Option<TaskRow>> {
    Ok(conn
        .query_row(
            "SELECT id, list_id, title, note, is_important, is_completed, completed_at,
                    due_date, due_time, reminder_offset, reminder_enabled, reminder_at,
                    recurrence_id, series_id, parent_task_id, recurrence_count, my_day,
                    sort_order, created_at, updated_at, deleted_at
             FROM tasks WHERE id = ?1",
            [id],
            |r| {
                Ok(TaskRow {
                    id: r.get(0)?,
                    list_id: r.get(1)?,
                    title: r.get(2)?,
                    note: r.get(3)?,
                    is_important: r.get::<_, i64>(4)? != 0,
                    is_completed: r.get::<_, i64>(5)? != 0,
                    completed_at: r.get(6)?,
                    due_date: r.get(7)?,
                    due_time: r.get(8)?,
                    reminder_offset: r.get(9)?,
                    reminder_enabled: r.get::<_, i64>(10)? != 0,
                    reminder_at: r.get(11)?,
                    recurrence_id: r.get(12)?,
                    series_id: r.get(13)?,
                    parent_task_id: r.get(14)?,
                    recurrence_count: r.get(15)?,
                    my_day: r.get(16)?,
                    sort_order: r.get(17)?,
                    created_at: r.get(18)?,
                    updated_at: r.get(19)?,
                    deleted_at: r.get(20)?,
                })
            },
        )
        .optional()?)
}

fn read_step(conn: &Connection, id: &str) -> AppResult<Option<StepRow>> {
    Ok(conn
        .query_row(
            "SELECT id, task_id, title, is_completed, sort_order FROM task_steps WHERE id = ?1",
            [id],
            |r| {
                Ok(StepRow {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    title: r.get(2)?,
                    is_completed: r.get::<_, i64>(3)? != 0,
                    sort_order: r.get(4)?,
                })
            },
        )
        .optional()?)
}

/// 记录一条 upsert。实体已不存在时退化为 delete，避免产生悬空引用。
fn log_row<T: Serialize>(
    conn: &Connection,
    entity: &str,
    entity_id: &str,
    row: Option<T>,
) -> AppResult<()> {
    match row {
        Some(row) => {
            insert_log(conn, entity, entity_id, OP_UPSERT, Some(to_json(&row)?))?;
        }
        None => {
            insert_log(conn, entity, entity_id, OP_DELETE, None)?;
        }
    }
    Ok(())
}

pub fn log_list(conn: &Connection, id: &str) -> AppResult<()> {
    let row = read_list(conn, id)?;
    log_row(conn, ENTITY_LIST, id, row)
}

pub fn log_recurrence(conn: &Connection, id: &str) -> AppResult<()> {
    let row = read_recurrence(conn, id)?;
    log_row(conn, ENTITY_RECURRENCE, id, row)
}

pub fn log_task(conn: &Connection, id: &str) -> AppResult<()> {
    let row = read_task(conn, id)?;
    log_row(conn, ENTITY_TASK, id, row)
}

pub fn log_step(conn: &Connection, id: &str) -> AppResult<()> {
    let row = read_step(conn, id)?;
    log_row(conn, ENTITY_STEP, id, row)
}

pub fn log_delete(conn: &Connection, entity: &str, entity_id: &str) -> AppResult<()> {
    insert_log(conn, entity, entity_id, OP_DELETE, None)?;
    Ok(())
}

/// 记录任务的全部步骤为 upsert（恢复任务时使用）
pub fn log_steps_of_task(conn: &Connection, task_id: &str) -> AppResult<()> {
    let ids: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT id FROM task_steps WHERE task_id = ?1 ORDER BY sort_order ASC")?;
        let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for id in ids {
        log_step(conn, &id)?;
    }
    Ok(())
}

/// 记录任务的全部步骤为 delete（删除任务前调用，随后步骤会随任务级联删除）
pub fn log_delete_steps_of_task(conn: &Connection, task_id: &str) -> AppResult<()> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM task_steps WHERE task_id = ?1")?;
        let rows = stmt.query_map([task_id], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for id in ids {
        log_delete(conn, ENTITY_STEP, &id)?;
    }
    Ok(())
}

/// 若重复规则已不被任何任务引用，则记录删除（规则本身仍保留在库中）
pub fn log_recurrence_if_orphan(conn: &Connection, id: &str) -> AppResult<()> {
    let refs: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE recurrence_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if refs == 0 {
        insert_log(conn, ENTITY_RECURRENCE, id, OP_DELETE, None)?;
    }
    Ok(())
}

/// 写入全量重建标记（备份恢复等整体替换数据后使用）
pub fn record_reset(conn: &Connection) -> AppResult<()> {
    insert_log(conn, "*", "*", OP_RESET, None)?;
    Ok(())
}

pub fn current_rev(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row("SELECT IFNULL(MAX(rev), 0) FROM sync_log", [], |r| r.get(0))?)
}

/// 一致性快照：rev 与所有实体行在同一个只读事务中读取。
pub fn snapshot(conn: &Connection) -> AppResult<SyncSnapshot> {
    let tx = conn.unchecked_transaction()?;

    let rev = current_rev(&tx)?;

    let lists = {
        let mut stmt = tx.prepare(
            "SELECT id, name, color, icon, sort_order, created_at, deleted_at
             FROM lists WHERE deleted_at IS NULL
             ORDER BY sort_order ASC, created_at ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ListRow {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                icon: r.get(3)?,
                sort_order: r.get(4)?,
                created_at: r.get(5)?,
                deleted_at: r.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let recurrences = {
        let mut stmt = tx.prepare(
            "SELECT r.id, r.kind, r.interval, r.weekdays, r.month_day, r.base_date, r.created_at
             FROM recurrences r
             WHERE EXISTS (
                 SELECT 1 FROM tasks t WHERE t.recurrence_id = r.id AND t.deleted_at IS NULL
             )",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(RecurrenceRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                interval: r.get(2)?,
                weekdays: r.get(3)?,
                month_day: r.get(4)?,
                base_date: r.get(5)?,
                created_at: r.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let tasks = {
        let mut stmt = tx.prepare(
            "SELECT id, list_id, title, note, is_important, is_completed, completed_at,
                    due_date, due_time, reminder_offset, reminder_enabled, reminder_at,
                    recurrence_id, series_id, parent_task_id, recurrence_count, my_day,
                    sort_order, created_at, updated_at, deleted_at
             FROM tasks WHERE deleted_at IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(TaskRow {
                id: r.get(0)?,
                list_id: r.get(1)?,
                title: r.get(2)?,
                note: r.get(3)?,
                is_important: r.get::<_, i64>(4)? != 0,
                is_completed: r.get::<_, i64>(5)? != 0,
                completed_at: r.get(6)?,
                due_date: r.get(7)?,
                due_time: r.get(8)?,
                reminder_offset: r.get(9)?,
                reminder_enabled: r.get::<_, i64>(10)? != 0,
                reminder_at: r.get(11)?,
                recurrence_id: r.get(12)?,
                series_id: r.get(13)?,
                parent_task_id: r.get(14)?,
                recurrence_count: r.get(15)?,
                my_day: r.get(16)?,
                sort_order: r.get(17)?,
                created_at: r.get(18)?,
                updated_at: r.get(19)?,
                deleted_at: r.get(20)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let steps = {
        let mut stmt = tx.prepare(
            "SELECT s.id, s.task_id, s.title, s.is_completed, s.sort_order
             FROM task_steps s
             WHERE EXISTS (
                 SELECT 1 FROM tasks t WHERE t.id = s.task_id AND t.deleted_at IS NULL
             )
             ORDER BY s.task_id ASC, s.sort_order ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(StepRow {
                id: r.get(0)?,
                task_id: r.get(1)?,
                title: r.get(2)?,
                is_completed: r.get::<_, i64>(3)? != 0,
                sort_order: r.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    tx.commit()?;

    Ok(SyncSnapshot {
        rev,
        lists,
        recurrences,
        tasks,
        steps,
    })
}

/// 查询 `rev > since` 的增量变更，按 rev 升序。
pub fn changes_since(
    conn: &Connection,
    since: i64,
    limit: Option<i64>,
) -> AppResult<SyncChanges> {
    let limit = limit.unwrap_or(DEFAULT_CHANGE_LIMIT).max(1);
    let latest_rev = current_rev(conn)?;
    let mut stmt = conn.prepare(
        "SELECT rev, entity, entity_id, op, payload, changed_at
         FROM sync_log WHERE rev > ?1 ORDER BY rev ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![since, limit + 1], |r| {
        let payload: Option<String> = r.get(4)?;
        Ok(SyncChange {
            rev: r.get(0)?,
            entity: r.get(1)?,
            entity_id: r.get(2)?,
            op: r.get(3)?,
            payload: payload.and_then(|p| serde_json::from_str::<Value>(&p).ok()),
            changed_at: r.get(5)?,
        })
    })?;
    let mut changes: Vec<SyncChange> = rows.collect::<Result<_, _>>()?;
    let has_more = changes.len() as i64 > limit;
    if has_more {
        changes.truncate(limit as usize);
    }
    Ok(SyncChanges {
        latest_rev,
        has_more,
        changes,
    })
}
