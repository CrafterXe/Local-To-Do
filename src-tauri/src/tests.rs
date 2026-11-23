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
fn completing_recurring_task_generates_next_occurrence() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-01-31".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(0),
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
    assert!(task.recurrence_id.is_some());
    assert!(task.series_id.is_some());

    let next = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(next.due_date.as_deref(), Some("2026-02-28"));
    assert!(!next.is_completed);
    assert_eq!(next.parent_task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(next.recurrence_id, task.recurrence_id.clone());

    // 撤销完成会移除自动生成的下一周期
    repo::complete_task(&conn, &task.id, false).unwrap();
    let children: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE parent_task_id=?1 AND deleted_at IS NULL",
            [&task.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(children, 0);
}

#[test]
fn skip_occurrence_moves_due_date_forward() {
    let db = setup();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-11-18".into()),
            due_time: Some("20:30".into()),
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
    let skipped = repo::skip_occurrence(&conn, &task.id).unwrap();
    assert_eq!(skipped.due_date.as_deref(), Some("2026-11-20"));
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
    assert_eq!(version, 2);
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
