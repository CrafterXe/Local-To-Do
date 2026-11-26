//! 镜像库只读查询。
//!
//! 这些函数只读取由同步引擎写入的镜像数据，不提供任何写接口；
//! 查询逻辑复用 Windows 端 `repo` 的实现，保证两端展示口径一致。

use std::cmp::Ordering;
use std::collections::HashSet;

use chrono::NaiveDate;
use serde::Deserialize;

use crate::domain::{parse_date, today_string, Rule};
use crate::error::AppResult;
use crate::mirror::MirrorDb;
use crate::models::{CalendarDay, Recurrence, Task, TaskList, TaskQuery};

/// Android 端任务查询条件（全部为展示用途，不影响业务数据）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorTaskQuery {
    /// all | today | completed | list
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub list_id: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    /// 仅对 all / list 生效：是否包含已完成任务
    #[serde(default)]
    pub include_completed: Option<bool>,
}

impl MirrorDb {
    /// 全部未删除清单（含待办/总数统计）
    pub fn lists(&self) -> AppResult<Vec<TaskList>> {
        let conn = self.lock();
        crate::repo::lists::all_lists(&conn)
    }

    /// 单个任务详情（含步骤与重复规则）
    pub fn task(&self, id: &str) -> AppResult<Task> {
        let conn = self.lock();
        crate::repo::tasks::get_task(&conn, id)
    }

    /// 按展示条件查询任务
    pub fn tasks(&self, query: &MirrorTaskQuery) -> AppResult<Vec<Task>> {
        let conn = self.lock();
        let include_completed = query.include_completed.unwrap_or(false);
        let search = normalize_search(query.search.as_deref());
        let scope = query.scope.as_deref().unwrap_or("all");

        let mut tasks = match scope {
            "list" => crate::repo::tasks::list_tasks(
                &conn,
                &TaskQuery {
                    view: "list".into(),
                    list_id: query.list_id.clone(),
                    search,
                    include_completed: Some(include_completed),
                },
            )?,
            "completed" => crate::repo::tasks::list_tasks(
                &conn,
                &TaskQuery {
                    view: "completed".into(),
                    list_id: None,
                    search,
                    include_completed: None,
                },
            )?,
            "today" => {
                let today = today_string();
                let mut base = crate::repo::tasks::list_tasks(
                    &conn,
                    &TaskQuery {
                        view: "myDay".into(),
                        list_id: None,
                        search: search.clone(),
                        include_completed: None,
                    },
                )?;
                let mut seen: HashSet<String> = base.iter().map(|t| t.id.clone()).collect();
                for task in occurring_open(&conn, &today)? {
                    if !matches_search(&task, search.as_deref()) || !seen.insert(task.id.clone()) {
                        continue;
                    }
                    base.push(task);
                }
                for task in completed_on(&conn, &today)? {
                    if !matches_search(&task, search.as_deref()) || !seen.insert(task.id.clone()) {
                        continue;
                    }
                    base.push(task);
                }
                // 已逾期但未完成的任务单独成组，仍归入「我的今天」
                for task in overdue_open(&conn, &today)? {
                    if !matches_search(&task, search.as_deref()) || !seen.insert(task.id.clone()) {
                        continue;
                    }
                    base.push(task);
                }
                base
            }
            _ => {
                let mut base = crate::repo::tasks::list_tasks(
                    &conn,
                    &TaskQuery {
                        view: "all".into(),
                        list_id: None,
                        search: search.clone(),
                        include_completed: None,
                    },
                )?;
                if include_completed {
                    base.extend(crate::repo::tasks::list_tasks(
                        &conn,
                        &TaskQuery {
                            view: "completed".into(),
                            list_id: None,
                            search,
                            include_completed: None,
                        },
                    )?);
                }
                base
            }
        };

        tasks.sort_by(task_order);
        Ok(tasks)
    }

    /// 某一天实际发生的任务（含当天到期的已完成单次任务）
    pub fn tasks_for_day(&self, date: &str) -> AppResult<Vec<Task>> {
        let conn = self.lock();
        let mut tasks = occurring_open(&conn, date)?;
        tasks.extend(completed_on(&conn, date)?);
        tasks.sort_by(task_order);
        Ok(tasks)
    }

    /// 月历统计：只统计仍未完成的待办，与 Windows 日历口径一致
    pub fn calendar(&self, start: &str, end: &str) -> AppResult<Vec<CalendarDay>> {
        let conn = self.lock();
        crate::repo::tasks::calendar_month(&conn, start, end)
    }
}

fn normalize_search(search: Option<&str>) -> Option<String> {
    search
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 与 SQL LIKE 一致的简化匹配（只作用于标题与备注，ASCII 不区分大小写）
fn matches_search(task: &Task, search: Option<&str>) -> bool {
    match search {
        None => true,
        Some(needle) => {
            let needle = needle.to_lowercase();
            task.title.to_lowercase().contains(&needle) || task.note.to_lowercase().contains(&needle)
        }
    }
}

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

/// 重复任务在某一天是否实际发生（与 Windows 日历同一判定）
fn occurs_on(task: &Task, target: NaiveDate) -> bool {
    match task.recurrence.as_ref() {
        Some(rec) => {
            let due = task.due_date.as_deref().and_then(parse_date);
            if let Some(due) = due {
                if target < due {
                    return false;
                }
            }
            recurrence_rule(rec)
                .map(|rule| rule.matches(target))
                .unwrap_or(false)
        }
        None => task.due_date.as_deref().and_then(parse_date) == Some(target),
    }
}

/// 当天实际发生的未完成任务（含重复任务）
fn occurring_open(conn: &rusqlite::Connection, date: &str) -> AppResult<Vec<Task>> {
    let target = parse_date(date).ok_or_else(|| crate::error::AppError::Other("日期无效".into()))?;
    let open = crate::repo::tasks::list_tasks(
        conn,
        &TaskQuery {
            view: "all".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )?;
    Ok(open
        .into_iter()
        .filter(|task| occurs_on(task, target))
        .collect())
}

/// 当天到期且已完成的单次任务（重复任务完成后会推进周期，不再回看旧日期）
fn completed_on(conn: &rusqlite::Connection, date: &str) -> AppResult<Vec<Task>> {
    let done = crate::repo::tasks::list_tasks(
        conn,
        &TaskQuery {
            view: "completed".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )?;
    Ok(done
        .into_iter()
        .filter(|task| task.recurrence.is_none() && task.due_date.as_deref() == Some(date))
        .collect())
}

/// 已逾期但未完成的单次任务（重复任务按当天发生情况处理，避免长期误判逾期）
fn overdue_open(conn: &rusqlite::Connection, date: &str) -> AppResult<Vec<Task>> {
    let target = parse_date(date).ok_or_else(|| crate::error::AppError::Other("日期无效".into()))?;
    let open = crate::repo::tasks::list_tasks(
        conn,
        &TaskQuery {
            view: "all".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )?;
    Ok(open
        .into_iter()
        .filter(|task| {
            task.recurrence.is_none()
                && task
                    .due_date
                    .as_deref()
                    .and_then(parse_date)
                    .map(|due| due < target)
                    .unwrap_or(false)
        })
        .collect())
}

/// 与 Windows 任务列表相同的排序：未完成优先 → 手动顺序 → 有日期优先 → 日期 → 创建时间倒序
fn task_order(a: &Task, b: &Task) -> Ordering {
    a.is_completed
        .cmp(&b.is_completed)
        .then(a.sort_order.cmp(&b.sort_order))
        .then(a.due_date.is_none().cmp(&b.due_date.is_none()))
        .then(a.due_date.cmp(&b.due_date))
        .then(b.created_at.cmp(&a.created_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror::MirrorDb;
    use crate::sync::{ListRow, RecurrenceRow, StepRow, SyncSnapshot, TaskRow};

    fn snapshot() -> SyncSnapshot {
        let today = today_string();
        let past = (chrono::Local::now().date_naive() - chrono::Duration::days(2))
            .format("%Y-%m-%d")
            .to_string();
        SyncSnapshot {
            rev: 10,
            lists: vec![ListRow {
                id: "l1".into(),
                name: "工作".into(),
                color: "#5b8cff".into(),
                icon: String::new(),
                sort_order: 0,
                created_at: 1,
                deleted_at: None,
            }],
            recurrences: vec![RecurrenceRow {
                id: "r1".into(),
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
                base_date: "2026-01-01".into(),
                created_at: 1,
            }],
            tasks: vec![
                task("t1", "今天到期", false, Some(today.clone()), None, None),
                task("t2", "今天已完成", true, Some(today.clone()), None, None),
                task("t3", "每日重复", false, Some("2026-01-01".into()), Some("r1"), None),
                task("t4", "我的一天", false, None, None, Some(today.clone())),
                task("t5", "未安排日期", false, None, None, None),
                task("t6", "已逾期任务", false, Some(past), None, None),
            ],
            steps: vec![StepRow {
                id: "s1".into(),
                task_id: "t1".into(),
                title: "第一步".into(),
                is_completed: false,
                sort_order: 0,
            }],
        }
    }

    fn task(
        id: &str,
        title: &str,
        is_completed: bool,
        due_date: Option<String>,
        recurrence_id: Option<&str>,
        my_day: Option<String>,
    ) -> TaskRow {
        TaskRow {
            id: id.into(),
            list_id: "l1".into(),
            title: title.into(),
            note: String::new(),
            is_important: false,
            is_completed,
            completed_at: if is_completed { Some(2) } else { None },
            due_date,
            due_time: Some("09:00".into()),
            reminder_offset: None,
            reminder_enabled: false,
            reminder_at: None,
            recurrence_id: recurrence_id.map(str::to_string),
            series_id: None,
            parent_task_id: None,
            recurrence_count: 0,
            my_day,
            sort_order: 0,
            created_at: 1,
            updated_at: 1,
            deleted_at: None,
        }
    }

    fn db() -> MirrorDb {
        let db = MirrorDb::open_memory().unwrap();
        db.apply_snapshot(&snapshot(), 100).unwrap();
        db
    }

    fn ids(tasks: &[Task]) -> Vec<String> {
        let mut v: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn lists_include_counts() {
        let db = db();
        let lists = db.lists().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].open_count, 5);
        assert_eq!(lists[0].total_count, 6);
    }

    #[test]
    fn all_scope_respects_include_completed() {
        let db = db();
        let open = db
            .tasks(&MirrorTaskQuery {
                scope: Some("all".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ids(&open), vec!["t1", "t3", "t4", "t5", "t6"]);
        let all = db
            .tasks(&MirrorTaskQuery {
                scope: Some("all".into()),
                include_completed: Some(true),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ids(&all), vec!["t1", "t2", "t3", "t4", "t5", "t6"]);
    }

    #[test]
    fn today_scope_merges_my_day_due_recurrence_and_overdue() {
        let db = db();
        let today = db
            .tasks(&MirrorTaskQuery {
                scope: Some("today".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ids(&today), vec!["t1", "t2", "t3", "t4", "t6"]);
        // 已完成任务排在最后
        assert_eq!(today.last().unwrap().id, "t2");
    }

    #[test]
    fn search_filters_title_and_note() {
        let db = db();
        let found = db
            .tasks(&MirrorTaskQuery {
                scope: Some("all".into()),
                search: Some("每日".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(ids(&found), vec!["t3"]);
    }

    #[test]
    fn day_view_contains_steps_and_completed() {
        let db = db();
        let today = today_string();
        let tasks = db.tasks_for_day(&today).unwrap();
        assert_eq!(ids(&tasks), vec!["t1", "t2", "t3"]);
        let t1 = tasks.iter().find(|t| t.id == "t1").unwrap();
        assert_eq!(t1.steps.len(), 1);
        assert_eq!(t1.recurrence, None);
    }

    #[test]
    fn task_detail_returns_recurrence() {
        let db = db();
        let task = db.task("t3").unwrap();
        assert_eq!(task.title, "每日重复");
        assert_eq!(task.recurrence.as_ref().unwrap().kind, "daily");
    }

    #[test]
    fn calendar_counts_open_tasks_only() {
        let db = db();
        let today = today_string();
        let days = db.calendar(&today, &today).unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].total, 2);
        assert_eq!(days[0].non_repeat, 1);
        assert_eq!(days[0].repeat, 1);
    }

    #[test]
    fn unknown_task_is_error() {
        let db = db();
        assert!(db.task("missing").is_err());
    }
}
