use std::sync::Mutex;

use ltd_core::db::{self, Db};
use ltd_core::domain::{
    self, reminder_disposition, reminder_is_late, ReminderDisposition, REMINDER_LATE_MS,
    REMINDER_MISSED_GRACE_MS,
};
use ltd_core::models::PendingReminder;
use ltd_core::repo;
use ltd_core::repo::tasks::TaskInput;
use rusqlite::Connection;

static TZ_TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_timezone() -> std::sync::MutexGuard<'static, ()> {
    let guard = TZ_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    domain::set_timezone(None);
    guard
}

fn pending_reminder() -> PendingReminder {
    PendingReminder {
        schedule_id: "schedule-1".into(),
        task_id: "task-1".into(),
        title: "提醒".into(),
        list_name: "任务".into(),
        due_date: Some("2026-11-23".into()),
        due_time: Some("09:00".into()),
        trigger_at: 1_000_000,
        is_completed: false,
        is_deleted: false,
        reminder_changed: false,
    }
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

fn status(conn: &Connection, schedule_id: &str) -> (String, Option<i64>) {
    conn.query_row(
        "SELECT status, fired_at FROM reminder_schedules WHERE id=?1",
        [schedule_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

#[test]
fn disposition_preserves_windows_grace_and_late_boundaries() {
    let reminder = pending_reminder();
    let trigger = reminder.trigger_at;

    assert_eq!(
        reminder_disposition(&reminder, trigger),
        ReminderDisposition::Fire
    );
    assert_eq!(
        reminder_disposition(&reminder, trigger + REMINDER_MISSED_GRACE_MS),
        ReminderDisposition::Fire,
        "恰好二十四小时仍按当前 Windows 行为触发"
    );
    assert_eq!(
        reminder_disposition(&reminder, trigger + REMINDER_MISSED_GRACE_MS + 1),
        ReminderDisposition::Missed
    );
    assert!(!reminder_is_late(trigger, trigger + REMINDER_LATE_MS));
    assert!(reminder_is_late(trigger, trigger + REMINDER_LATE_MS + 1));

    for (field, expected) in [
        ("completed", ReminderDisposition::Cancel),
        ("deleted", ReminderDisposition::Cancel),
        ("changed", ReminderDisposition::Cancel),
    ] {
        let mut invalid = reminder.clone();
        match field {
            "completed" => invalid.is_completed = true,
            "deleted" => invalid.is_deleted = true,
            _ => invalid.reminder_changed = true,
        }
        assert_eq!(
            reminder_disposition(&invalid, trigger + REMINDER_MISSED_GRACE_MS + 1),
            expected,
            "失效计划优先于错过判断: {field}"
        );
    }
}

#[test]
fn due_query_and_mark_schedule_survive_clock_changes_without_duplicate_delivery() {
    let _timezone = lock_timezone();
    domain::set_timezone(Some("UTC".into()));
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2099-01-01".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "时钟边界")
        },
    )
    .unwrap();
    let trigger = task.reminder_at.unwrap();

    assert!(repo::due_reminders(&conn, trigger - 1).unwrap().is_empty());
    let due = repo::due_reminders(&conn, trigger).unwrap();
    assert_eq!(due.len(), 1);
    let schedule_id = due[0].schedule_id.clone();
    assert_eq!(
        reminder_disposition(&due[0], trigger),
        ReminderDisposition::Fire
    );

    // 系统时间回拨不应产生提前触发；前跳到同一时刻也只返回同一条 pending。
    assert!(repo::due_reminders(&conn, trigger - 60_000)
        .unwrap()
        .is_empty());
    assert_eq!(
        repo::due_reminders(&conn, trigger + 60_000).unwrap().len(),
        1
    );
    repo::mark_schedule(&conn, &schedule_id, "fired", trigger + 60_000).unwrap();
    assert_eq!(
        status(&conn, &schedule_id),
        ("fired".into(), Some(trigger + 60_000))
    );
    assert!(repo::due_reminders(&conn, trigger + 60_000)
        .unwrap()
        .is_empty());

    repo::sync_schedule(&conn, &task.id, Some(trigger)).unwrap();
    let pending: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM reminder_schedules WHERE task_id=?1 AND status='pending'",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(pending, 0, "已触发时刻不得因重新同步而再次 pending");
    assert_eq!(status(&conn, &schedule_id).0, "fired");
}

#[test]
fn timezone_rebuild_replaces_pending_and_deduplicates_same_fired_instant() {
    let _timezone = lock_timezone();
    domain::set_timezone(Some("America/New_York".into()));
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2099-11-01".into()),
            due_time: Some("01:30".into()),
            reminder_offset: Some(0),
            reminder_enabled: Some(true),
            ..make_input(&conn, "时区重建")
        },
    )
    .unwrap();
    let new_york_at = task.reminder_at;
    let old_schedule: String = conn
        .query_row(
            "SELECT id FROM reminder_schedules WHERE task_id=?1 AND status='pending'",
            [&task.id],
            |row| row.get(0),
        )
        .unwrap();

    domain::set_timezone(Some("Asia/Shanghai".into()));
    db::refresh_all_reminders(&conn).unwrap();
    let shanghai = repo::get_task(&conn, &task.id).unwrap();
    let expected = domain::compute_reminder_at_in_timezone(
        Some("2099-11-01"),
        Some("01:30"),
        Some(0),
        chrono_tz::Asia::Shanghai,
    );
    assert_eq!(shanghai.reminder_at, expected);
    assert_ne!(shanghai.reminder_at, new_york_at);
    assert_eq!(status(&conn, &old_schedule).0, "cancelled");

    let current: Vec<(String, i64)> = {
        let mut stmt = conn
            .prepare("SELECT id, trigger_at FROM reminder_schedules WHERE task_id=?1 AND status='pending'")
            .unwrap();
        let rows = stmt
            .query_map([&task.id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].1, expected.unwrap());
    repo::mark_schedule(&conn, &current[0].0, "fired", expected.unwrap()).unwrap();

    // 同一时区重复重建不得把同一触发时刻重新置为 pending。
    db::refresh_all_reminders(&conn).unwrap();
    let counts: (i64, i64) = conn
        .query_row(
            "SELECT IFNULL(SUM(status='pending'), 0), IFNULL(SUM(status='fired'), 0)
             FROM reminder_schedules WHERE task_id=?1",
            [&task.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (0, 1));
    domain::set_timezone(None);
}
