use rusqlite::{params, Connection};

use crate::error::{err, AppResult};
use crate::models::TaskList;

use super::{new_id, now_ms};

const LIST_SELECT: &str = "SELECT l.id, l.name, l.color, l.icon, l.sort_order, l.created_at,
    (SELECT COUNT(*) FROM tasks t WHERE t.list_id = l.id AND t.deleted_at IS NULL AND t.is_completed = 0),
    (SELECT COUNT(*) FROM tasks t WHERE t.list_id = l.id AND t.deleted_at IS NULL)
  FROM lists l";

fn map_list(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskList> {
    Ok(TaskList {
        id: r.get(0)?,
        name: r.get(1)?,
        color: r.get(2)?,
        icon: r.get(3)?,
        sort_order: r.get(4)?,
        created_at: r.get(5)?,
        open_count: r.get(6)?,
        total_count: r.get(7)?,
    })
}

pub fn all_lists(conn: &Connection) -> AppResult<Vec<TaskList>> {
    let mut stmt = conn.prepare(&format!("{LIST_SELECT} ORDER BY l.sort_order ASC, l.created_at ASC"))?;
    let rows = stmt.query_map([], map_list)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_list(conn: &Connection, id: &str) -> AppResult<TaskList> {
    let mut stmt = conn.prepare(&format!("{LIST_SELECT} WHERE l.id = ?1"))?;
    let mut rows = stmt.query_map([id], map_list)?;
    match rows.next() {
        Some(Ok(l)) => Ok(l),
        _ => err("清单不存在"),
    }
}

pub fn create_list(conn: &Connection, name: &str, color: &str, icon: &str) -> AppResult<TaskList> {
    let name = name.trim();
    if name.is_empty() {
        return err("清单名称不能为空");
    }
    let next: i64 = conn.query_row("SELECT IFNULL(MAX(sort_order), -1) + 1 FROM lists", [], |r| r.get(0))?;
    let id = new_id();
    conn.execute(
        "INSERT INTO lists (id, name, color, icon, sort_order, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, name, color, icon, next, now_ms()],
    )?;
    get_list(conn, &id)
}

pub fn update_list(conn: &Connection, id: &str, name: &str, color: &str, icon: &str) -> AppResult<TaskList> {
    let name = name.trim();
    if name.is_empty() {
        return err("清单名称不能为空");
    }
    conn.execute(
        "UPDATE lists SET name = ?1, color = ?2, icon = ?3 WHERE id = ?4",
        params![name, color, icon, id],
    )?;
    get_list(conn, id)
}

/// 删除清单。mode = "move" 时把任务移动到 `target_list_id`；mode = "delete" 时软删除任务。
pub fn delete_list(
    conn: &Connection,
    id: &str,
    mode: &str,
    target_list_id: Option<&str>,
) -> AppResult<()> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM lists", [], |r| r.get(0))?;
    if total <= 1 {
        return err("至少需要保留一个清单");
    }
    let exists: i64 = conn.query_row("SELECT COUNT(*) FROM lists WHERE id = ?1", [id], |r| r.get(0))?;
    if exists == 0 {
        return err("清单不存在");
    }

    if mode == "delete" {
        // 软删除该清单下的所有任务，并取消提醒
        conn.execute(
            "UPDATE reminder_schedules SET status='cancelled'
             WHERE status='pending' AND task_id IN (SELECT id FROM tasks WHERE list_id = ?1)",
            [id],
        )?;
        conn.execute(
            "UPDATE tasks SET deleted_at = ?1 WHERE list_id = ?2 AND deleted_at IS NULL",
            params![now_ms(), id],
        )?;
    } else {
        let target = match target_list_id {
            Some(t) if t != id => t.to_string(),
            _ => {
                let mut stmt = conn.prepare("SELECT id FROM lists WHERE id != ?1 ORDER BY sort_order LIMIT 1")?;
                stmt.query_row([id], |r| r.get::<_, String>(0))?
            }
        };
        conn.execute(
            "UPDATE tasks SET list_id = ?1, updated_at = ?2 WHERE list_id = ?3",
            params![target, now_ms(), id],
        )?;
    }
    conn.execute("DELETE FROM lists WHERE id = ?1", [id])?;
    Ok(())
}

pub fn reorder_lists(conn: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    for (i, id) in ordered_ids.iter().enumerate() {
        conn.execute("UPDATE lists SET sort_order = ?1 WHERE id = ?2", params![i as i64, id])?;
    }
    Ok(())
}
