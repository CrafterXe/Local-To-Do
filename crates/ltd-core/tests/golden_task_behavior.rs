use std::sync::Mutex;

use ltd_core::db::Db;
use ltd_core::domain;
use ltd_core::repo;
use ltd_core::repo::tasks::{RecurrenceInput, TaskInput};
use rusqlite::Connection;

static TZ_TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_utc() -> std::sync::MutexGuard<'static, ()> {
    let guard = TZ_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    domain::set_timezone(Some("UTC".into()));
    guard
}

fn make_input(conn: &Connection, title: &str) -> TaskInput {
    TaskInput {
        title: title.into(),
        list_id: Some(repo::all_lists(conn).unwrap()[0].id.clone()),
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

fn monthly_31() -> Option<RecurrenceInput> {
    Some(RecurrenceInput {
        kind: "monthly".into(),
        interval: 1,
        weekdays: String::new(),
        month_day: Some(31),
    })
}

fn weekly_mwf() -> Option<RecurrenceInput> {
    Some(RecurrenceInput {
        kind: "weekly".into(),
        interval: 1,
        weekdays: "1,3,5".into(),
        month_day: None,
    })
}

fn insert_fired(conn: &Connection, task_id: &str) {
    conn.execute(
        "INSERT INTO reminder_schedules (id, task_id, trigger_at, status, fired_at, created_at)
         VALUES (?1, ?2, 1, 'fired', 1, 1)",
        [format!("fired-{task_id}"), task_id.to_string()],
    )
    .unwrap();
}

#[test]
fn completing_recurrence_advances_one_row_and_rebuilds_next_reminder() {
    let _timezone = lock_utc();
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2024-01-31".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(60),
            reminder_enabled: Some(true),
            recurrence: monthly_31(),
            ..make_input(&conn, "月末对账")
        },
    )
    .unwrap();
    let first_pending: String = conn
        .query_row(
            "SELECT id FROM reminder_schedules WHERE task_id=?1 AND status='pending'",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();

    let february = repo::complete_task(&conn, &task.id, true)
        .unwrap()
        .expect("无 fired 提醒时直接推进下一周期");
    assert_eq!(february.id, task.id);
    assert_eq!(february.recurrence_count, 1);
    assert_eq!(february.due_date.as_deref(), Some("2024-02-29"));
    assert!(!february.is_completed);
    let expected = domain::compute_reminder_at_in_timezone(
        Some("2024-02-29"),
        Some("09:00"),
        Some(60),
        chrono_tz::UTC,
    );
    assert_eq!(february.reminder_at, expected);
    let schedule: (String, i64) = conn
        .query_row(
            "SELECT status, trigger_at FROM reminder_schedules WHERE id=?1",
            [first_pending],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(schedule.0, "cancelled");
    let pending: (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), MIN(trigger_at) FROM reminder_schedules
             WHERE task_id=?1 AND status='pending'",
            [&task.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(pending, (1, expected.unwrap()));

    let march = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(march.recurrence_count, 2);
    assert_eq!(march.due_date.as_deref(), Some("2024-03-31"));
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1, "推进重复任务不得复制业务记录");
}

#[test]
fn fired_notice_requires_second_completion_to_advance_recurrence() {
    let _timezone = lock_utc();
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2099-12-09".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            recurrence: Some(RecurrenceInput {
                kind: "daily".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: None,
            }),
            ..make_input(&conn, "每日巡检")
        },
    )
    .unwrap();
    insert_fired(&conn, &task.id);

    assert!(repo::complete_task(&conn, &task.id, true)
        .unwrap()
        .is_none());
    let current = repo::get_task(&conn, &task.id).unwrap();
    assert!(current.is_completed);
    assert_eq!(current.recurrence_count, 0);
    assert_eq!(current.due_date, task.due_date);
    let fired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fired, 0);

    let next = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(next.recurrence_count, 1);
    assert_eq!(next.due_date.as_deref(), Some("2099-12-10"));
    assert!(!next.is_completed);
    let expected = domain::compute_reminder_at_in_timezone(
        Some("2099-12-10"),
        Some("09:00"),
        Some(0),
        chrono_tz::UTC,
    );
    assert_eq!(next.reminder_at, expected);
}

#[test]
fn changing_base_date_resets_count_aligns_first_occurrence_and_clears_fired() {
    let _timezone = lock_utc();
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    let input = TaskInput {
        due_date: Some("2026-11-17".into()),
        due_time: Some("09:00".into()),
        reminder_offset: Some(20),
        reminder_enabled: Some(true),
        recurrence: weekly_mwf(),
        ..make_input(&conn, "每周健身")
    };
    let task = repo::create_task(&conn, &input).unwrap();
    assert_eq!(task.due_date.as_deref(), Some("2026-11-18"));
    let advanced = repo::complete_task(&conn, &task.id, true).unwrap().unwrap();
    assert_eq!(advanced.recurrence_count, 1);
    assert_eq!(advanced.due_date.as_deref(), Some("2026-11-20"));
    insert_fired(&conn, &task.id);

    let recalculated = repo::update_task(
        &conn,
        &task.id,
        &TaskInput {
            due_date: Some("2026-12-08".into()),
            ..input
        },
    )
    .unwrap();
    assert_eq!(recalculated.recurrence_count, 0);
    assert_eq!(recalculated.due_date.as_deref(), Some("2026-12-09"));
    let fired: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='fired'",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(fired, 0);
    let expected = domain::compute_reminder_at_in_timezone(
        Some("2026-12-09"),
        Some("09:00"),
        Some(20),
        chrono_tz::UTC,
    );
    assert_eq!(recalculated.reminder_at, expected);
}
