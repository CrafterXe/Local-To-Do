use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::models::HistoryEntry;

use super::now_ms;

/// 记录任务操作历史
pub fn record(conn: &Connection, task_id: &str, task_title: &str, action: &str, detail: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO task_history (task_id, task_title, action, detail, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![task_id, task_title, action, detail, now_ms()],
    )?;
    // 只保留最近 3000 条
    conn.execute(
        "DELETE FROM task_history WHERE id NOT IN (SELECT id FROM task_history ORDER BY id DESC LIMIT 3000)",
        [],
    )?;
    Ok(())
}

pub fn list_history(conn: &Connection, limit: i64) -> AppResult<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, task_title, action, detail, at FROM task_history ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(HistoryEntry {
            id: r.get(0)?,
            task_id: r.get(1)?,
            task_title: r.get(2)?,
            action: r.get(3)?,
            detail: r.get(4)?,
            at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_task_history(conn: &Connection, task_id: &str) -> AppResult<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, task_title, action, detail, at FROM task_history WHERE task_id = ?1 ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([task_id], |r| {
        Ok(HistoryEntry {
            id: r.get(0)?,
            task_id: r.get(1)?,
            task_title: r.get(2)?,
            action: r.get(3)?,
            detail: r.get(4)?,
            at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}
