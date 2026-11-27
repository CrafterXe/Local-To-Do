use std::fs;
use std::path::Path;

use rusqlite::types::Value as SqlValue;
use rusqlite::{params_from_iter, Connection};
use serde_json::{Map, Value};

use crate::db::{load_settings, save_settings};
use crate::error::{err, AppError, AppResult};

pub const APP_VERSION: &str = "1.0.0";
const FORMAT: &str = "local-to-do-backup";
const FORMAT_VERSION: i64 = 1;

/// 把整张表导出为 JSON 行
fn dump_table(conn: &Connection, table: &str) -> AppResult<Vec<Value>> {
    let mut stmt = conn.prepare(&format!("SELECT * FROM {table}"))?;
    let columns: Vec<String> = stmt.column_names().map(|s| s.to_string()).collect();
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut obj = Map::new();
        for (i, name) in columns.iter().enumerate() {
            let v: Value = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => Value::from(n),
                rusqlite::types::ValueRef::Real(f) => Value::from(f),
                rusqlite::types::ValueRef::Text(t) => {
                    Value::from(String::from_utf8_lossy(t).to_string())
                }
                rusqlite::types::ValueRef::Blob(b) => {
                    Value::from(String::from_utf8_lossy(b).to_string())
                }
            };
            obj.insert(name.clone(), v);
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// 导出完整数据包（JSON），不包含任何平台文件路径或 Tauri 状态。
pub fn export_bundle(conn: &Connection) -> AppResult<Value> {
    let mut data = Map::new();
    for table in [
        "lists",
        "recurrences",
        "tasks",
        "task_steps",
        "notes",
        "task_history",
        "reminder_schedules",
        "app_settings",
    ] {
        data.insert(table.to_string(), Value::Array(dump_table(conn, table)?));
    }
    Ok(serde_json::json!({
        "format": FORMAT,
        "version": FORMAT_VERSION,
        "appVersion": APP_VERSION,
        "exportedAt": crate::domain::local_now_ms(),
        "data": Value::Object(data),
    }))
}

fn write_json(path: &Path, value: &Value) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, text)?;
    Ok(())
}

pub fn export_json(conn: &Connection, path: &Path) -> AppResult<()> {
    let bundle = export_bundle(conn)?;
    write_json(path, &bundle)
}

fn json_to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else {
                SqlValue::Real(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

fn insert_rows(conn: &Connection, table: &str, rows: &[Value]) -> AppResult<usize> {
    let mut count = 0;
    for row in rows {
        let obj = match row.as_object() {
            Some(o) => o,
            None => continue,
        };
        let cols: Vec<String> = obj.keys().cloned().collect();
        let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "INSERT INTO {table} ({}) VALUES ({})",
            cols.join(", "),
            placeholders.join(", ")
        );
        let vals: Vec<SqlValue> = cols.iter().map(|c| json_to_sql(&obj[c])).collect();
        conn.execute(&sql, params_from_iter(vals.iter()))?;
        count += 1;
    }
    Ok(count)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub lists: usize,
    pub tasks: usize,
    pub notes: usize,
    pub recurrences: usize,
}

fn parse_bundle(text: &str) -> AppResult<Value> {
    let bundle: Value = serde_json::from_str(text)?;
    if bundle.get("format").and_then(|v| v.as_str()) != Some(FORMAT) {
        return err("不是有效的 Local To Do 备份文件");
    }
    let version = bundle.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
    if version > FORMAT_VERSION {
        return err("备份文件版本较新，当前应用无法恢复");
    }
    if bundle.get("data").is_none() {
        return Err(AppError::Other("备份文件缺少 data 字段".into()));
    }
    Ok(bundle)
}

fn restore_report(bundle: &Value) -> RestoreReport {
    let data = &bundle["data"];
    let count = |name: &str| {
        data.get(name)
            .and_then(|v| v.as_array())
            .map(|rows| rows.len())
            .unwrap_or(0)
    };
    RestoreReport {
        lists: count("lists"),
        tasks: count("tasks"),
        notes: count("notes"),
        recurrences: count("recurrences"),
    }
}

/// 校验内存中的 JSON 备份，不写数据库。
pub fn validate_json_text(text: &str) -> AppResult<RestoreReport> {
    parse_bundle(text).map(|bundle| restore_report(&bundle))
}

/// 从 JSON 文件恢复；桌面与移动端共用内存文本恢复实现。
pub fn restore_from_json(conn: &mut Connection, path: &Path) -> AppResult<RestoreReport> {
    let text = fs::read_to_string(path)?;
    restore_from_json_text(conn, &text)
}

/// 从 JSON 文本恢复数据（覆盖现有数据；调用方需先征得用户确认）。
/// 业务表替换在同一个 SQLite 事务中完成，格式错误或任一插入失败均保留原数据。
pub fn restore_from_json_text(conn: &mut Connection, text: &str) -> AppResult<RestoreReport> {
    let bundle = parse_bundle(text)?;
    let data = &bundle["data"];

    let tx = conn.transaction()?;
    tx.execute("DELETE FROM reminder_schedules", [])?;
    tx.execute("DELETE FROM task_steps", [])?;
    tx.execute("DELETE FROM task_history", [])?;
    tx.execute("DELETE FROM tasks", [])?;
    tx.execute("DELETE FROM notes", [])?;
    tx.execute("DELETE FROM recurrences", [])?;
    tx.execute("DELETE FROM lists", [])?;

    let get_rows = |name: &str| -> Vec<Value> {
        data.get(name)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };

    let lists = insert_rows(&tx, "lists", &get_rows("lists"))?;
    let recurrences = insert_rows(&tx, "recurrences", &get_rows("recurrences"))?;
    let tasks = insert_rows(&tx, "tasks", &get_rows("tasks"))?;
    insert_rows(&tx, "task_steps", &get_rows("task_steps"))?;
    let notes = insert_rows(&tx, "notes", &get_rows("notes"))?;
    insert_rows(&tx, "task_history", &get_rows("task_history"))?;

    // 所有后续写入也属于恢复事务：任一步失败都必须完整保留原数据。
    crate::db::refresh_all_reminders_in_tx(&tx)?;

    // 恢复后更新最近备份时间
    let mut s = load_settings(&tx)?;
    s.last_backup_at = Some(crate::domain::local_now_ms());
    save_settings(&tx, &s)?;

    // 整体替换数据后，通知旧同步客户端丢弃游标并重新拉取全量快照。
    crate::sync::record_reset(&tx)?;
    tx.commit()?;

    Ok(RestoreReport {
        lists,
        tasks,
        notes,
        recurrences,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn invalid_or_future_text_restore_preserves_original_data() {
        let db = Db::open_memory().unwrap();
        let mut conn = db.lock();
        conn.execute(
            "INSERT INTO notes (id, title, body, created_at, updated_at)
             VALUES ('note-original', '原标题', '正文', 1, 1)",
            [],
        )
        .unwrap();
        let original_lists: i64 = conn
            .query_row("SELECT COUNT(*) FROM lists", [], |row| row.get(0))
            .unwrap();
        let mut bundle = export_bundle(&conn).unwrap();

        bundle["version"] = Value::from(FORMAT_VERSION + 1);
        let future = serde_json::to_string(&bundle).unwrap();
        assert!(restore_from_json_text(&mut conn, &future).is_err());
        assert!(validate_json_text(&future).is_err());

        bundle["version"] = Value::from(FORMAT_VERSION);
        bundle["data"]["notes"] = serde_json::json!([{"id": "broken"}]);
        let malformed = serde_json::to_string(&bundle).unwrap();
        assert!(restore_from_json_text(&mut conn, &malformed).is_err());

        let title: String = conn
            .query_row(
                "SELECT title FROM notes WHERE id='note-original'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let lists: i64 = conn
            .query_row("SELECT COUNT(*) FROM lists", [], |row| row.get(0))
            .unwrap();
        assert_eq!(title, "原标题");
        assert_eq!(lists, original_lists);
    }

    #[test]
    fn post_insert_reminder_failure_rolls_back_entire_restore() {
        let db = Db::open_memory().unwrap();
        let mut conn = db.lock();
        conn.execute(
            "INSERT INTO notes (id, title, body, created_at, updated_at)
             VALUES ('note-original', '原数据', '不得丢失', 1, 1)",
            [],
        )
        .unwrap();

        let imported = Db::open_memory().unwrap();
        let text = {
            let imported_conn = imported.lock();
            let list_id: String = imported_conn
                .query_row("SELECT id FROM lists LIMIT 1", [], |row| row.get(0))
                .unwrap();
            imported_conn
                .execute(
                    "INSERT INTO tasks
                     (id, list_id, title, due_date, due_time, reminder_offset,
                      reminder_enabled, created_at, updated_at)
                     VALUES ('task-imported', ?1, '导入任务', '2099-01-01', '09:00', 0, 1, 1, 1)",
                    [list_id],
                )
                .unwrap();
            serde_json::to_string(&export_bundle(&imported_conn).unwrap()).unwrap()
        };

        // 强制提醒计划在业务行插入之后失败，验证后续步骤仍处于同一事务。
        conn.execute_batch(
            "CREATE TRIGGER fail_restore_schedule BEFORE INSERT ON reminder_schedules
             BEGIN SELECT RAISE(ABORT, 'forced reminder failure'); END;",
        )
        .unwrap();
        assert!(restore_from_json_text(&mut conn, &text).is_err());

        let original: String = conn
            .query_row(
                "SELECT title FROM notes WHERE id='note-original'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let imported_tasks: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE id='task-imported'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(original, "原数据");
        assert_eq!(imported_tasks, 0);
    }

    #[test]
    fn text_restore_roundtrip_and_validation() {
        let db = Db::open_memory().unwrap();
        let mut conn = db.lock();
        conn.execute(
            "INSERT INTO notes (id, title, body, created_at, updated_at)
             VALUES ('note-1', '文本恢复', '内容', 1, 1)",
            [],
        )
        .unwrap();
        let text = serde_json::to_string(&export_bundle(&conn).unwrap()).unwrap();
        let report = validate_json_text(&text).unwrap();
        assert_eq!(report.notes, 1);

        conn.execute("DELETE FROM notes", []).unwrap();
        let restored = restore_from_json_text(&mut conn, &text).unwrap();
        assert_eq!(restored.notes, 1);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes WHERE id='note-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
