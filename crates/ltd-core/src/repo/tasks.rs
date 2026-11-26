use std::collections::HashMap;

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate};
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};

use crate::domain::{compute_reminder_at, parse_date, Rule};
use crate::error::{err, AppResult};
use crate::models::{CalendarDay, Recurrence, Task, TaskQuery, TaskStep};

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
    /// 提醒开关。新建任务时缺省为关闭
    #[serde(default)]
    pub reminder_enabled: Option<bool>,
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
    t.deleted_at, t.reminder_enabled, t.recurrence_count
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
        reminder_enabled: r.get::<_, i64>(27)? != 0,
        reminder_at: r.get(10)?,
        recurrence_id,
        series_id: r.get(12)?,
        parent_task_id: r.get(13)?,
        recurrence_count: r.get(28)?,
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

/// 由数据库中的重复规则构造纯逻辑规则
fn recurrence_rule(rec: &Recurrence) -> Option<Rule> {
    let base = parse_date(&rec.base_date)?;
    Some(Rule::new(
        &rec.kind,
        rec.interval.max(1),
        rec.weekday_numbers(),
        rec.month_day.map(|m| m as u32),
        base,
    ))
}

/// 月历/单日视图共用的候选任务：未删除、未完成。
/// 日历只展示仍然待处理的任务，与「全部任务」保持同一口径。
fn active_tasks(conn: &Connection) -> AppResult<Vec<Task>> {
    let sql = format!(
        "{TASK_SELECT} WHERE t.deleted_at IS NULL AND t.is_completed = 0{TASK_ORDER}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_task)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 重复任务在某一天是否实际发生。
/// 只从当前待处理的周期（due_date）开始向后计算，避免把已经完成的旧周期重复展示。
fn occurs_on(task: &Task, target: NaiveDate) -> bool {
    match task.recurrence.as_ref() {
        Some(rec) => {
            let due = task.due_date.as_deref().and_then(parse_date);
            if let Some(due) = due {
                if target < due {
                    return false;
                }
            }
            recurrence_rule(rec).map(|rule| rule.matches(target)).unwrap_or(false)
        }
        None => task.due_date.as_deref().and_then(parse_date) == Some(target),
    }
}

/// 月历数据：返回 [start, end] 区间内每天的实际任务统计
pub fn calendar_month(conn: &Connection, start: &str, end: &str) -> AppResult<Vec<CalendarDay>> {
    let start_date = parse_date(start).ok_or_else(|| crate::error::AppError::Other("起始日期无效".into()))?;
    let end_date = parse_date(end).ok_or_else(|| crate::error::AppError::Other("结束日期无效".into()))?;
    if end_date < start_date {
        return err("结束日期不能早于起始日期");
    }

    let tasks = active_tasks(conn)?;
    let mut days: BTreeMap<NaiveDate, CalendarDay> = BTreeMap::new();

    for task in &tasks {
        let mut d = start_date;
        while d <= end_date {
            if occurs_on(task, d) {
                let entry = days.entry(d).or_insert_with(|| CalendarDay {
                    date: d.format("%Y-%m-%d").to_string(),
                    total: 0,
                    non_repeat: 0,
                    repeat: 0,
                    has_important: false,
                });
                entry.total += 1;
                if task.recurrence.is_some() {
                    entry.repeat += 1;
                } else {
                    entry.non_repeat += 1;
                }
                if task.is_important {
                    entry.has_important = true;
                }
            }
            d += Duration::days(1);
        }
    }

    Ok(days.into_values().collect())
}

/// 单日视图：返回某一天实际发生的任务（含完整任务数据，可直接编辑）
pub fn tasks_for_day(conn: &Connection, date: &str) -> AppResult<Vec<Task>> {
    let target = parse_date(date).ok_or_else(|| crate::error::AppError::Other("日期无效".into()))?;
    let mut matched: Vec<Task> = active_tasks(conn)?
        .into_iter()
        .filter(|task| occurs_on(task, target))
        .collect();
    load_steps(conn, &mut matched)?;
    Ok(matched)
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

/// 已触发提醒、但尚未完成的任务，按最近触发时间倒序
pub fn notified_tasks(conn: &Connection) -> AppResult<Vec<Task>> {
    let sql = format!(
        "{TASK_SELECT}
         WHERE t.deleted_at IS NULL AND t.is_completed = 0
           AND EXISTS (SELECT 1 FROM reminder_schedules s
                       WHERE s.task_id = t.id AND s.status = 'fired')
         ORDER BY (SELECT MAX(s2.trigger_at) FROM reminder_schedules s2
                   WHERE s2.task_id = t.id AND s2.status = 'fired') DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_task)?;
    let mut tasks = rows.collect::<Result<Vec<_>, _>>()?;
    load_steps(conn, &mut tasks)?;
    Ok(tasks)
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

/// 由初时间点与规则计算第 n 次发生日期（n 从 0 开始）
fn recurrence_occurrence(rec: &RecurrenceInput, base: &str, n: i64) -> Option<String> {
    let base = parse_date(base)?;
    let weekdays: Vec<u32> = rec
        .weekdays
        .split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .collect();
    let rule = Rule::new(
        &rec.kind,
        rec.interval.max(1),
        weekdays,
        rec.month_day.map(|m| m as u32),
        base,
    );
    rule.nth(n).map(|d| d.format("%Y-%m-%d").to_string())
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
    super::in_tx(conn, |conn| {
        let title = input.title.trim();
        if title.is_empty() {
            return err("任务名称不能为空");
        }
        let list_id = resolve_list(conn, input.list_id.as_deref())?;
        let id = new_id();
        let mut due_date = input.due_date.clone();
        let recurrence_id = if let Some(rec) = input.recurrence.as_ref() {
            let base = due_date.clone().unwrap_or_else(crate::domain::today_string);
            // 初时间点按规则对齐到首次发生日期
            due_date = recurrence_occurrence(rec, &base, 0);
            Some(insert_recurrence(conn, rec, &base)?)
        } else {
            None
        };
        let due_time = input.due_time.clone().filter(|t| !t.trim().is_empty());
        let reminder_enabled = input.reminder_enabled.unwrap_or(false);
        // 开启提醒时若未选择方案，默认「准时提醒」，保证开关与调度一致
        let reminder_offset = if reminder_enabled {
            Some(input.reminder_offset.unwrap_or(0))
        } else {
            input.reminder_offset
        };
        let reminder_at = if reminder_enabled {
            compute_reminder_at(due_date.as_deref(), due_time.as_deref(), reminder_offset)
        } else {
            None
        };
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
                reminder_offset, reminder_enabled, reminder_at, recurrence_id, series_id, parent_task_id, my_day,
                sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, ?13, ?14, ?15, ?16)",
            params![
                id,
                list_id,
                title,
                input.note.clone().unwrap_or_default(),
                input.is_important.unwrap_or(false) as i64,
                due_date,
                due_time,
                reminder_offset,
                reminder_enabled as i64,
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
        if let Some(rid) = recurrence_id.as_ref() {
            crate::sync::log_recurrence(conn, rid)?;
        }
        crate::sync::log_task(conn, &id)?;
        get_task(conn, &id)
    })
}

pub fn update_task(conn: &Connection, id: &str, input: &TaskInput) -> AppResult<Task> {
    super::in_tx(conn, |conn| update_task_inner(conn, id, input))
}

fn update_task_inner(conn: &Connection, id: &str, input: &TaskInput) -> AppResult<Task> {
    let existing = get_task(conn, id)?;
    let title = input.title.trim();
    if title.is_empty() {
        return err("任务名称不能为空");
    }
    let list_id = match input.list_id.as_deref() {
        Some(id) => resolve_list(conn, Some(id))?,
        None => existing.list_id.clone(),
    };
    let note = input.note.clone().unwrap_or_else(|| existing.note.clone());
    let due_time = input.due_time.clone().filter(|t| !t.trim().is_empty());
    let mut due_date = input.due_date.clone();
    let mut recurrence_count = existing.recurrence_count;
    // 规则或初时间点变化后，旧周期产生的提醒必须作废
    let mut reset_notifications = false;

    let recurrence_id = match (input.recurrence.as_ref(), existing.recurrence.as_ref()) {
        // 取消重复：保留当前显示的日期，次数清零
        (None, _) => {
            recurrence_count = 0;
            None
        }
        // 规则未变：只有初时间点变化时才重置周期
        (Some(rec), Some(old)) if rule_matches_input(old, rec) => {
            let new_base = due_date.clone().unwrap_or_else(|| old.base_date.clone());
            if new_base != old.base_date {
                conn.execute(
                    "UPDATE recurrences SET base_date=?1 WHERE id=?2",
                    params![new_base, old.id],
                )?;
                due_date = recurrence_occurrence(rec, &new_base, 0);
                recurrence_count = 0;
                reset_notifications = true;
            } else {
                due_date = existing.due_date.clone();
            }
            Some(old.id.clone())
        }
        // 新建或修改重复规则：次数重置为 0，从初时间点重新起算
        (Some(rec), _) => {
            let base = due_date.clone().unwrap_or_else(crate::domain::today_string);
            due_date = recurrence_occurrence(rec, &base, 0);
            recurrence_count = 0;
            reset_notifications = true;
            Some(insert_recurrence(conn, rec, &base)?)
        }
    };

    // 提醒开关未显式传入时保持原值，避免修改其他字段时被默认值覆盖
    let reminder_enabled = input.reminder_enabled.unwrap_or(existing.reminder_enabled);
    // 开启提醒时若未选择方案，默认「准时提醒」，保证开关与调度一致
    let reminder_offset = if reminder_enabled {
        Some(input.reminder_offset.unwrap_or(0))
    } else {
        input.reminder_offset
    };
    let reminder_at = if existing.is_completed || !reminder_enabled {
        None
    } else {
        compute_reminder_at(due_date.as_deref(), due_time.as_deref(), reminder_offset)
    };

    // 关闭提醒、修改规则 / 初时间点或提醒时刻变化时，清除旧的已触发通知
    if reset_notifications || !reminder_enabled || reminder_at != existing.reminder_at {
        clear_fired_notifications(conn, id)?;
    }

    // 重要 / 我的一天 与本次编辑的字段相互独立：未显式传入时保持数据库中的原值
    let is_important = input.is_important.unwrap_or(existing.is_important);
    let my_day = match input.my_day.clone() {
        Some(v) => Some(v),
        None => existing.my_day.clone(),
    };

    conn.execute(
        "UPDATE tasks SET title=?1, list_id=?2, note=?3, is_important=?4, due_date=?5, due_time=?6,
            reminder_offset=?7, reminder_enabled=?8, reminder_at=?9, recurrence_id=?10, my_day=?11,
            recurrence_count=?12, updated_at=?13
         WHERE id=?14",
        params![
            title,
            list_id,
            note,
            is_important as i64,
            due_date,
            due_time,
            reminder_offset,
            reminder_enabled as i64,
            reminder_at,
            recurrence_id,
            my_day,
            recurrence_count,
            now_ms(),
            id
        ],
    )?;

    if existing.is_completed {
        cancel_schedules(conn, id)?;
    } else {
        sync_schedule(conn, id, reminder_at)?;
    }
    history::record(conn, id, title, "updated", "")?;

    if let Some(rid) = recurrence_id.as_ref() {
        crate::sync::log_recurrence(conn, rid)?;
    }
    // 取消重复后旧规则不再被引用，向手机发送删除事件
    if let (Some(old), None) = (existing.recurrence_id.as_deref(), recurrence_id.as_deref()) {
        crate::sync::log_recurrence_if_orphan(conn, old)?;
    }
    crate::sync::log_task(conn, id)?;
    get_task(conn, id)
}

pub fn set_important(conn: &Connection, id: &str, important: bool) -> AppResult<Task> {
    super::in_tx(conn, |conn| {
        conn.execute(
            "UPDATE tasks SET is_important=?1, updated_at=?2 WHERE id=?3",
            params![important as i64, now_ms(), id],
        )?;
        let task = get_task(conn, id)?;
        history::record(conn, id, &task.title, if important { "important" } else { "unimportant" }, "")?;
        crate::sync::log_task(conn, id)?;
        Ok(task)
    })
}

pub fn set_my_day(conn: &Connection, id: &str, add: bool) -> AppResult<Task> {
    super::in_tx(conn, |conn| {
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
        crate::sync::log_task(conn, id)?;
        Ok(task)
    })
}

pub fn move_task(conn: &Connection, id: &str, list_id: &str) -> AppResult<Task> {
    super::in_tx(conn, |conn| {
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
        crate::sync::log_task(conn, id)?;
        Ok(task)
    })
}

/// 完成 / 取消完成任务。
/// 重复任务始终是同一条记录：
/// - 存在待处理（已触发未处理）提醒：完成当前周期，次数与下次时间不变；
/// - 不存在待处理提醒：推进到下一周期，次数 +1，任务恢复为待完成。
pub fn complete_task(conn: &Connection, id: &str, completed: bool) -> AppResult<Option<Task>> {
    super::in_tx(conn, |conn| complete_task_inner(conn, id, completed))
}

fn complete_task_inner(conn: &Connection, id: &str, completed: bool) -> AppResult<Option<Task>> {
    let task = get_task(conn, id)?;
    if completed {
        if task.recurrence_id.is_some() {
            if !task.is_completed && has_fired_notification(conn, id)? {
                clear_fired_notifications(conn, id)?;
                conn.execute(
                    "UPDATE tasks SET is_completed=1, completed_at=?1, reminder_at=NULL, updated_at=?2 WHERE id=?3",
                    params![now_ms(), now_ms(), id],
                )?;
                cancel_schedules(conn, id)?;
                history::record(conn, id, &task.title, "completed", "重复任务当前周期")?;
                crate::sync::log_task(conn, id)?;
                return Ok(None);
            }
            return advance_recurrence(conn, &task).map(Some);
        }
        if task.is_completed {
            return Ok(None);
        }
        conn.execute(
            "UPDATE tasks SET is_completed=1, completed_at=?1, reminder_at=NULL, updated_at=?2 WHERE id=?3",
            params![now_ms(), now_ms(), id],
        )?;
        cancel_schedules(conn, id)?;
        history::record(conn, id, &task.title, "completed", "")?;
        crate::sync::log_task(conn, id)?;
        Ok(None)
    } else {
        if !task.is_completed {
            return Ok(None);
        }
        conn.execute(
            "UPDATE tasks SET is_completed=0, completed_at=NULL, updated_at=?1 WHERE id=?2",
            params![now_ms(), id],
        )?;
        let at = if task.reminder_enabled {
            compute_reminder_at(
                task.due_date.as_deref(),
                task.due_time.as_deref(),
                Some(task.reminder_offset.unwrap_or(0)),
            )
        } else {
            None
        };
        // 重复任务撤销完成时，不补发已经过去的提醒
        let at = if task.recurrence_id.is_some() {
            at.filter(|ms| *ms > now_ms())
        } else {
            at
        };
        conn.execute("UPDATE tasks SET reminder_at=?1 WHERE id=?2", params![at, id])?;
        sync_schedule(conn, id, at)?;
        history::record(conn, id, &task.title, "uncompleted", "")?;
        crate::sync::log_task(conn, id)?;
        Ok(None)
    }
}

/// 该任务是否存在已触发但尚未处理的提醒
fn has_fired_notification(conn: &Connection, id: &str) -> AppResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
        [id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

fn clear_fired_notifications(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "DELETE FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
        [id],
    )?;
    Ok(())
}

/// 推进重复任务到下一周期：更新原记录，绝不复制任务
fn advance_recurrence(conn: &Connection, task: &Task) -> AppResult<Task> {
    let rec = match task.recurrence.as_ref() {
        Some(r) => r,
        None => return err("该任务不是重复任务"),
    };
    let base = match parse_date(&rec.base_date) {
        Some(d) => d,
        None => return err("重复规则基准日期无效"),
    };
    let rule = Rule::new(
        &rec.kind,
        rec.interval.max(1),
        rec.weekday_numbers(),
        rec.month_day.map(|m| m as u32),
        base,
    );
    let count = task.recurrence_count + 1;
    let next_date = match rule.nth(count) {
        Some(d) => d.format("%Y-%m-%d").to_string(),
        None => {
            // 没有下一次发生：按普通任务结束
            clear_fired_notifications(conn, &task.id)?;
            cancel_schedules(conn, &task.id)?;
            conn.execute(
                "UPDATE tasks SET is_completed=1, completed_at=?1, reminder_at=NULL, updated_at=?2 WHERE id=?3",
                params![now_ms(), now_ms(), task.id],
            )?;
            history::record(conn, &task.id, &task.title, "completed", "重复已结束")?;
            crate::sync::log_task(conn, &task.id)?;
            return get_task(conn, &task.id);
        }
    };
    clear_fired_notifications(conn, &task.id)?;
    cancel_schedules(conn, &task.id)?;
    let reminder_at = if task.reminder_enabled {
        compute_reminder_at(
            Some(&next_date),
            task.due_time.as_deref(),
            Some(task.reminder_offset.unwrap_or(0)),
        )
    } else {
        None
    };
    conn.execute(
        "UPDATE tasks SET recurrence_count=?1, due_date=?2, is_completed=0, completed_at=NULL,
            reminder_at=?3, updated_at=?4
         WHERE id=?5",
        params![count, next_date, reminder_at, now_ms(), task.id],
    )?;
    sync_schedule(conn, &task.id, reminder_at)?;
    history::record(
        conn,
        &task.id,
        &task.title,
        "recurred",
        &format!("第 {} 次 {}", count, next_date),
    )?;
    crate::sync::log_task(conn, &task.id)?;
    get_task(conn, &task.id)
}

/// 删除任务。scope = "single" 或 "series"（整个重复系列）
pub fn delete_task(conn: &Connection, id: &str, scope: &str) -> AppResult<()> {
    super::in_tx(conn, |conn| {
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
            // 任务软删除后手机端不再展示，步骤同步发送删除事件
            crate::sync::log_delete_steps_of_task(conn, tid)?;
            conn.execute("UPDATE tasks SET deleted_at=?1 WHERE id=?2", params![now_ms(), tid])?;
            cancel_schedules(conn, tid)?;
            sync_schedule(conn, tid, None)?;
            crate::sync::log_delete(conn, crate::sync::ENTITY_TASK, tid)?;
        }
        history::record(
            conn,
            id,
            &task.title,
            if scope == "series" { "deleted_series" } else { "deleted" },
            &format!("{} 个任务", ids.len()),
        )?;
        Ok(())
    })
}

pub fn restore_task(conn: &Connection, id: &str) -> AppResult<Task> {
    super::in_tx(conn, |conn| {
        conn.execute("UPDATE tasks SET deleted_at=NULL, updated_at=?1 WHERE id=?2", params![now_ms(), id])?;
        let task = get_task(conn, id)?;
        if !task.is_completed {
            let at = if task.reminder_enabled {
                compute_reminder_at(
                    task.due_date.as_deref(),
                    task.due_time.as_deref(),
                    Some(task.reminder_offset.unwrap_or(0)),
                )
            } else {
                None
            };
            conn.execute("UPDATE tasks SET reminder_at=?1 WHERE id=?2", params![at, id])?;
            sync_schedule(conn, id, at)?;
        }
        history::record(conn, id, &task.title, "restored", "")?;
        crate::sync::log_steps_of_task(conn, id)?;
        if let Some(rid) = task.recurrence_id.as_ref() {
            crate::sync::log_recurrence(conn, rid)?;
        }
        crate::sync::log_task(conn, id)?;
        Ok(task)
    })
}

pub fn purge_task(conn: &Connection, id: &str) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let recurrence_id: Option<String> = match conn.query_row(
            "SELECT recurrence_id FROM tasks WHERE id=?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        ) {
            Ok(v) => v,
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(e.into()),
        };
        crate::sync::log_delete_steps_of_task(conn, id)?;
        conn.execute("DELETE FROM tasks WHERE id=?1", [id])?;
        crate::sync::log_delete(conn, crate::sync::ENTITY_TASK, id)?;
        if let Some(rid) = recurrence_id.as_ref() {
            crate::sync::log_recurrence_if_orphan(conn, rid)?;
        }
        Ok(())
    })
}

pub fn empty_deleted(conn: &Connection) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let task_ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM tasks WHERE deleted_at IS NOT NULL")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let list_ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT id FROM lists WHERE deleted_at IS NOT NULL")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let recurrence_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT recurrence_id FROM tasks
                 WHERE deleted_at IS NOT NULL AND recurrence_id IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<_, _>>()?
        };

        conn.execute(
            "UPDATE reminder_schedules SET status='cancelled'
             WHERE task_id IN (SELECT id FROM tasks WHERE deleted_at IS NOT NULL)",
            [],
        )?;
        for tid in &task_ids {
            crate::sync::log_delete_steps_of_task(conn, tid)?;
            crate::sync::log_delete(conn, crate::sync::ENTITY_TASK, tid)?;
        }
        conn.execute("DELETE FROM tasks WHERE deleted_at IS NOT NULL", [])?;
        // 已删除清单（其中的任务已在上一步清除）
        conn.execute("DELETE FROM lists WHERE deleted_at IS NOT NULL", [])?;

        for rid in &recurrence_ids {
            crate::sync::log_recurrence_if_orphan(conn, rid)?;
        }
        for lid in &list_ids {
            crate::sync::log_delete(conn, crate::sync::ENTITY_LIST, lid)?;
        }
        Ok(())
    })
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
    super::in_tx(conn, |conn| {
        let title = title.trim();
        if title.is_empty() {
            return err("步骤内容不能为空");
        }
        let next: i64 = conn
            .query_row("SELECT IFNULL(MAX(sort_order), -1) + 1 FROM task_steps WHERE task_id=?1", [task_id], |r| r.get(0))?;
        let step_id = new_id();
        conn.execute(
            "INSERT INTO task_steps (id, task_id, title, is_completed, sort_order) VALUES (?1, ?2, ?3, 0, ?4)",
            params![step_id, task_id, title, next],
        )?;
        conn.execute("UPDATE tasks SET updated_at=?1 WHERE id=?2", params![now_ms(), task_id])?;
        crate::sync::log_step(conn, &step_id)?;
        crate::sync::log_task(conn, task_id)?;
        get_task(conn, task_id)
    })
}

pub fn update_step(conn: &Connection, step_id: &str, title: &str, completed: bool) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let task_id: String = conn.query_row("SELECT task_id FROM task_steps WHERE id=?1", [step_id], |r| r.get(0))?;
        conn.execute(
            "UPDATE task_steps SET title=?1, is_completed=?2 WHERE id=?3",
            params![title.trim(), completed as i64, step_id],
        )?;
        conn.execute("UPDATE tasks SET updated_at=?1 WHERE id=?2", params![now_ms(), task_id])?;
        crate::sync::log_step(conn, step_id)?;
        crate::sync::log_task(conn, &task_id)?;
        Ok(())
    })
}

pub fn delete_step(conn: &Connection, step_id: &str) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        let task_id: String = conn.query_row("SELECT task_id FROM task_steps WHERE id=?1", [step_id], |r| r.get(0))?;
        conn.execute("DELETE FROM task_steps WHERE id=?1", [step_id])?;
        conn.execute("UPDATE tasks SET updated_at=?1 WHERE id=?2", params![now_ms(), task_id])?;
        crate::sync::log_delete(conn, crate::sync::ENTITY_STEP, step_id)?;
        crate::sync::log_task(conn, &task_id)?;
        Ok(())
    })
}

pub fn reorder_tasks(conn: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    super::in_tx(conn, |conn| {
        for (i, id) in ordered_ids.iter().enumerate() {
            conn.execute("UPDATE tasks SET sort_order=?1 WHERE id=?2", params![i as i64, id])?;
            crate::sync::log_task(conn, id)?;
        }
        Ok(())
    })
}
