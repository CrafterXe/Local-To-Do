//! Android 后台提醒调度。
//!
//! 设计要点：
//! - 只读取同步镜像中由 Windows 端计算好的 `reminder_at`，不重新实现重复规则；
//! - 提前量属于 Android 本地设置，仅在本机减去，不写回任何任务数据；
//! - 具体系统闹钟注册、通知展示与开机恢复由 Android 原生层
//!   `com.localtodo.desktop.ReminderScheduler` 完成，本模块只负责生成计划。
//!
//! 该模块在桌面端同样参与编译，用于单元测试计划生成逻辑；
//! 原生调用在非 Android 平台返回“不支持”。

use ltd_core::mirror_read::MirrorReminderRow;
use serde::{Deserialize, Serialize};

/// 最多提前注册 60 天内的提醒，避免无效闹钟堆积
pub const MAX_HORIZON_DAYS: i64 = 60;
/// 单次最多注册的闹钟数量
pub const MAX_ALARMS: usize = 200;
/// 提前量上限（7 天），与本地设置保持一致
pub const MAX_LEAD_MINUTES: i64 = 7 * 24 * 60;

const MINUTE_MS: i64 = 60_000;
const DAY_MS: i64 = 86_400_000;

/// 一条待注册的本地提醒
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderItem {
    pub task_id: String,
    pub title: String,
    pub body: String,
    /// 实际触发时间（UTC 毫秒），已扣除本地提前量
    pub trigger_at: i64,
}

/// 计划生成统计，便于排查与展示
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStats {
    pub scheduled: usize,
    pub skipped_past: usize,
    pub skipped_far: usize,
    pub truncated: usize,
}

/// 原生层上报的状态
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeReminderStatus {
    #[serde(default)]
    pub exact_alarm_allowed: bool,
    #[serde(default)]
    pub notifications_allowed: bool,
    /// 是否已加入电池优化白名单（部分厂商熄屏后会清理未加入白名单的应用）
    #[serde(default)]
    pub ignoring_battery_optimizations: bool,
    #[serde(default)]
    pub scheduled: usize,
}

/// 根据镜像行生成本地提醒计划。
///
/// 规则：
/// - 触发时间 = Windows 计算的 `reminder_at` - 本地提前量；
/// - 已经过去的提醒直接跳过，避免重新调度时立刻补发；
/// - 超过时间上限的提醒暂不注册，等后续同步或打开应用时再滚动注册。
pub fn build_plan(
    rows: &[MirrorReminderRow],
    lead_minutes: i64,
    now_ms: i64,
) -> (Vec<ReminderItem>, PlanStats) {
    let lead_ms = lead_minutes.clamp(0, MAX_LEAD_MINUTES) * MINUTE_MS;
    let horizon = now_ms + MAX_HORIZON_DAYS * DAY_MS;
    let mut stats = PlanStats::default();
    let mut items: Vec<ReminderItem> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

    for row in rows {
        if !seen.insert(row.task_id.as_str()) {
            continue;
        }
        let trigger_at = row.reminder_at - lead_ms;
        if trigger_at <= now_ms {
            stats.skipped_past += 1;
            continue;
        }
        if trigger_at > horizon {
            stats.skipped_far += 1;
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

/// 通知正文：到期时间 + 清单名，不包含备注等敏感内容
fn reminder_body(row: &MirrorReminderRow) -> String {
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

pub fn schedule(items: &[ReminderItem]) -> Result<(), String> {
    native::schedule(items)
}

pub fn cancel_all() -> Result<(), String> {
    native::cancel_all()
}

pub fn status() -> Result<NativeReminderStatus, String> {
    native::status()
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

    pub fn schedule(_items: &[ReminderItem]) -> Result<(), String> {
        Err("后台提醒仅在 Android 端可用".to_string())
    }

    pub fn cancel_all() -> Result<(), String> {
        Ok(())
    }

    pub fn status() -> Result<NativeReminderStatus, String> {
        Ok(NativeReminderStatus::default())
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

    const SCHEDULER_CLASS: &str = "com.localtodo.desktop.ReminderScheduler";

    fn err<E: std::fmt::Display>(e: E) -> String {
        format!("提醒调度调用失败：{e}")
    }

    /// 统一的 JNI 调用入口：确保挂起的 Java 异常被清除，不会终止进程
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
            _ => Err("提醒调度失败，请稍后重试".to_string()),
        }
    }

    /// 通过应用自身的 ClassLoader 加载 Kotlin 类（原生线程默认看不到应用类）
    fn scheduler_class<'a>(
        env: &mut JNIEnv<'a>,
        context: &JObject<'a>,
    ) -> Result<JClass<'a>, String> {
        let loader = env
            .call_method(
                context,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
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

    pub fn schedule(items: &[ReminderItem]) -> Result<(), String> {
        let json = serde_json::to_string(items).map_err(err)?;
        with_env(|env, context| {
            let class = scheduler_class(env, context)?;
            let text = env.new_string(&json).map_err(err)?;
            env.call_static_method(
                &class,
                "schedule",
                "(Landroid/content/Context;Ljava/lang/String;)V",
                &[JValue::Object(context), JValue::Object(&text)],
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
            let obj = value.l().map_err(err)?;
            if obj.is_null() {
                return Err("提醒状态不可用".to_string());
            }
            let text = unsafe { JString::from_raw(obj.into_raw()) };
            let text: String = env.get_string(&text).map_err(err)?.into();
            Ok(text)
        })?;
        serde_json::from_str(&text).map_err(err)
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

    fn row(id: &str, title: &str, due: Option<(&str, &str)>, at: i64) -> MirrorReminderRow {
        MirrorReminderRow {
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
    fn plans_future_reminder_without_lead() {
        let rows = vec![row("t1", "写周报", Some(("2026-11-26", "18:00")), NOW + 60_000)];
        let (items, stats) = build_plan(&rows, 0, NOW);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].trigger_at, NOW + 60_000);
        assert_eq!(items[0].body, "到期：2026-11-26 18:00 · 工作");
        assert_eq!(stats.scheduled, 1);
    }

    #[test]
    fn lead_minutes_move_trigger_earlier() {
        let rows = vec![row("t1", "开会", None, NOW + 30 * 60_000)];
        let (items, _) = build_plan(&rows, 10, NOW);
        assert_eq!(items[0].trigger_at, NOW + 20 * 60_000);
    }

    #[test]
    fn past_reminders_are_skipped() {
        let rows = vec![row("t1", "过期", None, NOW - 1)];
        let (items, stats) = build_plan(&rows, 0, NOW);
        assert!(items.is_empty());
        assert_eq!(stats.skipped_past, 1);
    }

    #[test]
    fn lead_can_push_future_reminder_into_past() {
        let rows = vec![row("t1", "即将开始", None, NOW + 5 * 60_000)];
        let (items, stats) = build_plan(&rows, 15, NOW);
        assert!(items.is_empty());
        assert_eq!(stats.skipped_past, 1);
    }

    #[test]
    fn far_future_reminders_are_deferred() {
        let rows = vec![row(
            "t1",
            "很久以后",
            None,
            NOW + (MAX_HORIZON_DAYS + 1) * DAY_MS,
        )];
        let (items, stats) = build_plan(&rows, 0, NOW);
        assert!(items.is_empty());
        assert_eq!(stats.skipped_far, 1);
    }

    #[test]
    fn multiple_tasks_at_same_time_are_kept() {
        let rows = vec![
            row("t1", "任务一", None, NOW + 60_000),
            row("t2", "任务二", None, NOW + 60_000),
        ];
        let (items, _) = build_plan(&rows, 0, NOW);
        assert_eq!(items.len(), 2);
        assert_ne!(items[0].task_id, items[1].task_id);
    }

    #[test]
    fn duplicate_rows_are_deduplicated() {
        let rows = vec![
            row("t1", "任务一", None, NOW + 60_000),
            row("t1", "任务一", None, NOW + 120_000),
        ];
        let (items, _) = build_plan(&rows, 0, NOW);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn plan_is_sorted_by_trigger_time() {
        let rows = vec![
            row("t2", "后", None, NOW + 300_000),
            row("t1", "先", None, NOW + 100_000),
        ];
        let (items, _) = build_plan(&rows, 0, NOW);
        assert_eq!(items[0].task_id, "t1");
        assert_eq!(items[1].task_id, "t2");
    }

    #[test]
    fn plan_is_capped() {
        let rows: Vec<MirrorReminderRow> = (0..MAX_ALARMS + 5)
            .map(|i| row(&format!("t{i}"), "任务", None, NOW + 60_000 + i as i64))
            .collect();
        let (items, stats) = build_plan(&rows, 0, NOW);
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
}
