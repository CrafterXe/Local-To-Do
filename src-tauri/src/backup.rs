use std::fs;
use std::path::{Path, PathBuf};

use chrono::{Local, TimeZone};
use rusqlite::{params, Connection};

use ltd_core::db::load_settings;
use ltd_core::error::AppResult;
use ltd_core::models::BackupEntry;

pub use ltd_core::backup::{export_json, restore_from_json, RestoreReport};

const KEEP_BACKUPS: usize = 20;

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
    format!(
        "local_to_do_backup_{}",
        Local::now().format("%Y%m%d_%H%M%S")
    )
}

/// 立即备份：生成 JSON 快照 + 数据库文件副本
pub fn backup_now(backup_dir: &Path, conn: &Connection) -> AppResult<BackupEntry> {
    fs::create_dir_all(backup_dir)?;
    let stem = backup_file_name();
    let json_path = backup_dir.join(format!("{stem}.json"));
    let db_path = backup_dir.join(format!("{stem}.db"));

    export_json(conn, &json_path)?;

    if db_path.exists() {
        fs::remove_file(&db_path)?;
    }
    conn.execute(
        "VACUUM INTO ?1",
        params![db_path.to_string_lossy().to_string()],
    )?;

    let now = ltd_core::domain::local_now_ms();
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('last_backup_at', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![now.to_string()],
    )?;

    prune_backups(backup_dir)?;

    let size = fs::metadata(&json_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
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
