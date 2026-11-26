use rusqlite::{params, Connection};

use crate::domain::compute_reminder_at;
use crate::error::{err, AppResult};
use crate::models::{DeletedList, TaskList};

use super::{history, new_id, now_ms, sync_schedule};

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
    let mut stmt = conn.prepare(&format!(
        "{LIST_SELECT} WHERE l.deleted_at IS NULL ORDER BY l.sort_order ASC, l.created_at ASC"
    ))?;
    let rows = stmt.query_map([], map_list)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_list(conn: &Connection, id: &str) -> AppResult<TaskList> {
    let mut stmt = conn.prepare(&format!(
        "{LIST_SELECT} WHERE l.id = ?1 AND l.deleted_at IS NULL"
    ))?;
    let mut rows = stmt.query_map([id], map_list)?;
    match rows.next() {
        Some(Ok(l)) => Ok(l),
        _ => err("清单不存在"),
    }
}

pub fn create_list(conn: &Connection, name: &str, color: &str, icon: &str) -> AppResult<TaskList> {
    super::in_tx(conn, |conn| {
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
        crate::sync::log_list(conn, &id)?;
        get_list(conn, &id)
    })
}

pub fn update_list(conn: &Connection, id: &str, name: &str, color: &str, icon: &str) -> AppResult<TaskList> {
    super::in_tx(conn, |conn| {
        let name = name.trim();
        if name.is_empty() {
            return err("清单名称不能为空");
        }
        conn.execute(
            "UPDATE lists SET name = ?1, color = ?2, icon = ?3 WHERE id = ?4 AND deleted_at IS NULL",
            params![name, color, icon, id],
        )?;
        crate::sync::log_list(conn, id)?;
        get_list(conn, id)
    })
}

/// 删除清单。mode = "move" 时把任务移动到 `target_list_id`；mode = "delete" 时连同任务一起软删除。
/// 清单本身始终软删除，进入「最近删除」，避免级联物理删除任务。
pub fn delete_list(
    conn: &Connection,
    id: &str,
    mode: &str,
    target_list_id: Option<&str>,
) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM lists WHERE deleted_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        if total <= 1 {
            return err("至少需要保留一个清单");
        }
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM lists WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return err("清单不存在");
        }

        let now = now_ms();
        if mode == "delete" {
            // 受影响的任务需要在删除前收集，用于同步发送删除事件
            let task_ids: Vec<String> = {
                let mut stmt = conn.prepare(
                    "SELECT id FROM tasks WHERE list_id = ?1 AND deleted_at IS NULL",
                )?;
                let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<_, _>>()?
            };
            // 取消提醒，并让任务与清单使用同一个删除时间戳，便于整组恢复
            conn.execute(
                "UPDATE reminder_schedules SET status='cancelled'
                 WHERE status='pending' AND task_id IN (SELECT id FROM tasks WHERE list_id = ?1)",
                [id],
            )?;
            for tid in &task_ids {
                crate::sync::log_delete_steps_of_task(conn, tid)?;
            }
            conn.execute(
                "UPDATE tasks SET deleted_at = ?1 WHERE list_id = ?2 AND deleted_at IS NULL",
                params![now, id],
            )?;
            for tid in &task_ids {
                crate::sync::log_delete(conn, crate::sync::ENTITY_TASK, tid)?;
            }
        } else {
            let target = match target_list_id {
                Some(t) if t != id => t.to_string(),
                _ => {
                    let mut stmt = conn
                        .prepare("SELECT id FROM lists WHERE id != ?1 AND deleted_at IS NULL ORDER BY sort_order LIMIT 1")?;
                    stmt.query_row([id], |r| r.get::<_, String>(0))?
                }
            };
            // 只同步仍在显示的任务；回收站中的任务手机端已删除，避免被重新加回
            let task_ids: Vec<String> = {
                let mut stmt = conn.prepare(
                    "SELECT id FROM tasks WHERE list_id = ?1 AND deleted_at IS NULL",
                )?;
                let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
                rows.collect::<Result<_, _>>()?
            };
            conn.execute(
                "UPDATE tasks SET list_id = ?1, updated_at = ?2 WHERE list_id = ?3",
                params![target, now, id],
            )?;
            for tid in &task_ids {
                crate::sync::log_task(conn, tid)?;
            }
        }
        conn.execute(
            "UPDATE lists SET deleted_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        crate::sync::log_delete(conn, crate::sync::ENTITY_LIST, id)?;
        Ok(())
    })
}

/// 最近删除中的清单列表
pub fn deleted_lists(conn: &Connection) -> AppResult<Vec<DeletedList>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.name, l.color, l.sort_order, l.deleted_at,
            (SELECT COUNT(*) FROM tasks t WHERE t.list_id = l.id AND t.deleted_at IS NOT NULL)
           FROM lists l WHERE l.deleted_at IS NOT NULL
          ORDER BY l.deleted_at DESC, l.sort_order ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(DeletedList {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
            sort_order: r.get(3)?,
            deleted_at: r.get(4)?,
            task_count: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 恢复清单，并恢复当初随清单一起删除的任务
pub fn restore_list(conn: &Connection, id: &str) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let deleted_at: Option<i64> = match conn.query_row(
            "SELECT deleted_at FROM lists WHERE id = ?1",
            [id],
            |r| r.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => return err("清单不存在"),
            Err(e) => return Err(e.into()),
        };
        let ts = match deleted_at {
            Some(t) => t,
            None => return err("清单未被删除"),
        };
        // 与清单同时删除的任务才随清单恢复
        let restored: Vec<(String, Option<String>)> = {
            let mut stmt = conn.prepare(
                "SELECT id, recurrence_id FROM tasks WHERE list_id = ?1 AND deleted_at = ?2",
            )?;
            let rows = stmt.query_map(params![id, ts], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let now = now_ms();
        conn.execute("UPDATE lists SET deleted_at = NULL WHERE id = ?1", [id])?;
        conn.execute(
            "UPDATE tasks SET deleted_at = NULL, updated_at = ?1 WHERE list_id = ?2 AND deleted_at = ?3",
            params![now, id, ts],
        )?;

        // 恢复的任务重新计算并注册提醒
        let mut stmt = conn.prepare(
            "SELECT id, title, is_completed, due_date, due_time, reminder_offset, reminder_enabled
               FROM tasks WHERE list_id = ?1 AND deleted_at IS NULL",
        )?;
        let rows = stmt.query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, i64>(6)? != 0,
            ))
        })?;
        let tasks: Vec<_> = rows.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for (task_id, title, completed, due_date, due_time, offset, enabled) in tasks {
            if completed {
                continue;
            }
            let at = if enabled {
                compute_reminder_at(due_date.as_deref(), due_time.as_deref(), Some(offset.unwrap_or(0)))
            } else {
                None
            };
            conn.execute(
                "UPDATE tasks SET reminder_at = ?1 WHERE id = ?2",
                params![at, task_id],
            )?;
            sync_schedule(conn, &task_id, at)?;
            history::record(conn, &task_id, &title, "restored", "恢复清单时一并恢复")?;
        }

        // 同步：清单、任务及其步骤/重复规则重新下发给手机
        crate::sync::log_list(conn, id)?;
        for (task_id, recurrence_id) in restored {
            crate::sync::log_steps_of_task(conn, &task_id)?;
            if let Some(rid) = recurrence_id.as_ref() {
                crate::sync::log_recurrence(conn, rid)?;
            }
            crate::sync::log_task(conn, &task_id)?;
        }
        Ok(())
    })
}

/// 永久删除清单（连同其任务）
pub fn purge_list(conn: &Connection, id: &str) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let deleted: Option<i64> = match conn.query_row(
            "SELECT deleted_at FROM lists WHERE id = ?1",
            [id],
            |r| r.get(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => return err("清单不存在"),
            Err(e) => return Err(e.into()),
        };
        if deleted.is_none() {
            return err("只能永久删除已删除的清单");
        }
        let task_ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM tasks WHERE list_id = ?1")?;
            let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let recurrence_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT recurrence_id FROM tasks
                 WHERE list_id = ?1 AND recurrence_id IS NOT NULL",
            )?;
            let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for tid in &task_ids {
            crate::sync::log_delete_steps_of_task(conn, tid)?;
            crate::sync::log_delete(conn, crate::sync::ENTITY_TASK, tid)?;
        }
        conn.execute(
            "UPDATE reminder_schedules SET status='cancelled'
             WHERE task_id IN (SELECT id FROM tasks WHERE list_id = ?1)",
            [id],
        )?;
        conn.execute("DELETE FROM tasks WHERE list_id = ?1", [id])?;
        conn.execute("DELETE FROM lists WHERE id = ?1", [id])?;
        for rid in &recurrence_ids {
            crate::sync::log_recurrence_if_orphan(conn, rid)?;
        }
        crate::sync::log_delete(conn, crate::sync::ENTITY_LIST, id)?;
        Ok(())
    })
}

pub fn reorder_lists(conn: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        for (i, id) in ordered_ids.iter().enumerate() {
            conn.execute("UPDATE lists SET sort_order = ?1 WHERE id = ?2", params![i as i64, id])?;
            crate::sync::log_list(conn, id)?;
        }
        Ok(())
    })
}
