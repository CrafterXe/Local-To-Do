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

/// 从 JSON 备份恢复数据（覆盖现有数据；调用方需先征得用户确认）。
/// 业务表替换在同一个 SQLite 事务中完成，任一插入失败都会整体回滚。
pub fn restore_from_json(conn: &mut Connection, path: &Path) -> AppResult<RestoreReport> {
    let text = fs::read_to_string(path)?;
    let bundle: Value = serde_json::from_str(&text)?;
    if bundle.get("format").and_then(|v| v.as_str()) != Some(FORMAT) {
        return err("不是有效的 Local To Do 备份文件");
    }
    let version = bundle.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
    if version > FORMAT_VERSION {
        return err("备份文件版本较新，当前应用无法恢复");
    }
    let data = bundle
        .get("data")
        .ok_or_else(|| AppError::Other("备份文件缺少 data 字段".into()))?;

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
    tx.commit()?;

    crate::db::refresh_all_reminders(conn)?;

    // 恢复后更新最近备份时间
    let mut s = load_settings(conn)?;
    s.last_backup_at = Some(crate::domain::local_now_ms());
    save_settings(conn, &s)?;

    // 整体替换数据后，通知手机端丢弃游标并重新拉取全量快照
    crate::sync::record_reset(conn)?;

    Ok(RestoreReport {
        lists,
        tasks,
        notes,
        recurrences,
    })
}
