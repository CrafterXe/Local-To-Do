use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::AppResult;
use crate::state::AppState;
use crate::{backup, db, domain, logging, repo};

/// 超过该时长未触发的提醒视为「错过」，不再弹出打扰性通知
const MISSED_GRACE_MS: i64 = 24 * 3600 * 1000;
const CHECK_INTERVAL_MS: i64 = 20_000;
const BACKUP_CHECK_INTERVAL_MS: i64 = 30 * 60 * 1000;

static LAST_BACKUP_CHECK: AtomicI64 = AtomicI64::new(0);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiredReminderEvent {
    pub task_id: String,
    pub title: String,
    pub body: String,
    pub trigger_at: i64,
    pub late: bool,
}

/// 发送 Windows 原生通知
pub fn send_notification(app: &AppHandle, title: &str, body: &str) -> bool {
    use tauri_plugin_notification::NotificationExt;
    match app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
    {
        Ok(_) => true,
        Err(e) => {
            logging::warn(format!("通知发送失败: {e}"));
            false
        }
    }
}

/// 启动后台提醒调度线程（不受窗口可见性影响）
pub fn spawn(app: AppHandle, state: Arc<AppState>) {
    let result = std::thread::Builder::new()
        .name("reminder-scheduler".into())
        .spawn(move || {
            logging::info("提醒调度器已启动");
            {
                let conn = state.db.lock();
                let _ = repo::prune_schedules(&conn);
            }
            let mut last_offset = chrono::Local::now().offset().local_minus_utc();
            loop {
                // 自动时区模式下，系统时区/夏令时变化后重新校准尚未触发的提醒
                let offset = chrono::Local::now().offset().local_minus_utc();
                if offset != last_offset {
                    last_offset = offset;
                    if domain::current_timezone().is_none() {
                        let conn = state.db.lock();
                        match db::refresh_all_reminders(&conn) {
                            Ok(()) => logging::info("检测到系统时区变化，已重新校准提醒"),
                            Err(e) => logging::warn(format!("重新校准提醒失败: {e}")),
                        }
                    }
                }
                let now = domain::local_now_ms();
                if let Err(e) = tick(&app, &state, now) {
                    logging::error(format!("调度器执行异常: {e}"));
                }
                maybe_auto_backup(&state);
                let next = {
                    let conn = state.db.lock();
                    repo::next_trigger(&conn).unwrap_or(None)
                };
                let wait_ms = match next {
                    Some(t) if t > now => (t - now).min(CHECK_INTERVAL_MS),
                    _ => CHECK_INTERVAL_MS,
                };
                state.wait_for_wake(Duration::from_millis(wait_ms.max(500) as u64));
            }
        });
    if let Err(e) = result {
        logging::error(format!("无法启动调度线程: {e}"));
    }
}

fn maybe_auto_backup(state: &Arc<AppState>) {
    let now = domain::local_now_ms();
    let last = LAST_BACKUP_CHECK.load(Ordering::Relaxed);
    if now - last < BACKUP_CHECK_INTERVAL_MS {
        return;
    }
    LAST_BACKUP_CHECK.store(now, Ordering::Relaxed);
    let conn = state.db.lock();
    match backup::auto_backup_if_due(&conn, &state.backup_dir, false) {
        Ok(true) => logging::info("自动备份完成"),
        Ok(false) => {}
        Err(e) => logging::error(format!("自动备份失败: {e}")),
    }
}

fn tick(app: &AppHandle, state: &Arc<AppState>, now: i64) -> AppResult<()> {
    let due = {
        let conn = state.db.lock();
        repo::due_reminders(&conn, now)?
    };
    if due.is_empty() {
        return Ok(());
    }
    let (sound_enabled, sound_path) = {
        let conn = state.db.lock();
        let s = db::load_settings(&conn)?;
        (s.sound_enabled, s.sound_path)
    };

    for r in due {
        {
            let conn = state.db.lock();
            if r.is_completed || r.is_deleted || r.reminder_changed {
                repo::mark_schedule(&conn, &r.schedule_id, "cancelled", now)?;
                logging::info(format!("提醒计划失效，已取消: {}", r.title));
                continue;
            }
            if now - r.trigger_at > MISSED_GRACE_MS {
                repo::mark_schedule(&conn, &r.schedule_id, "missed", now)?;
                logging::warn(format!(
                    "提醒已错过超过 24 小时，不再弹窗: {} ({})",
                    r.title,
                    domain::format_trigger(r.trigger_at)
                ));
                continue;
            }
            // 先标记已触发，保证同一条提醒不会重复弹出
            repo::mark_schedule(&conn, &r.schedule_id, "fired", now)?;
        }

        let due_text = match (&r.due_date, &r.due_time) {
            (Some(d), Some(t)) => format!("到期：{d} {t}"),
            (Some(d), None) => format!("到期：{d}"),
            _ => String::new(),
        };
        let body = if due_text.is_empty() {
            format!("清单：{}", r.list_name)
        } else {
            format!("{due_text} · {}", r.list_name)
        };
        let late = now - r.trigger_at > 120_000;
        let body = if late {
            format!("（延迟提醒）{body}")
        } else {
            body
        };

        // 提醒是否触发由任务自身的开关决定，不再依赖全局通知开关
        let ok = send_notification(app, &r.title, &body);
        if ok {
            logging::info(format!("已发送提醒通知: {} @ {}", r.title, domain::format_trigger(r.trigger_at)));
        }
        // 应用自身播放提示音：不依赖 toast 的自定义声音能力
        if sound_enabled {
            if let Err(e) = crate::sound::play(sound_path.as_deref()) {
                logging::warn(format!("提示音播放失败: {e}"));
            }
        }

        let payload = FiredReminderEvent {
            task_id: r.task_id.clone(),
            title: r.title.clone(),
            body: body.clone(),
            trigger_at: r.trigger_at,
            late,
        };
        let _ = app.emit("reminder-fired", payload);
    }
    Ok(())
}
