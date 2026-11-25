//! 核心业务逻辑集成测试（使用内存 SQLite）

use rusqlite::Connection;

use crate::db::Db;
use crate::domain;
use crate::models::TaskQuery;
use crate::repo;
use crate::repo::tasks::{RecurrenceInput, TaskInput};

fn setup() -> Db {
    Db::open_memory().expect("内存数据库初始化失败")
}

fn default_list_id(conn: &Connection) -> String {
    repo::all_lists(conn).unwrap()[0].id.clone()
}

fn make_input(conn: &Connection, title: &str) -> TaskInput {
    TaskInput {
        title: title.into(),
        list_id: Some(default_list_id(conn)),
        note: None,
        is_important: None,
        due_date: None,
        due_time: None,
        reminder_offset: None,
        reminder_enabled: None,
        my_day: None,
        recurrence: None,
    }
}

fn pending_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM reminder_schedules WHERE status='pending'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn create_update_and_delete_task() {
    let db = setup();
    let conn = db.lock();

    let input = TaskInput {
        note: Some("备注内容".into()),
        due_date: Some("2026-12-01".into()),
        due_time: Some("09:30".into()),
        is_important: Some(true),
        ..make_input(&conn, "写周报")
    };
    let task = repo::create_task(&conn, &input).unwrap();
    assert_eq!(task.title, "写周报");
    assert!(task.is_important);
    assert_eq!(task.note, "备注内容");

    let list = repo::list_tasks(
        &conn,
        &TaskQuery {
            view: "all".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )
    .unwrap();
    assert_eq!(list.len(), 1);

    let updated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            title: "写月报".into(),
            ..input.clone()
        },
    )
    .unwrap();
    assert_eq!(updated.title, "写月报");
    assert_eq!(updated.note, "备注内容");

    repo::delete_task(&conn, &task.id, "single").unwrap();
    let list = repo::list_tasks(
        &conn,
        &TaskQuery {
            view: "all".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )
    .unwrap();
    assert!(list.is_empty());

    let deleted = repo::list_tasks(
        &conn,
        &TaskQuery {
            view: "deleted".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )
    .unwrap();
    assert_eq!(deleted.len(), 1);

    repo::restore_task(&conn, &task.id).unwrap();
    let restored = repo::get_task(&conn, &task.id).unwrap();
    assert_eq!(restored.title, "写月报");
}

#[test]
fn important_and_completion_toggle() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(&conn, &make_input(&conn, "买菜")).unwrap();

    let t = repo::set_important(&conn, &task.id, true).unwrap();
    assert!(t.is_important);
    let counts = repo::sidebar_counts(&conn).unwrap();
    assert_eq!(counts.important, 1);

    repo::complete_task(&conn, &task.id, true).unwrap();
    let t = repo::get_task(&conn, &task.id).unwrap();
    assert!(t.is_completed);
    let counts = repo::sidebar_counts(&conn).unwrap();
    assert_eq!(counts.important, 0);
    assert_eq!(counts.completed, 1);

    repo::complete_task(&conn, &task.id, false).unwrap();
    let t = repo::get_task(&conn, &task.id).unwrap();
    assert!(!t.is_completed);
}

#[test]
fn reminder_schedule_created_updated_and_cancelled() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-01".into()),
            due_time: Some("20:30".into()),
            reminder_offset: Some(10),
            reminder_enabled: Some(true),
            ..make_input(&conn, "提醒测试")
        },
    )
    .unwrap();
    assert!(task.reminder_at.is_some());
    assert_eq!(pending_count(&conn), 1);

    let updated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-02".into()),
            due_time: Some("08:00".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "提醒测试")
        },
    )
    .unwrap();
    assert_ne!(task.reminder_at, updated.reminder_at);
    assert_eq!(pending_count(&conn), 1);

    repo::delete_task(&conn, &task.id, "single").unwrap();
    assert_eq!(pending_count(&conn), 0);
}

#[test]
fn recurring_task_advances_cycle_without_duplicating() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-01-31".into()),
            due_time: Some("09:00".into()),
            recurrence: Some(RecurrenceInput {
                kind: "monthly".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: Some(31),
            }),
            ..make_input(&conn, "月度对账")
        },
    )
    .unwrap();
    assert_eq!(task.due_date.as_deref(), Some("2026-01-31"));
    assert_eq!(task.recurrence_count, 0);

    // 无待处理提醒 → 推进周期，仍是同一条记录
    let first = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(first.id, task.id);
    assert_eq!(first.recurrence_count, 1);
    assert_eq!(first.due_date.as_deref(), Some("2026-02-28"));
    assert!(!first.is_completed);

    let second = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(second.recurrence_count, 2);
    assert_eq!(second.due_date.as_deref(), Some("2026-03-31"));

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 1, "重复任务不应复制出新任务");
}

#[test]
fn weekly_recurrence_uses_weekday_rule() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-11-17".into()),
            recurrence: Some(RecurrenceInput {
                kind: "weekly".into(),
                interval: 1,
                weekdays: "1,3,5".into(),
                month_day: None,
            }),
            ..make_input(&conn, "健身")
        },
    )
    .unwrap();
    // 2026-11-17 是周二 → 首次发生对齐到周三 11-18
    assert_eq!(task.due_date.as_deref(), Some("2026-11-18"));
    let t1 = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(t1.due_date.as_deref(), Some("2026-11-20"));
    let t2 = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(t2.due_date.as_deref(), Some("2026-11-23"));
}

#[test]
fn completing_recurring_with_fired_notice_finishes_current_cycle() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            recurrence: Some(RecurrenceInput {
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
            }),
            ..make_input(&conn, "晨会")
        },
    )
    .unwrap();
    insert_fired(&conn, &task.id);

    // 情况 A：存在待处理提醒 → 完成当前周期，次数与时间不变
    assert!(repo::complete_task(&conn, &task.id, true).unwrap().is_none());
    let done = repo::get_task(&conn, &task.id).unwrap();
    assert!(done.is_completed);
    assert_eq!(done.recurrence_count, 0);
    assert_eq!(done.due_date.as_deref(), Some("2026-12-09"));
    let fired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
            [&task.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fired, 0, "完成后应清除该任务的通知");

    // 情况 B：再次点击已完成任务 → 推进到下一周期
    let next = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(next.id, task.id);
    assert_eq!(next.recurrence_count, 1);
    assert_eq!(next.due_date.as_deref(), Some("2026-12-10"));
    assert!(!next.is_completed);
}

#[test]
fn changing_rule_resets_repeat_count_and_recomputes() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            recurrence: Some(RecurrenceInput {
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
            }),
            ..make_input(&conn, "巡检")
        },
    )
    .unwrap();
    let advanced = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(advanced.recurrence_count, 1);
    assert_eq!(advanced.due_date.as_deref(), Some("2026-12-10"));
    // 旧规则已触发但未处理的通知
    insert_fired(&conn, &task.id);

    let updated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            recurrence: Some(RecurrenceInput {
                kind: "weekly".into(),
                interval: 1,
                weekdays: "1".into(),
                month_day: None,
            }),
            ..make_input(&conn, "巡检")
        },
    )
    .unwrap();
    assert_eq!(updated.recurrence_count, 0);
    assert_eq!(updated.due_date.as_deref(), Some("2026-12-14"));
    // 修改规则后，旧规则产生的通知应被清除
    let fired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
            [&task.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fired, 0);
}

#[test]
fn changing_initial_date_recomputes_next_occurrence() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            recurrence: Some(RecurrenceInput {
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
            }),
            ..make_input(&conn, "阅读")
        },
    )
    .unwrap();
    repo::complete_task(&conn, &task.id, true).unwrap();

    let updated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-20".into()),
            recurrence: Some(RecurrenceInput {
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
            }),
            ..make_input(&conn, "阅读")
        },
    )
    .unwrap();
    assert_eq!(updated.recurrence_count, 0);
    assert_eq!(updated.due_date.as_deref(), Some("2026-12-20"));
}

#[test]
fn reminder_switch_keeps_scheme_and_schedule() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            ..make_input(&conn, "提醒开关")
        },
    )
    .unwrap();
    assert!(!task.reminder_enabled);
    assert!(task.reminder_at.is_none());

    // 开启提醒 → 按方案建立调度
    let on = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(60),
            reminder_enabled: Some(true),
            ..make_input(&conn, "提醒开关")
        },
    )
    .unwrap();
    assert!(on.reminder_enabled);
    assert!(on.reminder_at.is_some());
    assert_eq!(pending_count(&conn), 1);

    // 关闭提醒 → 取消调度但保留方案
    let off = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(60),
            reminder_enabled: Some(false),
            ..make_input(&conn, "提醒开关")
        },
    )
    .unwrap();
    assert!(!off.reminder_enabled);
    assert!(off.reminder_at.is_none());
    assert_eq!(off.reminder_offset, Some(60));
    assert_eq!(pending_count(&conn), 0);

    // 重新开启 → 使用原方案恢复调度
    let again = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(60),
            reminder_enabled: Some(true),
            ..make_input(&conn, "提醒开关")
        },
    )
    .unwrap();
    assert!(again.reminder_enabled);
    assert_eq!(again.reminder_at, on.reminder_at);
    assert_eq!(pending_count(&conn), 1);
}

#[test]
fn enabling_reminder_without_scheme_defaults_to_on_time() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            ..make_input(&conn, "无方案")
        },
    )
    .unwrap();
    assert!(task.reminder_offset.is_none());

    // 打开开关但不指定方案 → 默认准时提醒并建立调度
    let on = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: None,
            reminder_enabled: Some(true),
            ..make_input(&conn, "无方案")
        },
    )
    .unwrap();
    assert_eq!(on.reminder_offset, Some(0));
    let expected = domain::compute_reminder_at(Some("2026-12-09"), Some("09:00"), Some(0));
    assert_eq!(on.reminder_at, expected);
    assert_eq!(pending_count(&conn), 1);

    // 关闭开关时清除已触发通知，任务从提醒栏移除
    insert_fired(&conn, &task.id);
    let off = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(false),
            ..make_input(&conn, "无方案")
        },
    )
    .unwrap();
    assert!(!off.reminder_enabled);
    assert!(off.reminder_at.is_none());
    let fired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
            [&task.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fired, 0);
}

fn insert_fired(conn: &Connection, task_id: &str) {
    let now = domain::local_now_ms();
    conn.execute(
        "INSERT INTO reminder_schedules (id, task_id, trigger_at, status, fired_at, created_at)
         VALUES (?1, ?2, ?3, 'fired', ?4, ?4)",
        rusqlite::params![format!("fired-{task_id}"), task_id, now - 1000, now],
    )
    .unwrap();
}

#[test]
fn notes_crud_and_search() {
    let db = setup();
    let conn = db.lock();
    let note = repo::create_note(&conn, "读书笔记", "今天读了《数学分析》").unwrap();
    assert_eq!(repo::list_notes(&conn, None).unwrap().len(), 1);
    assert_eq!(repo::list_notes(&conn, Some("数学")).unwrap().len(), 1);
    assert_eq!(repo::list_notes(&conn, Some("不存在")).unwrap().len(), 0);

    let updated = repo::update_note(&conn, &note.id, "读书笔记", "补充内容").unwrap();
    assert_eq!(updated.body, "补充内容");

    repo::delete_note(&conn, &note.id).unwrap();
    assert_eq!(repo::list_notes(&conn, None).unwrap().len(), 0);
}

#[test]
fn list_deletion_moves_tasks() {
    let db = setup();
    let conn = db.lock();
    let extra = repo::create_list(&conn, "工作", "#ff6600", "").unwrap();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(extra.id.clone()),
            ..make_input(&conn, "周会")
        },
    )
    .unwrap();

    repo::delete_list(&conn, &extra.id, "move", None).unwrap();
    let moved = repo::get_task(&conn, &task.id).unwrap();
    assert_ne!(moved.list_id, extra.id);
    let lists = repo::all_lists(&conn).unwrap();
    assert_eq!(lists.len(), 1);
}

#[test]
fn backup_export_and_restore_roundtrip() {
    let db = setup();
    let conn = db.lock();
    repo::create_note(&conn, "笔记", "内容").unwrap();
    repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-05".into()),
            due_time: Some("10:00".into()),
            reminder_offset: Some(5),
            reminder_enabled: Some(true),
            ..make_input(&conn, "备份任务")
        },
    )
    .unwrap();

    let dir = std::env::temp_dir().join(format!("local_todo_test_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let json_path = dir.join("backup.json");
    crate::backup::export_json(&conn, &json_path).unwrap();
    assert!(json_path.exists());
    let csv_path = dir.join("tasks.csv");
    crate::backup::export_csv(&conn, &csv_path).unwrap();
    let csv_text = std::fs::read_to_string(&csv_path).unwrap();
    assert!(csv_text.contains("备份任务"));

    // 清空数据后恢复
    conn.execute("DELETE FROM tasks", []).unwrap();
    conn.execute("DELETE FROM notes", []).unwrap();
    assert_eq!(repo::list_notes(&conn, None).unwrap().len(), 0);
    drop(conn);

    let mut conn2 = db.lock();
    let report = crate::backup::restore_from_json(&mut conn2, &json_path).unwrap();
    assert_eq!(report.tasks, 1);
    assert_eq!(report.notes, 1);
    assert_eq!(repo::list_notes(&conn2, None).unwrap().len(), 1);
    assert_eq!(pending_count(&conn2), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn settings_roundtrip() {
    let db = setup();
    let conn = db.lock();
    let mut settings = crate::db::load_settings(&conn).unwrap();
    assert_eq!(settings.theme, "dark");
    settings.theme = "light".into();
    settings.notifications_enabled = false;
    settings.auto_backup_interval_hours = 6;
    crate::db::save_settings(&conn, &settings).unwrap();

    let loaded = crate::db::load_settings(&conn).unwrap();
    assert_eq!(loaded.theme, "light");
    assert!(!loaded.notifications_enabled);
    assert_eq!(loaded.auto_backup_interval_hours, 6);
}

#[test]
fn reminder_computation_matches_due_time() {
    let ms = domain::compute_reminder_at(Some("2026-12-01"), Some("20:30"), Some(15)).unwrap();
    let due = domain::compute_reminder_at(Some("2026-12-01"), Some("20:30"), Some(0)).unwrap();
    assert_eq!(due - ms, 15 * 60_000);
}

#[test]
fn due_reminders_returns_pending_once() {
    let db = setup();
    let conn = db.lock();
    let now = domain::local_now_ms();
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some(date),
            due_time: Some("23:59".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "到点提醒")
        },
    )
    .unwrap();
    // 把计划改成已过期，模拟睡眠唤醒后的补触发
    conn.execute(
        "UPDATE reminder_schedules SET trigger_at=?1 WHERE task_id=?2 AND status='pending'",
        rusqlite::params![now - 60_000, task.id],
    )
    .unwrap();
    let due = repo::due_reminders(&conn, now).unwrap();
    assert_eq!(due.len(), 1);
    repo::mark_schedule(&conn, &due[0].schedule_id, "fired", now).unwrap();
    assert_eq!(repo::due_reminders(&conn, now).unwrap().len(), 0);
}

#[test]
fn migration_from_v1_adds_list_deleted_at() {
    let dir = std::env::temp_dir().join(format!("local_todo_mig_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("old.db");
    {
        let db = Db::open(&path).unwrap();
        let conn = db.lock();
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_lists_deleted;
             ALTER TABLE lists DROP COLUMN deleted_at;
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let db = Db::open(&path).unwrap();
    let conn = db.lock();
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 4);
    let has_col: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('lists') WHERE name='deleted_at'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(has_col, 1);
    assert_eq!(repo::all_lists(&conn).unwrap().len(), 1);
    drop(conn);
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deleted_list_is_restorable_with_its_tasks() {
    let db = setup();
    let conn = db.lock();
    let extra = repo::create_list(&conn, "项目 A", "#00a884", "").unwrap();
    let t1 = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(extra.id.clone()),
            due_date: Some("2026-12-01".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "任务一")
        },
    )
    .unwrap();
    let t2 = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(extra.id.clone()),
            ..make_input(&conn, "任务二")
        },
    )
    .unwrap();
    repo::complete_task(&conn, &t2.id, true).unwrap();

    repo::delete_list(&conn, &extra.id, "delete", None).unwrap();

    // 清单进入最近删除，清单内任务不出现在任务的最近删除列表中
    let deleted_lists = repo::deleted_lists(&conn).unwrap();
    assert_eq!(deleted_lists.len(), 1);
    assert_eq!(deleted_lists[0].name, "项目 A");
    assert_eq!(deleted_lists[0].task_count, 2);
    let deleted_tasks = repo::list_tasks(
        &conn,
        &TaskQuery {
            view: "deleted".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )
    .unwrap();
    assert!(deleted_tasks.is_empty());

    // 恢复清单后，任务连同完成状态、提醒和时间一起恢复
    repo::restore_list(&conn, &extra.id).unwrap();
    assert!(repo::deleted_lists(&conn).unwrap().is_empty());
    let restored = repo::get_task(&conn, &t1.id).unwrap();
    assert_eq!(restored.list_id, extra.id);
    assert_eq!(restored.due_date.as_deref(), Some("2026-12-01"));
    assert!(restored.reminder_at.is_some());
    assert_eq!(pending_count(&conn), 1);
    let restored2 = repo::get_task(&conn, &t2.id).unwrap();
    assert!(restored2.is_completed);
}

#[test]
fn deleted_list_move_mode_keeps_tasks_active() {
    let db = setup();
    let conn = db.lock();
    let extra = repo::create_list(&conn, "临时", "#e0a03a", "").unwrap();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(extra.id.clone()),
            ..make_input(&conn, "待办")
        },
    )
    .unwrap();
    repo::delete_list(&conn, &extra.id, "move", None).unwrap();

    assert_eq!(repo::deleted_lists(&conn).unwrap().len(), 1);
    let moved = repo::get_task(&conn, &task.id).unwrap();
    assert_ne!(moved.list_id, extra.id);

    repo::restore_list(&conn, &extra.id).unwrap();
    assert_eq!(repo::all_lists(&conn).unwrap().len(), 2);
    // 移动出去的任务不会被错误地“恢复”回来
    assert_ne!(repo::get_task(&conn, &task.id).unwrap().list_id, extra.id);
}

#[test]
fn purge_list_and_empty_deleted_clear_everything() {
    let db = setup();
    let conn = db.lock();
    let extra = repo::create_list(&conn, "待清理", "#e05252", "").unwrap();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(extra.id.clone()),
            due_date: Some("2026-12-01".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "将被清除")
        },
    )
    .unwrap();
    let single = repo::create_task(&conn, &make_input(&conn, "单独删除")).unwrap();

    repo::delete_list(&conn, &extra.id, "delete", None).unwrap();
    repo::delete_task(&conn, &single.id, "single").unwrap();
    assert_eq!(pending_count(&conn), 0);

    repo::purge_list(&conn, &extra.id).unwrap();
    assert!(repo::deleted_lists(&conn).unwrap().is_empty());
    assert!(repo::get_task(&conn, &task.id).is_err());
    assert_eq!(repo::deleted_count(&conn).unwrap(), 1);

    repo::empty_deleted(&conn).unwrap();
    assert_eq!(repo::deleted_count(&conn).unwrap(), 0);
    assert!(repo::get_task(&conn, &single.id).is_err());
}

#[test]
fn custom_reminder_offset_recomputes_after_due_change() {
    let db = setup();
    let conn = db.lock();
    // 自定义：提前 1 天 3 小时 15 分钟 = 1635 分钟
    let offset = 1 * 1440 + 3 * 60 + 15;
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-10".into()),
            due_time: Some("18:00".into()),
            reminder_offset: Some(offset),
            reminder_enabled: Some(true),
            ..make_input(&conn, "自定义提醒")
        },
    )
    .unwrap();
    let due = domain::compute_reminder_at(Some("2026-12-10"), Some("18:00"), Some(0)).unwrap();
    assert_eq!(task.reminder_at, Some(due - offset * 60_000));

    let updated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-20".into()),
            due_time: Some("18:00".into()),
            reminder_offset: Some(offset),
            reminder_enabled: Some(true),
            ..make_input(&conn, "自定义提醒")
        },
    )
    .unwrap();
    let due2 = domain::compute_reminder_at(Some("2026-12-20"), Some("18:00"), Some(0)).unwrap();
    assert_eq!(updated.reminder_at, Some(due2 - offset * 60_000));
    assert_eq!(pending_count(&conn), 1);
}

#[test]
fn sound_settings_roundtrip() {
    let db = setup();
    let conn = db.lock();
    let mut settings = crate::db::load_settings(&conn).unwrap();
    assert!(settings.sound_enabled);
    assert!(settings.sound_path.is_none());

    settings.sound_enabled = false;
    settings.sound_path = Some("C:\\sounds\\ding.wav".into());
    crate::db::save_settings(&conn, &settings).unwrap();

    let loaded = crate::db::load_settings(&conn).unwrap();
    assert!(!loaded.sound_enabled);
    assert_eq!(loaded.sound_path.as_deref(), Some("C:\\sounds\\ding.wav"));
}

#[test]
fn deleting_task_records_deleted_at_for_recent_view() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(&conn, &make_input(&conn, "删除时间")).unwrap();
    repo::delete_task(&conn, &task.id, "single").unwrap();
    let deleted = repo::list_tasks(
        &conn,
        &TaskQuery {
            view: "deleted".into(),
            list_id: None,
            search: None,
            include_completed: None,
        },
    )
    .unwrap();
    assert_eq!(deleted.len(), 1);
    assert!(deleted[0].deleted_at.is_some());
    assert_eq!(repo::deleted_count(&conn).unwrap(), 1);
}

#[test]
fn fired_reminder_is_not_recreated_after_task_update() {
    let db = setup();
    let conn = db.lock();
    let now = domain::local_now_ms();
    let past = chrono::Local::now() - chrono::Duration::minutes(10);
    let date = past.format("%Y-%m-%d").to_string();
    let time = past.format("%H:%M").to_string();
    let input = TaskInput {
        due_date: Some(date),
        due_time: Some(time),
        reminder_offset: Some(0),
        reminder_enabled: Some(true),
        ..make_input(&conn, "重复提醒检查")
    };
    let task = repo::create_task(&conn, &input).unwrap();
    let due = repo::due_reminders(&conn, now).unwrap();
    assert_eq!(due.len(), 1);
    repo::mark_schedule(&conn, &due[0].schedule_id, "fired", now).unwrap();

    // 再次保存任务（相同时间）不应重新创建已触发过的提醒
    repo::update_task(&conn, &task.id, &input).unwrap();
    assert_eq!(pending_count(&conn), 0);

    // 取消完成等场景同样不应重复创建
    repo::complete_task(&conn, &task.id, true).unwrap();
    repo::complete_task(&conn, &task.id, false).unwrap();
    assert_eq!(pending_count(&conn), 0);
    assert_eq!(repo::due_reminders(&conn, now).unwrap().len(), 0);
}

#[test]
fn calendar_expands_recurring_task_by_rule() {
    let db = setup();
    let conn = db.lock();
    repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-01".into()),
            recurrence: Some(RecurrenceInput {
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
            }),
            ..make_input(&conn, "每天的任务")
        },
    )
    .unwrap();

    // 月历网格通常从 11 月底延伸到次年 1 月初
    let days = repo::calendar_month(&conn, "2026-11-30", "2027-01-10").unwrap();
    let on = |date: &str| days.iter().find(|d| d.date == date).cloned();

    // 当前待处理周期之前不展示，周期内每天都算一次发生
    assert!(on("2026-11-30").is_none());
    let dec1 = on("2026-12-01").expect("12-01 应有任务");
    assert_eq!(dec1.total, 1);
    assert_eq!(dec1.repeat, 1);
    assert_eq!(dec1.non_repeat, 0);
    assert_eq!(on("2026-12-31").unwrap().total, 1);

    // 单日视图只返回当天实际发生的任务
    assert_eq!(repo::tasks_for_day(&conn, "2026-11-30").unwrap().len(), 0);
    assert_eq!(repo::tasks_for_day(&conn, "2026-12-15").unwrap().len(), 1);
}

#[test]
fn calendar_only_shows_tasks_with_due_date_and_marks_important() {
    let db = setup();
    let conn = db.lock();
    let important = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-08".into()),
            is_important: Some(true),
            ..make_input(&conn, "重要任务")
        },
    )
    .unwrap();
    repo::create_task(&conn, &make_input(&conn, "没有日期的任务")).unwrap();

    let days = repo::calendar_month(&conn, "2026-12-01", "2026-12-31").unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].date, "2026-12-08");
    assert_eq!(days[0].non_repeat, 1);
    assert!(days[0].has_important);

    let day = repo::tasks_for_day(&conn, "2026-12-08").unwrap();
    assert_eq!(day.len(), 1);
    assert_eq!(day[0].id, important.id);
    assert!(repo::tasks_for_day(&conn, "2026-12-09").unwrap().is_empty());
}

#[test]
fn editing_fields_keeps_important_and_my_day() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(&conn, &make_input(&conn, "独立性检查")).unwrap();
    repo::set_important(&conn, &task.id, true).unwrap();
    repo::set_my_day(&conn, &task.id, true).unwrap();

    // 只改到期日期与提醒方案：重要 / 我的一天 保持不变
    let updated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-31".into()),
            due_time: Some("18:00".into()),
            reminder_offset: Some(20),
            reminder_enabled: Some(true),
            ..make_input(&conn, "独立性检查")
        },
    )
    .unwrap();
    assert!(updated.is_important);
    assert!(updated.my_day.is_some());

    // 只改所属清单：同样保持不变
    let extra = repo::create_list(&conn, "另一个清单", "#00a884", "").unwrap();
    let moved = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            list_id: Some(extra.id.clone()),
            ..make_input(&conn, "独立性检查")
        },
    )
    .unwrap();
    assert!(moved.is_important);
    assert!(moved.my_day.is_some());
    assert_eq!(moved.list_id, extra.id);
}

#[test]
fn new_task_reminder_is_disabled_by_default() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-12-01".into()),
            due_time: Some("20:00".into()),
            reminder_offset: Some(30),
            ..make_input(&conn, "默认关闭提醒")
        },
    )
    .unwrap();
    assert!(!task.reminder_enabled);
    assert!(task.reminder_at.is_none());
    assert_eq!(pending_count(&conn), 0);

    // 开启后按已有方案计算并注册调度
    let on = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: task.due_date.clone(),
            due_time: task.due_time.clone(),
            reminder_offset: task.reminder_offset,
            reminder_enabled: Some(true),
            ..make_input(&conn, "默认关闭提醒")
        },
    )
    .unwrap();
    assert!(on.reminder_enabled);
    assert!(on.reminder_at.is_some());
    assert_eq!(pending_count(&conn), 1);

    // 关闭后取消调度，但保留提醒方案
    let off = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: task.due_date.clone(),
            due_time: task.due_time.clone(),
            reminder_offset: task.reminder_offset,
            reminder_enabled: Some(false),
            ..make_input(&conn, "默认关闭提醒")
        },
    )
    .unwrap();
    assert!(!off.reminder_enabled);
    assert_eq!(off.reminder_offset, Some(30));
    assert!(off.reminder_at.is_none());
    assert_eq!(pending_count(&conn), 0);
}

#[test]
fn notified_tasks_list_clears_after_completion() {
    let db = setup();
    let conn = db.lock();
    let now = domain::local_now_ms();
    let past = chrono::Local::now() - chrono::Duration::minutes(5);
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some(past.format("%Y-%m-%d").to_string()),
            due_time: Some(past.format("%H:%M").to_string()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "提醒栏任务")
        },
    )
    .unwrap();
    assert!(repo::notified_tasks(&conn).unwrap().is_empty());

    let due = repo::due_reminders(&conn, now).unwrap();
    repo::mark_schedule(&conn, &due[0].schedule_id, "fired", now).unwrap();
    let notified = repo::notified_tasks(&conn).unwrap();
    assert_eq!(notified.len(), 1);
    assert_eq!(notified[0].id, task.id);

    // 完成后从提醒栏移除
    repo::complete_task(&conn, &task.id, true).unwrap();
    assert!(repo::notified_tasks(&conn).unwrap().is_empty());
}

#[test]
fn reset_settings_keeps_tasks_and_lists() {
    let db = setup();
    let conn = db.lock();
    let list = repo::create_list(&conn, "项目", "#ff6600", "").unwrap();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(list.id.clone()),
            ..make_input(&conn, "保留任务")
        },
    )
    .unwrap();

    let mut s = crate::db::load_settings(&conn).unwrap();
    s.theme = "light".into();
    s.sound_path = Some("C:\\ding.wav".into());
    s.timezone_auto = false;
    s.timezone = Some("Asia/Tokyo".into());
    crate::db::save_settings(&conn, &s).unwrap();

    let reset = crate::db::reset_settings(&conn).unwrap();
    assert_eq!(reset.theme, crate::models::AppSettings::default().theme);
    assert!(reset.timezone_auto);
    assert!(reset.timezone.is_none());

    // 用户数据不受影响
    assert_eq!(repo::all_lists(&conn).unwrap().len(), 2);
    assert_eq!(repo::get_task(&conn, &task.id).unwrap().title, "保留任务");
}
