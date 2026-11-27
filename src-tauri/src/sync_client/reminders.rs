//! Android 后台提醒调度。
//!
//! 计划直接来自权威 `local-to-do.db`：`tasks.reminder_at` 就是最终触发时刻，
//! 不再读取同步镜像，也不再叠加 Android 本地提前量。原生层负责系统闹钟、
//! 通知展示和进程/开机恢复；本模块负责查询、去重、计划生成与 JNI 桥接。

use ltd_core::error::AppResult;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// 单次最多注册的闹钟数量；全部未来提醒按触发时间排序后取最早的一批。
pub const MAX_ALARMS: usize = 200;

/// 权威数据库中的一条提醒候选。
#[derive(Debug, Clone, PartialEq)]
pub struct ReminderRow {
    pub task_id: String,
    pub title: String,
    pub list_name: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub reminder_at: i64,
}

/// 一条待注册的本地提醒。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderItem {
    pub task_id: String,
    pub title: String,
    pub body: String,
    /// 实际触发时间（UTC 毫秒），与 tasks.reminder_at 完全一致。
    pub trigger_at: i64,
}

/// 原生层已展示、等待权威数据库确认的通知。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiredReminder {
    pub task_id: String,
    pub trigger_at: i64,
}

/// 计划生成统计，便于排查与展示。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStats {
    pub scheduled: usize,
    pub skipped_past: usize,
    pub skipped_far: usize,
    pub truncated: usize,
}

/// 原生层上报的状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeReminderStatus {
    #[serde(default)]
    pub exact_alarm_allowed: bool,
    #[serde(default)]
    pub notifications_allowed: bool,
    /// 是否已加入电池优化白名单（部分厂商熄屏后会清理未加入白名单的应用）。
    #[serde(default)]
    pub ignoring_battery_optimizations: bool,
    #[serde(default)]
    pub scheduled: usize,
}

/// 查询尚未触发的权威提醒。已经 fired 的同一任务/时刻不会再次注册。
pub fn reminder_candidates(conn: &Connection) -> AppResult<Vec<ReminderRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, COALESCE(l.name, ''), t.due_date, t.due_time, t.reminder_at
         FROM tasks t LEFT JOIN lists l ON l.id = t.list_id
         WHERE t.is_completed = 0
           AND t.deleted_at IS NULL
           AND t.reminder_enabled = 1
           AND t.reminder_at IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM reminder_schedules rs
               WHERE rs.task_id = t.id AND rs.trigger_at = t.reminder_at AND rs.status = 'fired'
           )
         ORDER BY t.reminder_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ReminderRow {
            task_id: row.get(0)?,
            title: row.get(1)?,
            list_name: row.get(2)?,
            due_date: row.get(3)?,
            due_time: row.get(4)?,
            reminder_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 所有仍应保留通知的任务，包括已经展示但尚未完成的提醒。
pub fn active_reminder_task_ids(conn: &Connection) -> AppResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM tasks
         WHERE is_completed = 0 AND deleted_at IS NULL
           AND reminder_enabled = 1 AND reminder_at IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// 根据权威行生成本地提醒计划。
///
/// 触发时间直接使用 `reminder_at`；已过提醒跳过，不限制未来日期范围。
pub fn build_plan(rows: &[ReminderRow], now_ms: i64) -> (Vec<ReminderItem>, PlanStats) {
    let mut stats = PlanStats::default();
    let mut items: Vec<ReminderItem> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for row in rows {
        if !seen.insert(row.task_id.as_str()) {
            continue;
        }
        let trigger_at = row.reminder_at;
        if trigger_at <= now_ms {
            stats.skipped_past += 1;
            continue;
        }
        items.push(ReminderItem {
            task_id: row.task_id.clone(),
            title: row.title.clone(),
            body: reminder_body(row),
            trigger_at,
        });
    }

    items.sort_by(|a, b| {
        a.trigger_at
            .cmp(&b.trigger_at)
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
    if items.len() > MAX_ALARMS {
        stats.truncated = items.len() - MAX_ALARMS;
        items.truncate(MAX_ALARMS);
    }
    stats.scheduled = items.len();
    (items, stats)
}

/// 通知正文：到期时间 + 清单名，不包含备注等敏感内容。
fn reminder_body(row: &ReminderRow) -> String {
    let due = match (&row.due_date, &row.due_time) {
        (Some(date), Some(time)) => Some(format!("到期：{date} {time}")),
        (Some(date), None) => Some(format!("到期：{date}")),
        _ => None,
    };
    let list = row.list_name.trim();
    match (due, list.is_empty()) {
        (Some(due), false) => format!("{due} · {list}"),
        (Some(due), true) => due,
        (None, false) => format!("清单：{list}"),
        (None, true) => "任务提醒".to_string(),
    }
}

// ---- 原生调度桥接 ----

pub fn schedule(items: &[ReminderItem], active_task_ids: &[String]) -> Result<(), String> {
    native::schedule(items, active_task_ids)
}

pub fn cancel_all() -> Result<(), String> {
    native::cancel_all()
}

pub fn status() -> Result<NativeReminderStatus, String> {
    native::status()
}

pub fn fired() -> Result<Vec<FiredReminder>, String> {
    native::fired()
}

pub fn consume_fired(items: &[FiredReminder]) -> Result<(), String> {
    native::consume_fired(items)
}

pub fn request_notifications() -> Result<bool, String> {
    native::request_notifications()
}

pub fn send_test_notification(title: &str, body: &str) -> Result<bool, String> {
    native::send_test_notification(title, body)
}

pub fn share_backup(path: &str) -> Result<(), String> {
    native::share_backup(path)
}

pub fn open_exact_alarm_settings() -> Result<(), String> {
    native::open_exact_alarm_settings()
}

pub fn open_battery_settings() -> Result<(), String> {
    native::open_battery_settings()
}

#[cfg(not(target_os = "android"))]
mod native {
    use super::*;

    pub fn schedule(_items: &[ReminderItem], _active_task_ids: &[String]) -> Result<(), String> {
        Err("后台提醒仅在 Android 端可用".to_string())
    }

    pub fn cancel_all() -> Result<(), String> {
        Ok(())
    }

    pub fn status() -> Result<NativeReminderStatus, String> {
        Ok(NativeReminderStatus::default())
    }

    pub fn fired() -> Result<Vec<FiredReminder>, String> {
        Ok(Vec::new())
    }

    pub fn consume_fired(_items: &[FiredReminder]) -> Result<(), String> {
        Ok(())
    }

    pub fn request_notifications() -> Result<bool, String> {
        Err("通知权限请求仅在 Android 端可用".to_string())
    }

    pub fn send_test_notification(_title: &str, _body: &str) -> Result<bool, String> {
        Err("原生测试通知仅在 Android 端可用".to_string())
    }

    pub fn share_backup(_path: &str) -> Result<(), String> {
        Err("备份分享仅在 Android 端可用".to_string())
    }

    pub fn open_exact_alarm_settings() -> Result<(), String> {
        Err("后台提醒仅在 Android 端可用".to_string())
    }

    pub fn open_battery_settings() -> Result<(), String> {
        Err("后台提醒仅在 Android 端可用".to_string())
    }
}

#[cfg(target_os = "android")]
mod native {
    use super::*;
    use jni::objects::{JClass, JObject, JString, JValue};
    use jni::sys::jobject;
    use jni::{JNIEnv, JavaVM};

    const SCHEDULER_CLASS: &str = "com.localtodo.mobile.ReminderScheduler";

    fn err<E: std::fmt::Display>(e: E) -> String {
        format!("Android 原生调用失败：{e}")
    }

    /// 统一的 JNI 调用入口：确保挂起的 Java 异常被清除，不会终止进程。
    fn with_env<T>(
        f: impl for<'a> FnOnce(&mut JNIEnv<'a>, &JObject<'a>) -> Result<T, String>,
    ) -> Result<T, String> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) }.map_err(err)?;
        let mut guard = vm.attach_current_thread().map_err(err)?;
        let env: &mut JNIEnv = &mut guard;
        env.push_local_frame(16).map_err(err)?;
        let context = unsafe { JObject::from_raw(ctx.context() as jobject) };
        let result = f(env, &context);
        let pending = env.exception_check().unwrap_or(false);
        if pending {
            let _ = env.exception_clear();
        }
        unsafe {
            let _ = env.pop_local_frame(&JObject::null());
        }
        match (result, pending) {
            (Ok(value), false) => Ok(value),
            (Err(message), false) => Err(message),
            _ => Err("Android 原生操作失败，请稍后重试".to_string()),
        }
    }

    /// 通过应用自身的 ClassLoader 加载 Kotlin 类（原生线程默认看不到应用类）。
    fn scheduler_class<'a>(
        env: &mut JNIEnv<'a>,
        context: &JObject<'a>,
    ) -> Result<JClass<'a>, String> {
        let loader = env
            .call_method(context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .and_then(|v| v.l())
            .map_err(err)?;
        let name = env.new_string(SCHEDULER_CLASS).map_err(err)?;
        let class = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )
            .and_then(|v| v.l())
            .map_err(err)?;
        if class.is_null() {
            return Err("未找到提醒调度组件".to_string());
        }
        Ok(unsafe { JClass::from_raw(class.into_raw()) })
    }

    fn java_string(env: &mut JNIEnv, value: jni::objects::JValueOwned) -> Result<String, String> {
        let obj = value.l().map_err(err)?;
        if obj.is_null() {
            return Err("原生返回值为空".to_string());
        }
        let text = unsafe { JString::from_raw(obj.into_raw()) };
        let java_string = env.get_string(&text).map_err(err)?;
        let result: String = java_string.into();
        Ok(result)
    }

    pub fn schedule(items: &[ReminderItem], active_task_ids: &[String]) -> Result<(), String> {
        let json = serde_json::to_string(items).map_err(err)?;
        let active_json = serde_json::to_string(active_task_ids).map_err(err)?;
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let text = env.new_string(&json).map_err(err)?;
            let active = env.new_string(&active_json).map_err(err)?;
            env.call_static_method(
                &class,
                "schedule",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                &[
                    JValue::Object(context),
                    JValue::Object(&text),
                    JValue::Object(&active),
                ],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    pub fn cancel_all() -> Result<(), String> {
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            env.call_static_method(
                &class,
                "cancelAll",
                "(Landroid/content/Context;)V",
                &[JValue::Object(context)],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    pub fn status() -> Result<NativeReminderStatus, String> {
        let text = with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let value = env
                .call_static_method(
                    &class,
                    "statusJson",
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(context)],
                )
                .map_err(err)?;
            java_string(env, value)
        })?;
        serde_json::from_str(&text).map_err(err)
    }

    pub fn fired() -> Result<Vec<FiredReminder>, String> {
        let text = with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let value = env
                .call_static_method(
                    &class,
                    "firedJson",
                    "(Landroid/content/Context;)Ljava/lang/String;",
                    &[JValue::Object(context)],
                )
                .map_err(err)?;
            java_string(env, value)
        })?;
        serde_json::from_str(&text).map_err(err)
    }

    pub fn consume_fired(items: &[FiredReminder]) -> Result<(), String> {
        if items.is_empty() {
            return Ok(());
        }
        let json = serde_json::to_string(items).map_err(err)?;
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let text = env.new_string(&json).map_err(err)?;
            env.call_static_method(
                &class,
                "consumeFired",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(context), JValue::Object(&text)],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    pub fn request_notifications() -> Result<bool, String> {
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            env.call_static_method(
                &class,
                "requestNotifications",
                "(Landroid/content/Context;)Z",
                &[JValue::Object(context)],
            )
            .and_then(|value| value.z())
            .map_err(err)
        })
    }

    pub fn send_test_notification(title: &str, body: &str) -> Result<bool, String> {
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let title = env.new_string(title).map_err(err)?;
            let body = env.new_string(body).map_err(err)?;
            env.call_static_method(
                &class,
                "showTestNotification",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(context),
                    JValue::Object(&title),
                    JValue::Object(&body),
                ],
            )
            .and_then(|value| value.z())
            .map_err(err)
        })
    }

    pub fn share_backup(path: &str) -> Result<(), String> {
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let path = env.new_string(path).map_err(err)?;
            env.call_static_method(
                &class,
                "shareBackup",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(context), JValue::Object(&path)],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    pub fn open_exact_alarm_settings() -> Result<(), String> {
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            env.call_static_method(
                &class,
                "openExactAlarmSettings",
                "(Landroid/content/Context;)V",
                &[JValue::Object(context)],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    pub fn open_battery_settings() -> Result<(), String> {
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            env.call_static_method(
                &class,
                "openBatterySettings",
                "(Landroid/content/Context;)V",
                &[JValue::Object(context)],
            )
            .map_err(err)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, title: &str, due: Option<(&str, &str)>, at: i64) -> ReminderRow {
        ReminderRow {
            task_id: id.into(),
            title: title.into(),
            list_name: "工作".into(),
            due_date: due.map(|(d, _)| d.to_string()),
            due_time: due.map(|(_, t)| t.to_string()),
            reminder_at: at,
        }
    }

    const NOW: i64 = 1_800_000_000_000;

    #[test]
    fn plans_authoritative_trigger_without_android_lead() {
        let rows = vec![row(
            "t1",
            "写周报",
            Some(("2026-11-26", "18:00")),
            NOW + 60_000,
        )];
        let (items, stats) = build_plan(&rows, NOW);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].trigger_at, NOW + 60_000);
        assert_eq!(items[0].body, "到期：2026-11-26 18:00 · 工作");
        assert_eq!(stats.scheduled, 1);
    }

    #[test]
    fn past_reminders_are_skipped() {
        let rows = vec![row("t1", "过期", None, NOW - 1)];
        let (items, stats) = build_plan(&rows, NOW);
        assert!(items.is_empty());
        assert_eq!(stats.skipped_past, 1);
    }

    #[test]
    fn far_future_reminders_are_registered_without_horizon() {
        let trigger_at = NOW + 10 * 365 * 86_400_000;
        let rows = vec![row("t1", "十年提醒", None, trigger_at)];
        let (items, stats) = build_plan(&rows, NOW);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].trigger_at, trigger_at);
        assert_eq!(stats.skipped_far, 0);
        assert_eq!(stats.truncated, 0);
    }

    #[test]
    fn multiple_tasks_at_same_time_are_kept() {
        let rows = vec![
            row("t1", "任务一", None, NOW + 60_000),
            row("t2", "任务二", None, NOW + 60_000),
        ];
        let (items, _) = build_plan(&rows, NOW);
        assert_eq!(items.len(), 2);
        assert_ne!(items[0].task_id, items[1].task_id);
    }

    #[test]
    fn duplicate_rows_are_deduplicated() {
        let rows = vec![
            row("t1", "任务一", None, NOW + 60_000),
            row("t1", "任务一", None, NOW + 120_000),
        ];
        let (items, _) = build_plan(&rows, NOW);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn plan_is_sorted_by_trigger_time() {
        let rows = vec![
            row("t2", "后", None, NOW + 300_000),
            row("t1", "先", None, NOW + 100_000),
        ];
        let (items, _) = build_plan(&rows, NOW);
        assert_eq!(items[0].task_id, "t1");
        assert_eq!(items[1].task_id, "t2");
    }

    #[test]
    fn plan_is_capped() {
        let rows: Vec<ReminderRow> = (0..MAX_ALARMS + 5)
            .map(|i| row(&format!("t{i}"), "任务", None, NOW + 60_000 + i as i64))
            .collect();
        let (items, stats) = build_plan(&rows, NOW);
        assert_eq!(items.len(), MAX_ALARMS);
        assert_eq!(stats.truncated, 5);
    }

    #[test]
    fn body_handles_missing_due_and_list() {
        let mut r = row("t1", "无日期", None, NOW + 60_000);
        assert_eq!(reminder_body(&r), "清单：工作");
        r.list_name = String::new();
        assert_eq!(reminder_body(&r), "任务提醒");
    }

    #[test]
    fn authoritative_candidates_exclude_fired_but_keep_notification_active() {
        let db = ltd_core::db::Db::open_memory().unwrap();
        let conn = db.lock();
        let list_id: String = conn
            .query_row("SELECT id FROM lists LIMIT 1", [], |row| row.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO tasks
             (id, list_id, title, reminder_enabled, reminder_at, created_at, updated_at)
             VALUES (?1, ?2, '权威提醒', 1, 5000, 1, 1)",
            rusqlite::params!["task-1", list_id],
        )
        .unwrap();
        assert_eq!(reminder_candidates(&conn).unwrap().len(), 1);

        conn.execute(
            "INSERT INTO reminder_schedules
             (id, task_id, trigger_at, status, fired_at, created_at)
             VALUES ('schedule-1', 'task-1', 5000, 'fired', 5000, 1)",
            [],
        )
        .unwrap();
        assert!(reminder_candidates(&conn).unwrap().is_empty());
        assert_eq!(active_reminder_task_ids(&conn).unwrap(), vec!["task-1"]);
    }
}
