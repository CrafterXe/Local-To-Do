use std::collections::HashMap;

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};

use crate::domain::{compute_reminder_at, parse_date, Rule};
use crate::error::{err, AppResult};
use crate::models::{Recurrence, Task, TaskQuery, TaskStep};

use super::{cancel_schedules, history, new_id, now_ms, sync_schedule};

/// 新建/更新任务的输入。字段为 None 表示清除该属性。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub list_id: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub is_important: Option<bool>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub due_time: Option<String>,
    #[serde(default)]
    pub reminder_offset: Option<i64>,
    #[serde(default)]
    pub my_day: Option<String>,
    #[serde(default)]
    pub recurrence: Option<RecurrenceInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceInput {
    pub kind: String,
    #[serde(default = "one")]
    pub interval: i64,
    #[serde(default)]
    pub weekdays: String,
    #[serde(default)]
    pub month_day: Option<i64>,
}

fn one() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarCounts {
    pub my_day: i64,
    pub important: i64,
    pub planned: i64,
    pub all_open: i64,
    pub completed: i64,
    pub deleted: i64,
    pub notes: i64,
}

const TASK_SELECT: &str = "SELECT t.id, t.list_id, t.title, t.note, t.is_important, t.is_completed,
    t.completed_at, t.due_date, t.due_time, t.reminder_offset, t.reminder_at, t.recurrence_id,
    t.series_id, t.parent_task_id, t.my_day, t.sort_order, t.created_at, t.updated_at,
    IFNULL(l.name, '任务'), IFNULL(l.color, '#2f6fed'),
    r.kind, r.interval, r.weekdays, r.month_day, r.base_date, r.created_at,
    t.deleted_at
  FROM tasks t
  LEFT JOIN lists l ON l.id = t.list_id
  LEFT JOIN recurrences r ON r.id = t.recurrence_id";

const TASK_ORDER: &str = " ORDER BY t.is_completed ASC, t.sort_order ASC,
    CASE WHEN t.due_date IS NULL THEN 1 ELSE 0 END ASC, t.due_date ASC, t.created_at DESC";

fn map_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let recurrence_id: Option<String> = r.get(11)?;
    let rec_kind: Option<String> = r.get(20)?;
    let recurrence = match (recurrence_id.clone(), rec_kind) {
        (Some(id), Some(kind)) => Some(Recurrence {
            id,
            kind,
            interval: r.get(21)?,
            weekdays: r.get(22)?,
            month_day: r.get(23)?,
            base_date: r.get(24)?,
            created_at: r.get(25)?,
        }),
        _ => None,
    };
    Ok(Task {
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
        reminder_at: r.get(10)?,
        recurrence_id,
        series_id: r.get(12)?,
        parent_task_id: r.get(13)?,
        my_day: r.get(14)?,
        sort_order: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
        deleted_at: r.get(26)?,
        list_name: r.get(18)?,
        list_color: r.get(19)?,
        steps: Vec::new(),
        recurrence,
    })
}

fn load_steps(conn: &Connection, tasks: &mut [Task]) -> AppResult<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, task_id, title, is_completed, sort_order FROM task_steps WHERE task_id IN ({placeholders}) ORDER BY sort_order ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(ids.iter()), |r| {
        Ok(TaskStep {
            id: r.get(0)?,
            task_id: r.get(1)?,
            title: r.get(2)?,
            is_completed: r.get::<_, i64>(3)? != 0,
            sort_order: r.get(4)?,
        })
    })?;
    let mut map: HashMap<String, Vec<TaskStep>> = HashMap::new();
    for s in rows {
        let s = s?;
        map.entry(s.task_id.clone()).or_default().push(s);
    }
    for t in tasks.iter_mut() {
        t.steps = map.remove(&t.id).unwrap_or_default();
    }
    Ok(())
}

pub fn list_tasks(conn: &Connection, q: &TaskQuery) -> AppResult<Vec<Task>> {
    let today = crate::domain::today_string();
    let mut conds: Vec<String> = Vec::new();
    let mut vals: Vec<Value> = Vec::new();

    if q.view == "deleted" {
        conds.push("t.deleted_at IS NOT NULL".into());
        // 随清单一起删除的任务只在清单条目中展示，避免重复恢复
        conds.push("l.deleted_at IS NULL".into());
    } else {
        conds.push("t.deleted_at IS NULL".into());
        match q.view.as_str() {
            "myDay" => {
                conds.push("t.my_day = ?".into());
                vals.push(Value::Text(today));
                conds.push("t.is_completed = 0".into());
            }
            "important" => {
                conds.push("t.is_important = 1".into());
                conds.push("t.is_completed = 0".into());
            }
            "planned" => {
                conds.push("t.due_date IS NOT NULL".into());
                conds.push("t.is_completed = 0".into());
            }
            "all" => conds.push("t.is_completed = 0".into()),
            "completed" => conds.push("t.is_completed = 1".into()),
            "list" => {
                conds.push("t.list_id = ?".into());
                vals.push(Value::Text(q.list_id.clone().unwrap_or_default()));
                if !q.include_completed.unwrap_or(false) {
                    conds.push("t.is_completed = 0".into());
                }
            }
            _ => conds.push("t.is_completed = 0".into()),
        }
    }

    if let Some(s) = q.search.as_ref().map(|s| s.trim().to_string()) {
        if !s.is_empty() {
            conds.push("(t.title LIKE ? OR t.note LIKE ?)".into());
            let like = format!("%{s}%");
            vals.push(Value::Text(like.clone()));
            vals.push(Value::Text(like));
        }
    }

    let sql = format!("{TASK_SELECT} WHERE {}", conds.join(" AND "));
    let order = if q.view == "deleted" {
        " ORDER BY t.deleted_at DESC"
    } else {
        TASK_ORDER
    };
    let sql = format!("{sql}{order}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(vals.iter()), map_task)?;
    let mut tasks = rows.collect::<Result<Vec<_>, _>>()?;
    load_steps(conn, &mut tasks)?;
    Ok(tasks)
}

pub fn get_task(conn: &Connection, id: &str) -> AppResult<Task> {
    let sql = format!("{TASK_SELECT} WHERE t.id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map([id], map_task)?;
    let mut task = match rows.next() {
        Some(Ok(t)) => t,
        _ => return err("任务不存在"),
    };
    drop(rows);
    drop(stmt);
    let mut one = vec![task.clone()];
    load_steps(conn, &mut one)?;
    task.steps = one.remove(0).steps;
    Ok(task)
}

fn resolve_list(conn: &Connection, list_id: Option<&str>) -> AppResult<String> {
    if let Some(id) = list_id {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM lists WHERE id=?1 AND deleted_at IS NULL",
            [id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Ok(id.to_string());
        }
    }
    let settings = crate::db::load_settings(conn)?;
    if let Some(id) = settings.default_list_id {
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM lists WHERE id=?1 AND deleted_at IS NULL",
            [&id],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Ok(id);
        }
    }
    let first: Option<String> = conn
        .query_row(
            "SELECT id FROM lists WHERE deleted_at IS NULL ORDER BY sort_order LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    first.ok_or_else(|| crate::error::AppError::Other("没有可用清单".into()))
}

fn rule_matches_input(rec: &Recurrence, input: &RecurrenceInput) -> bool {
    rec.kind == input.kind.to_lowercase()
        && rec.interval == input.interval.max(1)
        && rec.weekdays == normalize_weekdays(&input.weekdays)
        && rec.month_day == input.month_day
}

fn normalize_weekdays(s: &str) -> String {
    let mut v: Vec<i64> = s
        .split(',')
        .filter_map(|x| x.trim().parse::<i64>().ok())
        .filter(|n| (1..=7).contains(n))
        .collect();
    v.sort_unstable();
    v.dedup();
    v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")
}

fn insert_recurrence(conn: &Connection, input: &RecurrenceInput, base_date: &str) -> AppResult<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO recurrences (id, kind, interval, weekdays, month_day, base_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            input.kind.to_lowercase(),
            input.interval.max(1),
            normalize_weekdays(&input.weekdays),
            input.month_day,
            base_date,
            now_ms()
        ],
    )?;
    Ok(id)
}

pub fn create_task(conn: &Connection, input: &TaskInput) -> AppResult<Task> {
    let title = input.title.trim();
    if title.is_empty() {
        return err("任务名称不能为空");
    }
    let list_id = resolve_list(conn, input.list_id.as_deref())?;
    let id = new_id();
    let mut due_date = input.due_date.clone();
    let recurrence_id = if let Some(rec) = input.recurrence.as_ref() {
        if due_date.is_none() {
            due_date = Some(crate::domain::today_string());
        }
        Some(insert_recurrence(conn, rec, due_date.as_deref().unwrap_or(""))?)
    } else {
        None
    };
    let due_time = input.due_time.clone().filter(|t| !t.trim().is_empty());
    let reminder_offset = if input.reminder_offset.is_some() { input.reminder_offset } else { None };
    let reminder_at = compute_reminder_at(due_date.as_deref(), due_time.as_deref(), reminder_offset);
    let series_id = if recurrence_id.is_some() {
        Some(id.clone())
    } else {
        None
    };
    let next: Option<i64> = conn
        .query_row("SELECT MAX(sort_order) FROM tasks WHERE list_id=?1", [&list_id], |r| r.get(0))
        .ok();
    conn.execute(
        "INSERT INTO tasks (id, list_id, title, note, is_important, is_completed, due_date, due_time,
            reminder_offset, reminder_at, recurrence_id, series_id, parent_task_id, my_day,
            sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12, ?13, ?14, ?15)",
        params![
            id,
            list_id,
            title,
            input.note.clone().unwrap_or_default(),
            input.is_important.unwrap_or(false) as i64,
            due_date,
            due_time,
            reminder_offset,
            reminder_at,
            recurrence_id,
            series_id,
            input.my_day.clone(),
            next.unwrap_or(0) + 1,
            now_ms(),
            now_ms()
        ],
    )?;
    sync_schedule(conn, &id, reminder_at)?;
    history::record(conn, &id, title, "created", "")?;
    get_task(conn, &id)
}

pub fn update_task(conn: &Connection, id: &str, input: &TaskInput) -> AppResult<Task> {
    let existing = get_task(conn, id)?;
    let title = input.title.trim();
    if title.is_empty() {
        return err("任务名称不能为空");
    }
    let list_id = resolve_list(conn, input.list_id.as_deref())?;
    let due_date = input.due_date.clone();
    let due_time = input.due_time.clone().filter(|t| !t.trim().is_empty());

    let recurrence_id = match (input.recurrence.as_ref(), existing.recurrence.as_ref()) {
        (None, _) => None,
        (Some(rec), Some(old)) if rule_matches_input(old, rec) => Some(old.id.clone()),
        (Some(rec), _) => {
            let base = due_date.clone().unwrap_or_else(crate::domain::today_string);
            Some(insert_recurrence(conn, rec, &base)?)
        }
    };
    let recurrence_changed = recurrence_id != existing.recurrence_id;

    let reminder_offset = input.reminder_offset;
    let reminder_at = if existing.is_completed {
        None
    } else {
        compute_reminder_at(due_date.as_deref(), due_time.as_deref(), reminder_offset)
    };

    conn.execute(
        "UPDATE tasks SET title=?1, list_id=?2, note=?3, is_important=?4, due_date=?5, due_time=?6,
            reminder_offset=?7, reminder_at=?8, recurrence_id=?9, my_day=?10, updated_at=?11
         WHERE id=?12",
        params![
            title,
            list_id,
            input.note.clone().unwrap_or_default(),
            input.is_important.unwrap_or(false) as i64,
            due_date,
            due_time,
            reminder_offset,
            reminder_at,
            recurrence_id,
            input.my_day.clone(),
            now_ms(),
            id
        ],
    )?;

    // 修改重复规则时，同步修正后续尚未完成的任务所使用的规则
    if recurrence_changed {
        if let (Some(new_rec), Some(series)) = (recurrence_id.as_ref(), existing.series_id.as_ref()) {
            conn.execute(
                "UPDATE tasks SET recurrence_id=?1 WHERE series_id=?2 AND id != ?3
                 AND is_completed=0 AND deleted_at IS NULL",
                params![new_rec, series, id],
            )?;
        }
    }

    if existing.is_completed {
        cancel_schedules(conn, id)?;
    } else {
        sync_schedule(conn, id, reminder_at)?;
    }
    history::record(conn, id, title, "updated", "")?;
    get_task(conn, id)
}

pub fn set_important(conn: &Connection, id: &str, important: bool) -> AppResult<Task> {
    conn.execute(
        "UPDATE tasks SET is_important=?1, updated_at=?2 WHERE id=?3",
        params![important as i64, now_ms(), id],
    )?;
    let task = get_task(conn, id)?;
    history::record(conn, id, &task.title, if important { "important" } else { "unimportant" }, "")?;
    Ok(task)
}

pub fn set_my_day(conn: &Connection, id: &str, add: bool) -> AppResult<Task> {
    let value: Option<String> = if add {
        Some(crate::domain::today_string())
    } else {
        None
    };
    conn.execute(
        "UPDATE tasks SET my_day=?1, updated_at=?2 WHERE id=?3",
        params![value, now_ms(), id],
    )?;
    let task = get_task(conn, id)?;
    history::record(conn, id, &task.title, if add { "my_day" } else { "my_day_removed" }, "")?;
    Ok(task)
}

pub fn move_task(conn: &Connection, id: &str, list_id: &str) -> AppResult<Task> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM lists WHERE id=?1 AND deleted_at IS NULL",
        [list_id],
        |r| r.get(0),
    )?;
    if exists == 0 {
        return err("目标清单不存在");
    }
    conn.execute(
        "UPDATE tasks SET list_id=?1, updated_at=?2 WHERE id=?3",
        params![list_id, now_ms(), id],
    )?;
    let task = get_task(conn, id)?;
    history::record(conn, id, &task.title, "moved", &task.list_name)?;
    Ok(task)
}

/// 完成 / 取消完成任务。返回自动生成的下一个周期任务（如有）。
pub fn complete_task(conn: &Connection, id: &str, completed: bool) -> AppResult<Option<Task>> {
    let task = get_task(conn, id)?;
    if completed {
        if task.is_completed {
            return Ok(None);
        }
        conn.execute(
            "UPDATE tasks SET is_completed=1, completed_at=?1, reminder_at=NULL, updated_at=?2 WHERE id=?3",
            params![now_ms(), now_ms(), id],
        )?;
        cancel_schedules(conn, id)?;
        // 重复任务：生成下一个周期
        let next = if task.recurrence_id.is_some() {
            generate_next(conn, &task)?
        } else {
            None
        };
        history::record(
            conn,
            id,
            &task.title,
            "completed",
            next.as_ref().map(|t| format!("下一周期: {}", t.due_date.clone().unwrap_or_default())).unwrap_or_default().as_str(),
        )?;
        Ok(next)
    } else {
        if !task.is_completed {
            return Ok(None);
        }
        conn.execute(
            "UPDATE tasks SET is_completed=0, completed_at=NULL, updated_at=?1 WHERE id=?2",
            params![now_ms(), id],
        )?;
        // 撤销完成时，删除自动生成的下一周期任务
        let mut stmt = conn.prepare(
            "SELECT id FROM tasks WHERE parent_task_id=?1 AND is_completed=0 AND deleted_at IS NULL",
        )?;
        let children: Vec<String> = stmt
            .query_map([id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for child in children {
            conn.execute("UPDATE tasks SET deleted_at=?1 WHERE id=?2", params![now_ms(), child])?;
            cancel_schedules(conn, &child)?;
            sync_schedule(conn, &child, None)?;
        }
        let at = compute_reminder_at(task.due_date.as_deref(), task.due_time.as_deref(), task.reminder_offset);
        conn.execute("UPDATE tasks SET reminder_at=?1 WHERE id=?2", params![at, id])?;
        sync_schedule(conn, id, at)?;
        history::record(conn, id, &task.title, "uncompleted", "")?;
        Ok(None)
    }
}

fn generate_next(conn: &Connection, task: &Task) -> AppResult<Option<Task>> {
    let rec_id = match task.recurrence_id.as_ref() {
        Some(r) => r,
        None => return Ok(None),
    };
    let mut stmt = conn.prepare(
        "SELECT kind, interval, weekdays, month_day, base_date FROM recurrences WHERE id=?1",
    )?;
    let rec: Result<(String, i64, String, Option<i64>, String), rusqlite::Error> = stmt.query_row(
        [rec_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    );
    drop(stmt);
    let (kind, interval, weekdays, month_day, base_date) = rec?;
    let base = match parse_date(&base_date) {
        Some(d) => d,
        None => return err("重复规则基准日期无效"),
    };
    let after = task
        .due_date
        .as_deref()
        .and_then(parse_date)
        .unwrap_or_else(crate::domain::reminder::now_date);
    let weekdays_vec: Vec<u32> = weekdays
        .split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .collect();
    let rule = Rule::new(
        &kind,
        interval,
        weekdays_vec,
        month_day.map(|m| m as u32),
        base,
    );
    let next_date = match rule.next_after(after) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => return Ok(None),
    };

    // 去重：同一父任务只生成一次
    let dup: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE parent_task_id=?1 AND deleted_at IS NULL",
        [&task.id],
        |r| r.get(0),
    )?;
    if dup > 0 {
        return Ok(None);
    }

    let id = new_id();
    let reminder_at = compute_reminder_at(
        Some(&next_date),
        task.due_time.as_deref(),
        task.reminder_offset,
    );
    let series_id = task.series_id.clone().unwrap_or_else(|| task.id.clone());
    conn.execute(
        "INSERT INTO tasks (id, list_id, title, note, is_important, is_completed, due_date, due_time,
            reminder_offset, reminder_at, recurrence_id, series_id, parent_task_id, my_day,
            sort_order, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, ?13, ?14, ?15)",
        params![
            id,
            task.list_id,
            task.title,
            task.note,
            task.is_important as i64,
            next_date,
            task.due_time,
            task.reminder_offset,
            reminder_at,
            rec_id,
            series_id,
            task.id,
            task.sort_order,
            now_ms(),
            now_ms()
        ],
    )?;
    // 复制步骤（未完成状态）
    for s in &task.steps {
        conn.execute(
            "INSERT INTO task_steps (id, task_id, title, is_completed, sort_order) VALUES (?1, ?2, ?3, 0, ?4)",
            params![new_id(), id, s.title, s.sort_order],
        )?;
    }
    sync_schedule(conn, &id, reminder_at)?;
    history::record(conn, &id, &task.title, "recurrence_created", &next_date)?;
    get_task(conn, &id).map(Some)
}

/// 跳过本次重复：把到期时间推进到下一个周期
pub fn skip_occurrence(conn: &Connection, id: &str) -> AppResult<Task> {
    let task = get_task(conn, id)?;
    let rec = match task.recurrence.as_ref() {
        Some(r) => r,
        None => return err("该任务不是重复任务"),
    };
    let base = match parse_date(&rec.base_date) {
        Some(d) => d,
        None => return err("重复规则基准日期无效"),
    };
    let after = task
        .due_date
        .as_deref()
        .and_then(parse_date)
        .unwrap_or_else(crate::domain::reminder::now_date);
    let weekdays: Vec<u32> = rec.weekday_numbers();
    let rule = Rule::new(
        &rec.kind,
        rec.interval,
        weekdays,
        rec.month_day.map(|m| m as u32),
        base,
    );
    let next_date = match rule.next_after(after) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => return err("无法计算下一次重复日期"),
    };
    let reminder_at = compute_reminder_at(Some(&next_date), task.due_time.as_deref(), task.reminder_offset);
    conn.execute(
        "UPDATE tasks SET due_date=?1, reminder_at=?2, updated_at=?3 WHERE id=?4",
        params![next_date, reminder_at, now_ms(), id],
    )?;
    sync_schedule(conn, id, reminder_at)?;
    history::record(conn, id, &task.title, "skipped", &next_date)?;
    get_task(conn, id)
}

/// 删除任务。scope = "single" 或 "series"（整个重复系列）
pub fn delete_task(conn: &Connection, id: &str, scope: &str) -> AppResult<()> {
    let task = get_task(conn, id)?;
    let ids: Vec<String> = if scope == "series" {
        let series = task.series_id.clone().unwrap_or_else(|| task.id.clone());
        let mut stmt = conn.prepare(
            "SELECT id FROM tasks WHERE (series_id=?1 OR id=?2) AND deleted_at IS NULL",
        )?;
        let v = stmt
            .query_map(params![series, task.id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        v
    } else {
        vec![task.id.clone()]
    };
    for tid in &ids {
        conn.execute("UPDATE tasks SET deleted_at=?1 WHERE id=?2", params![now_ms(), tid])?;
        cancel_schedules(conn, tid)?;
        sync_schedule(conn, tid, None)?;
    }
    history::record(
        conn,
        id,
        &task.title,
        if scope == "series" { "deleted_series" } else { "deleted" },
        &format!("{} 个任务", ids.len()),
    )?;
    Ok(())
}

pub fn restore_task(conn: &Connection, id: &str) -> AppResult<Task> {
    conn.execute("UPDATE tasks SET deleted_at=NULL, updated_at=?1 WHERE id=?2", params![now_ms(), id])?;
    let task = get_task(conn, id)?;
    if !task.is_completed {
        let at = compute_reminder_at(task.due_date.as_deref(), task.due_time.as_deref(), task.reminder_offset);
        conn.execute("UPDATE tasks SET reminder_at=?1 WHERE id=?2", params![at, id])?;
        sync_schedule(conn, id, at)?;
    }
    history::record(conn, id, &task.title, "restored", "")?;
    Ok(task)
}

pub fn purge_task(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute("DELETE FROM tasks WHERE id=?1", [id])?;
    Ok(())
}

pub fn empty_deleted(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "UPDATE reminder_schedules SET status='cancelled'
         WHERE task_id IN (SELECT id FROM tasks WHERE deleted_at IS NOT NULL)",
        [],
    )?;
    conn.execute("DELETE FROM tasks WHERE deleted_at IS NOT NULL", [])?;
    // 已删除清单（其中的任务已在上一步清除）
    conn.execute("DELETE FROM lists WHERE deleted_at IS NOT NULL", [])?;
    Ok(())
}

pub fn deleted_count(conn: &Connection) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT (SELECT COUNT(*) FROM tasks t WHERE t.deleted_at IS NOT NULL
                    AND (t.list_id IS NULL OR t.list_id NOT IN (SELECT id FROM lists WHERE deleted_at IS NOT NULL)))
                + (SELECT COUNT(*) FROM lists WHERE deleted_at IS NOT NULL)",
        [],
        |r| r.get(0),
    )?)
}

pub fn sidebar_counts(conn: &Connection) -> AppResult<SidebarCounts> {
    let today = crate::domain::today_string();
    let count = |sql: &str, p: &[&dyn rusqlite::ToSql]| -> AppResult<i64> {
        Ok(conn.query_row(sql, p, |r| r.get(0))?)
    };
    Ok(SidebarCounts {
        my_day: count(
            "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND is_completed=0 AND my_day=?1",
            &[&today],
        )?,
        important: count(
            "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND is_completed=0 AND is_important=1",
            &[],
        )?,
        planned: count(
            "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND is_completed=0 AND due_date IS NOT NULL",
            &[],
        )?,
        all_open: count(
            "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND is_completed=0",
            &[],
        )?,
        completed: count(
            "SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL AND is_completed=1",
            &[],
        )?,
        deleted: deleted_count(conn)?,
        notes: count("SELECT COUNT(*) FROM notes WHERE deleted_at IS NULL", &[])?,
    })
}

pub fn next_trigger(conn: &Connection) -> AppResult<Option<i64>> {
    let v: Option<i64> = conn
        .query_row(
            "SELECT MIN(trigger_at) FROM reminder_schedules WHERE status='pending'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(None);
    Ok(v)
}

// ---------- 步骤 ----------

pub fn add_step(conn: &Connection, task_id: &str, title: &str) -> AppResult<Task> {
    let title = title.trim();
    if title.is_empty() {
        return err("步骤内容不能为空");
    }
    let next: i64 = conn
        .query_row("SELECT IFNULL(MAX(sort_order), -1) + 1 FROM task_steps WHERE task_id=?1", [task_id], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO task_steps (id, task_id, title, is_completed, sort_order) VALUES (?1, ?2, ?3, 0, ?4)",
        params![new_id(), task_id, title, next],
    )?;
    conn.execute("UPDATE tasks SET updated_at=?1 WHERE id=?2", params![now_ms(), task_id])?;
    get_task(conn, task_id)
}

pub fn update_step(conn: &Connection, step_id: &str, title: &str, completed: bool) -> AppResult<()> {
    let task_id: String = conn.query_row("SELECT task_id FROM task_steps WHERE id=?1", [step_id], |r| r.get(0))?;
    conn.execute(
        "UPDATE task_steps SET title=?1, is_completed=?2 WHERE id=?3",
        params![title.trim(), completed as i64, step_id],
    )?;
    conn.execute("UPDATE tasks SET updated_at=?1 WHERE id=?2", params![now_ms(), task_id])?;
    Ok(())
}

pub fn delete_step(conn: &Connection, step_id: &str) -> AppResult<()> {
    let task_id: String = conn.query_row("SELECT task_id FROM task_steps WHERE id=?1", [step_id], |r| r.get(0))?;
    conn.execute("DELETE FROM task_steps WHERE id=?1", [step_id])?;
    conn.execute("UPDATE tasks SET updated_at=?1 WHERE id=?2", params![now_ms(), task_id])?;
    Ok(())
}

pub fn reorder_tasks(conn: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    for (i, id) in ordered_ids.iter().enumerate() {
        conn.execute("UPDATE tasks SET sort_order=?1 WHERE id=?2", params![i as i64, id])?;
    }
    Ok(())
}
