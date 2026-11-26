use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ltd_core::backup::{export_bundle, export_json, restore_from_json};
use ltd_core::db::Db;
use ltd_core::domain;
use ltd_core::repo;
use ltd_core::repo::tasks::{RecurrenceInput, TaskInput};
use rusqlite::Connection;
use serde_json::Value;

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    domain::set_timezone(None);
    guard
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("ltd-core-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

fn assert_integrity(conn: &Connection) {
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
    let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
    let mut rows = stmt.query([]).unwrap();
    let mut violations = 0;
    while rows.next().unwrap().is_some() {
        violations += 1;
    }
    assert_eq!(violations, 0, "不应存在外键违规");
}

#[test]
fn file_db_initializes_pragmas_schema_seed_and_reopens_persistent_data() {
    let _guard = lock_test();
    let dir = TestDir::new("init");
    let path = dir.join("local-to-do.db");
    {
        let db = Db::open(&path).unwrap();
        let conn = db.lock();
        let journal: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal.to_ascii_lowercase(), "wal");
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5000
        );
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5
        );
        for table in [
            "lists",
            "recurrences",
            "tasks",
            "task_steps",
            "reminder_schedules",
            "task_history",
            "notes",
            "app_settings",
            "sync_log",
        ] {
            let present: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(present, 1, "缺少数据表 {table}");
        }
        assert_eq!(count(&conn, "lists"), 1);
        assert_eq!(repo::all_lists(&conn).unwrap()[0].name, "任务");
        repo::create_task(&conn, &make_input(&conn, "重启后保留")).unwrap();
        assert_integrity(&conn);
    }

    let reopened = Db::open(&path).unwrap();
    let conn = reopened.lock();
    assert_eq!(count(&conn, "tasks"), 1);
    assert_eq!(
        repo::get_task(
            &conn,
            &repo::list_tasks(
                &conn,
                &ltd_core::models::TaskQuery {
                    view: "all".into(),
                    list_id: None,
                    search: None,
                    include_completed: Some(true),
                }
            )
            .unwrap()[0]
                .id
        )
        .unwrap()
        .title,
        "重启后保留"
    );
    assert_integrity(&conn);
}

#[test]
fn foreign_keys_reject_orphan_task_and_step() {
    let _guard = lock_test();
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    let task_error = conn.execute(
        "INSERT INTO tasks (id, list_id, title, created_at, updated_at)
         VALUES ('orphan-task', 'missing-list', '孤儿', 1, 1)",
        [],
    );
    assert!(task_error.is_err());
    let step_error = conn.execute(
        "INSERT INTO task_steps (id, task_id, title) VALUES ('orphan-step', 'missing-task', '孤儿')",
        [],
    );
    assert!(step_error.is_err());
    assert_eq!(count(&conn, "tasks"), 0);
    assert_eq!(count(&conn, "task_steps"), 0);
    assert_integrity(&conn);
}

#[test]
fn failed_business_write_rolls_back_task_recurrence_history_and_sync_log() {
    let _guard = lock_test();
    let db = Db::open_memory().unwrap();
    let conn = db.lock();
    conn.execute_batch(
        "CREATE TRIGGER reject_sync_log BEFORE INSERT ON sync_log
         BEGIN SELECT RAISE(FAIL, 'forced sync log failure'); END;",
    )
    .unwrap();
    let result = repo::create_task(
        &conn,
        &TaskInput {
            due_date: Some("2026-01-31".into()),
            recurrence: Some(RecurrenceInput {
                kind: "monthly".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: Some(31),
            }),
            ..make_input(&conn, "必须整体回滚")
        },
    );
    assert!(result.is_err());
    assert_eq!(count(&conn, "tasks"), 0);
    assert_eq!(count(&conn, "recurrences"), 0);
    assert_eq!(count(&conn, "task_history"), 0);
    assert_eq!(count(&conn, "sync_log"), 0);
    assert_eq!(count(&conn, "lists"), 1, "事务前的种子数据必须保留");
    assert_integrity(&conn);
}

#[test]
fn failed_migration_restores_pre_migration_backup_and_user_data() {
    let _guard = lock_test();
    let dir = TestDir::new("migration-rollback");
    let path = dir.join("old.db");
    let note_id = {
        let db = Db::open(&path).unwrap();
        let conn = db.lock();
        let note = repo::create_note(&conn, "迁移前笔记", "不得丢失").unwrap();
        note.id
    };
    {
        let conn = Connection::open(&path).unwrap();
        // 故障注入：v4 打开时，v5 所需索引会因故意缺失的列而失败。
        conn.execute_batch(
            "DROP TABLE sync_log;
             CREATE TABLE sync_log (dummy TEXT);
             PRAGMA user_version = 4;",
        )
        .unwrap();
    }

    let migration = Db::open(&path);
    assert!(migration.is_err(), "故障注入必须使迁移失败");
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().contains(".v4-backup-"))
        .count();
    assert_eq!(backups, 1, "迁移前应留下一个一致性副本");

    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        4,
        "失败迁移不得提交新版本号"
    );
    assert_eq!(count(&conn, "notes"), 1);
    let title: String = conn
        .query_row("SELECT title FROM notes WHERE id=?1", [note_id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(title, "迁移前笔记");
    let partial_index: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_sync_log_entity'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(partial_index, 0, "失败索引不得部分提交");
    assert_integrity(&conn);
}

#[test]
fn json_backup_roundtrip_preserves_graph_and_rebuilds_reminders() {
    let _guard = lock_test();
    domain::set_timezone(Some("UTC".into()));
    let dir = TestDir::new("backup-roundtrip");
    let json_path = dir.join("backup.json");
    let db = Db::open_memory().unwrap();
    let mut conn = db.lock();
    let list = repo::create_list(&conn, "项目", "#ff6600", "").unwrap();
    let task = repo::create_task(
        &conn,
        &TaskInput {
            list_id: Some(list.id.clone()),
            due_date: Some("2099-01-31".into()),
            due_time: Some("09:00".into()),
            reminder_offset: Some(20),
            reminder_enabled: Some(true),
            recurrence: Some(RecurrenceInput {
                kind: "monthly".into(),
                interval: 1,
                weekdays: String::new(),
                month_day: Some(31),
            }),
            ..make_input(&conn, "完整关系图")
        },
    )
    .unwrap();
    repo::add_step(&conn, &task.id, "第一步").unwrap();
    let note = repo::create_note(&conn, "笔记", "正文").unwrap();
    export_json(&conn, &json_path).unwrap();

    repo::create_task(&conn, &make_input(&conn, "恢复后应消失的冗余任务")).unwrap();
    repo::create_note(&conn, "冗余笔记", "恢复后应消失").unwrap();
    let report = restore_from_json(&mut conn, &json_path).unwrap();
    assert_eq!(
        (report.lists, report.tasks, report.notes, report.recurrences),
        (2, 1, 1, 1)
    );

    let restored = repo::get_task(&conn, &task.id).unwrap();
    assert_eq!(restored.title, "完整关系图");
    assert_eq!(restored.steps.len(), 1);
    assert_eq!(restored.steps[0].title, "第一步");
    assert_eq!(restored.recurrence.as_ref().unwrap().month_day, Some(31));
    assert_eq!(restored.list_id, list.id);
    assert_eq!(repo::list_notes(&conn, None).unwrap().len(), 1);
    assert_eq!(repo::get_note(&conn, &note.id).unwrap().body, "正文");
    let pending: (i64, i64) = conn
        .query_row(
            "SELECT COUNT(*), MIN(trigger_at) FROM reminder_schedules WHERE task_id=?1 AND status='pending'",
            [&task.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(pending, (1, restored.reminder_at.unwrap()));
    assert!(count(&conn, "task_history") >= 1);
    assert_integrity(&conn);
}

#[test]
fn invalid_restore_rolls_back_and_preserves_existing_database() {
    let _guard = lock_test();
    let dir = TestDir::new("restore-rollback");
    let json_path = dir.join("invalid.json");
    let db = Db::open_memory().unwrap();
    let mut conn = db.lock();
    let task = repo::create_task(&conn, &make_input(&conn, "现有任务")).unwrap();
    repo::create_note(&conn, "现有笔记", "不得被清空").unwrap();
    let mut bundle = export_bundle(&conn).unwrap();
    bundle["data"]["tasks"][0]["list_id"] = Value::String("missing-list".into());
    std::fs::write(&json_path, serde_json::to_string_pretty(&bundle).unwrap()).unwrap();

    let result = restore_from_json(&mut conn, &json_path);
    assert!(result.is_err(), "损坏外键必须使恢复失败");
    assert_eq!(count(&conn, "tasks"), 1);
    assert_eq!(count(&conn, "notes"), 1);
    assert_eq!(count(&conn, "lists"), 1);
    assert_eq!(repo::get_task(&conn, &task.id).unwrap().title, "现有任务");
    assert_eq!(repo::list_notes(&conn, None).unwrap()[0].title, "现有笔记");
    assert_integrity(&conn);
}

#[test]
fn concurrent_task_writes_are_serialized_without_lost_rows_or_duplicate_order() {
    let _guard = lock_test();
    let dir = TestDir::new("concurrent");
    let path = dir.join("concurrent.db");
    let db = Arc::new(Db::open(&path).unwrap());
    let list_id = repo::all_lists(&db.lock()).unwrap()[0].id.clone();
    let mut workers = Vec::new();

    for worker in 0..4 {
        let db = Arc::clone(&db);
        let list_id = list_id.clone();
        workers.push(std::thread::spawn(move || {
            for index in 0..8 {
                let input = TaskInput {
                    title: format!("并发任务-{worker}-{index}"),
                    list_id: Some(list_id.clone()),
                    note: None,
                    is_important: None,
                    due_date: None,
                    due_time: None,
                    reminder_offset: None,
                    reminder_enabled: None,
                    my_day: None,
                    recurrence: None,
                };
                let conn = db.lock();
                repo::create_task(&conn, &input).unwrap();
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }

    let conn = db.lock();
    assert_eq!(count(&conn, "tasks"), 32);
    assert_eq!(count(&conn, "task_history"), 32);
    let unique_order: i64 = conn
        .query_row("SELECT COUNT(DISTINCT sort_order) FROM tasks", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(unique_order, 32, "串行事务不得产生重复排序值");
    assert_integrity(&conn);
}
