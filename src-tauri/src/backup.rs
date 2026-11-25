use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};
use rusqlite::types::Value as SqlValue;
use rusqlite::{params, params_from_iter, Connection};
use serde_json::{Map, Value};

use ltd_core::db::{load_settings, save_settings};
use ltd_core::error::{err, AppError, AppResult};
use ltd_core::models::BackupEntry;

pub const APP_VERSION: &str = "1.0.0";
const FORMAT: &str = "local-to-do-backup";
const FORMAT_VERSION: i64 = 1;
const KEEP_BACKUPS: usize = 20;

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
                rusqlite::types::ValueRef::Blob(b) => Value::from(String::from_utf8_lossy(b).to_string()),
            };
            obj.insert(name.clone(), v);
        }
        out.push(Value::Object(obj));
    }
    Ok(out)
}

/// 导出完整数据包（JSON）
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
        "exportedAt": ltd_core::domain::local_now_ms(),
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

fn fmt_ms(ms: Option<i64>) -> String {
    match ms {
        Some(v) => match Local.timestamp_millis_opt(v) {
            chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
            _ => String::new(),
        },
        None => String::new(),
    }
}

fn recurrence_label(kind: &str, interval: i64, weekdays: &str, month_day: Option<i64>) -> String {
    let weekday_names = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    match kind {
        "daily" => {
            if interval <= 1 {
                "每天".into()
            } else {
                format!("每 {interval} 天")
            }
        }
        "weekdays" => "每个工作日".into(),
        "weekly" => {
            let days: Vec<&str> = weekdays
                .split(',')
                .filter_map(|s| s.trim().parse::<usize>().ok())
                .filter(|n| (1..=7).contains(n))
                .map(|n| weekday_names[n - 1])
                .collect();
            let base = if interval <= 1 {
                "每周".to_string()
            } else {
                format!("每 {interval} 周")
            };
            if days.is_empty() {
                base
            } else {
                format!("{base} {}", days.join("、"))
            }
        }
        "monthly" => {
            let base = if interval <= 1 {
                "每月".to_string()
            } else {
                format!("每 {interval} 个月")
            };
            match month_day {
                Some(d) => format!("{base} {d} 日"),
                None => base,
            }
        }
        "yearly" => "每年".into(),
        _ => "自定义重复".into(),
    }
}

/// 导出任务 CSV
pub fn export_csv(conn: &Connection, path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut wtr = csv::Writer::from_path(path)?;
    wtr.write_record([
        "任务ID",
        "清单",
        "标题",
        "备注",
        "重要",
        "已完成",
        "完成时间",
        "到期日期",
        "到期时间",
        "提醒提前分钟",
        "重复规则",
        "加入我的一天",
        "创建时间",
        "更新时间",
    ])?;
    let mut stmt = conn.prepare(
        "SELECT t.id, IFNULL(l.name,''), t.title, t.note, t.is_important, t.is_completed,
                t.completed_at, t.due_date, t.due_time, t.reminder_offset,
                IFNULL(r.kind,''), IFNULL(r.interval,1), IFNULL(r.weekdays,''), r.month_day,
                t.my_day, t.created_at, t.updated_at
         FROM tasks t LEFT JOIN lists l ON l.id=t.list_id LEFT JOIN recurrences r ON r.id=t.recurrence_id
         WHERE t.deleted_at IS NULL
         ORDER BY t.created_at ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)? != 0,
            r.get::<_, i64>(5)? != 0,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<i64>>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, i64>(11)?,
            r.get::<_, String>(12)?,
            r.get::<_, Option<i64>>(13)?,
            r.get::<_, Option<String>>(14)?,
            r.get::<_, i64>(15)?,
            r.get::<_, i64>(16)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            list,
            title,
            note,
            important,
            completed,
            completed_at,
            due_date,
            due_time,
            offset,
            kind,
            interval,
            weekdays,
            month_day,
            my_day,
            created,
            updated,
        ) = row?;
        let repeat = if kind.is_empty() {
            String::new()
        } else {
            recurrence_label(&kind, interval, &weekdays, month_day)
        };
        wtr.write_record([
            id,
            list,
            title,
            note,
            if important { "是" } else { "否" }.into(),
            if completed { "是" } else { "否" }.into(),
            fmt_ms(completed_at),
            due_date.unwrap_or_default(),
            due_time.unwrap_or_default(),
            offset.map(|v| v.to_string()).unwrap_or_default(),
            repeat,
            my_day.unwrap_or_default(),
            fmt_ms(Some(created)),
            fmt_ms(Some(updated)),
        ])?;
    }
    wtr.flush()?;
    Ok(())
}

fn backup_file_name() -> String {
    format!("local_to_do_backup_{}", Local::now().format("%Y%m%d_%H%M%S"))
}

/// 立即备份：生成 JSON 快照 + 数据库文件副本
pub fn backup_now(backup_dir: &Path, conn: &Connection) -> AppResult<BackupEntry> {
    fs::create_dir_all(backup_dir)?;
    let stem = backup_file_name();
    let json_path = backup_dir.join(format!("{stem}.json"));
    let db_path = backup_dir.join(format!("{stem}.db"));

    let bundle = export_bundle(conn)?;
    write_json(&json_path, &bundle)?;

    if db_path.exists() {
        fs::remove_file(&db_path)?;
    }
    conn.execute("VACUUM INTO ?1", params![db_path.to_string_lossy().to_string()])?;

    let now = ltd_core::domain::local_now_ms();
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('last_backup_at', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![now.to_string()],
    )?;

    prune_backups(backup_dir)?;

    let size = fs::metadata(&json_path).map(|m| m.len() as i64).unwrap_or(0);
    Ok(BackupEntry {
        path: json_path.to_string_lossy().to_string(),
        file_name: format!("{stem}.json"),
        size,
        created_at: now,
        format: "json".into(),
    })
}

pub fn list_backups(backup_dir: &Path) -> AppResult<Vec<BackupEntry>> {
    let mut out = Vec::new();
    if !backup_dir.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(backup_dir)? {
        let entry = entry?;
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext != "json" && ext != "db" && ext != "csv" {
            continue;
        }
        let meta = entry.metadata()?;
        let created = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        out.push(BackupEntry {
            path: path.to_string_lossy().to_string(),
            file_name: path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            size: meta.len() as i64,
            created_at: created,
            format: ext,
        });
    }
    out.sort_by(|a, b| b.path.cmp(&a.path));
    Ok(out)
}

fn prune_backups(backup_dir: &Path) -> AppResult<()> {
    let entries = list_backups(backup_dir)?;
    let mut json_files: Vec<&BackupEntry> = entries.iter().filter(|e| e.format == "json").collect();
    json_files.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    for old in json_files.into_iter().skip(KEEP_BACKUPS) {
        let _ = fs::remove_file(&old.path);
        let db_twin = PathBuf::from(&old.path).with_extension("db");
        let _ = fs::remove_file(db_twin);
    }
    Ok(())
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

/// 从 JSON 备份恢复数据（覆盖现有数据；调用方需先征得用户确认）
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

    ltd_core::db::refresh_all_reminders(conn)?;

    // 恢复后更新最近备份时间
    let mut s = load_settings(conn)?;
    s.last_backup_at = Some(ltd_core::domain::local_now_ms());
    save_settings(conn, &s)?;

    Ok(RestoreReport {
        lists,
        tasks,
        notes,
        recurrences,
    })
}

/// 到达间隔则执行自动备份
pub fn auto_backup_if_due(conn: &Connection, backup_dir: &Path, force: bool) -> AppResult<bool> {
    let settings = load_settings(conn)?;
    if !settings.auto_backup_enabled && !force {
        return Ok(false);
    }
    let now = ltd_core::domain::local_now_ms();
    let interval_ms = settings.auto_backup_interval_hours.max(1) * 3600 * 1000;
    if !force {
        if let Some(last) = settings.last_backup_at {
            if now - last < interval_ms {
                return Ok(false);
            }
        }
    }
    backup_now(backup_dir, conn)?;
    Ok(true)
}
