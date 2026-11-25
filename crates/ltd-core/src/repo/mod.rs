pub mod history;
pub mod lists;
pub mod notes;
pub mod tasks;

pub use history::{list_history, list_task_history};
pub use lists::*;
pub use notes::*;
pub use tasks::*;

use rusqlite::Connection;

use crate::error::AppResult;
use crate::models::PendingReminder;

pub fn now_ms() -> i64 {
    crate::domain::local_now_ms()
}

/// 在单个事务中执行写入与同步记录：任一步失败则整体回滚，
/// 保证业务数据与 sync_log 不会出现一半成功的情况。
pub(crate) fn in_tx<T>(
    conn: &Connection,
    f: impl FnOnce(&Connection) -> AppResult<T>,
) -> AppResult<T> {
    let tx = conn.unchecked_transaction()?;
    let out = f(&tx)?;
    tx.commit()?;
    Ok(out)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 同步某任务的提醒计划：只保留一条 pending 记录。
/// 若同一任务、同一触发时间已经触发过，则不重复创建，避免修改任务后重复弹窗。
pub fn sync_schedule(conn: &Connection, task_id: &str, trigger_at: Option<i64>) -> AppResult<()> {
    conn.execute(
        "UPDATE reminder_schedules SET status='cancelled' WHERE task_id=?1 AND status='pending'",
        [task_id],
    )?;
    if let Some(at) = trigger_at {
        let already_fired: i64 = conn.query_row(
            "SELECT COUNT(*) FROM reminder_schedules
             WHERE task_id=?1 AND trigger_at=?2 AND status='fired'",
            rusqlite::params![task_id, at],
            |r| r.get(0),
        )?;
        if already_fired > 0 {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO reminder_schedules (id, task_id, trigger_at, status, created_at)
             VALUES (?1, ?2, ?3, 'pending', ?4)",
            rusqlite::params![new_id(), task_id, at, now_ms()],
        )?;
    }
    Ok(())
}

/// 取消所有提醒计划（删除任务时）
pub fn cancel_schedules(conn: &Connection, task_id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE reminder_schedules SET status='cancelled' WHERE task_id=?1 AND status='pending'",
        [task_id],
    )?;
    Ok(())
}

/// 查询所有到期的待触发提醒
pub fn due_reminders(conn: &Connection, now: i64) -> AppResult<Vec<PendingReminder>> {
    let mut stmt = conn.prepare(
        "SELECT rs.id, rs.task_id, rs.trigger_at, t.title, t.due_date, t.due_time,
                t.is_completed, t.deleted_at, t.reminder_at, IFNULL(l.name, '任务')
         FROM reminder_schedules rs
         JOIN tasks t ON t.id = rs.task_id
         LEFT JOIN lists l ON l.id = t.list_id
         WHERE rs.status = 'pending' AND rs.trigger_at <= ?1
           AND IFNULL(t.reminder_enabled, 1) = 1
         ORDER BY rs.trigger_at ASC",
    )?;
    let rows = stmt.query_map([now], |r| {
        let is_completed: i64 = r.get(6)?;
        let deleted: Option<i64> = r.get(7)?;
        let reminder_at: Option<i64> = r.get(8)?;
        let trigger_at: i64 = r.get(2)?;
        Ok(PendingReminder {
            schedule_id: r.get(0)?,
            task_id: r.get(1)?,
            trigger_at,
            title: r.get(3)?,
            due_date: r.get(4)?,
            due_time: r.get(5)?,
            list_name: r.get(9)?,
            is_completed: is_completed != 0,
            is_deleted: deleted.is_some(),
            reminder_changed: reminder_at != Some(trigger_at),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 把某条提醒计划标记为已触发 / 错过 / 取消
pub fn mark_schedule(conn: &Connection, schedule_id: &str, status: &str, fired_at: i64) -> AppResult<()> {
    conn.execute(
        "UPDATE reminder_schedules SET status=?1, fired_at=?2 WHERE id=?3",
        rusqlite::params![status, fired_at, schedule_id],
    )?;
    Ok(())
}

/// 清理 30 天前的已处理提醒记录，避免表无限增长
pub fn prune_schedules(conn: &Connection) -> AppResult<()> {
    let cutoff = now_ms() - 30i64 * 24 * 3600 * 1000;
    conn.execute(
        "DELETE FROM reminder_schedules WHERE status != 'pending' AND IFNULL(fired_at, created_at) < ?1",
        [cutoff],
    )?;
    Ok(())
}
