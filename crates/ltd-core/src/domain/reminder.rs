use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use std::sync::RwLock;

use super::recurrence::parse_date;
use crate::models::PendingReminder;

/// 超过该时长未触发的提醒视为错过，与 Windows 调度器保持一致。
pub const REMINDER_MISSED_GRACE_MS: i64 = 24 * 3600 * 1000;
/// 超过该时长触发时，通知正文标记为延迟提醒。
pub const REMINDER_LATE_MS: i64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderDisposition {
    Cancel,
    Missed,
    Fire,
}

/// 手动时区（IANA 名称）。为空表示自动跟随 Windows 系统时区。
static MANUAL_TZ: RwLock<Option<String>> = RwLock::new(None);

/// 设置当前生效的时区。传入 None 表示跟随系统。
pub fn set_timezone(name: Option<String>) {
    let normalized = name.and_then(|n| if is_valid_timezone(&n) { Some(n) } else { None });
    if let Ok(mut guard) = MANUAL_TZ.write() {
        *guard = normalized;
    }
}

/// 当前手动时区（None = 自动跟随系统）
pub fn current_timezone() -> Option<String> {
    MANUAL_TZ.read().ok().and_then(|g| g.clone())
}

pub fn is_valid_timezone(name: &str) -> bool {
    name.parse::<Tz>().is_ok()
}

/// 当前时间（UTC 毫秒时间戳），始终来自系统时钟
pub fn local_now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn manual_tz() -> Option<Tz> {
    current_timezone().and_then(|n| n.parse::<Tz>().ok())
}

/// 今天的日期字符串（按当前生效时区）
pub fn today_string() -> String {
    match manual_tz() {
        Some(tz) => Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string(),
        None => Local::now().format("%Y-%m-%d").to_string(),
    }
}

pub fn now_date() -> NaiveDate {
    match manual_tz() {
        Some(tz) => Utc::now().with_timezone(&tz).date_naive(),
        None => Local::now().date_naive(),
    }
}

fn wall_clock_ms_in_timezone(date: NaiveDate, time: NaiveTime, timezone: Tz) -> Option<i64> {
    let naive: NaiveDateTime = date.and_time(time);
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt.timestamp_millis()),
        // 夏令时回拨导致同一本地时间出现两次：取较早的一次
        chrono::LocalResult::Ambiguous(early, _) => Some(early.timestamp_millis()),
        // 夏令时前拨导致该本地时间不存在：视为无有效触发时刻
        chrono::LocalResult::None => None,
    }
}

/// 把「到期日期 + 到期时间」换算成 UTC 毫秒时间戳。
fn wall_clock_ms(date: NaiveDate, time: NaiveTime) -> Option<i64> {
    let naive: NaiveDateTime = date.and_time(time);
    if let Some(tz) = manual_tz() {
        return wall_clock_ms_in_timezone(date, time, tz);
    }
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt.timestamp_millis()),
        chrono::LocalResult::Ambiguous(dt, _) => Some(dt.timestamp_millis()),
        chrono::LocalResult::None => None,
    }
}

fn reminder_inputs(
    due_date: Option<&str>,
    due_time: Option<&str>,
    offset_minutes: Option<i64>,
) -> Option<(NaiveDate, NaiveTime, i64)> {
    let date = parse_date(due_date?)?;
    let offset = offset_minutes?;
    let time = due_time
        .and_then(|t| NaiveTime::parse_from_str(t.trim(), "%H:%M").ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).expect("09:00 合法"));
    Some((date, time, offset.max(0)))
}

/// 在明确指定的 IANA 时区中计算提醒，不读取或修改进程级时区状态。
/// 该纯函数是 Windows 与 Android 黄金测试及跨平台调用的共同入口。
pub fn compute_reminder_at_in_timezone(
    due_date: Option<&str>,
    due_time: Option<&str>,
    offset_minutes: Option<i64>,
    timezone: Tz,
) -> Option<i64> {
    let (date, time, offset) = reminder_inputs(due_date, due_time, offset_minutes)?;
    let base_ms = wall_clock_ms_in_timezone(date, time, timezone)?;
    Some(base_ms - offset * 60_000)
}

/// 计算提醒触发时间（UTC 毫秒）。
///
/// 规则：提醒基于「到期日期 + 到期时间」减去提前量；没有日期则没有提醒。
/// 没有设置具体时间时默认 09:00（与 Microsoft To Do 的提醒必须有时间的设定一致）。
pub fn compute_reminder_at(
    due_date: Option<&str>,
    due_time: Option<&str>,
    offset_minutes: Option<i64>,
) -> Option<i64> {
    let (date, time, offset) = reminder_inputs(due_date, due_time, offset_minutes)?;
    let base_ms = wall_clock_ms(date, time)?;
    Some(base_ms - offset * 60_000)
}

/// 对一条已经到期的计划作出与 Windows 调度器相同的处置判断。
pub fn reminder_disposition(reminder: &PendingReminder, now: i64) -> ReminderDisposition {
    if reminder.is_completed || reminder.is_deleted || reminder.reminder_changed {
        ReminderDisposition::Cancel
    } else if now - reminder.trigger_at > REMINDER_MISSED_GRACE_MS {
        ReminderDisposition::Missed
    } else {
        ReminderDisposition::Fire
    }
}

pub fn reminder_is_late(trigger_at: i64, now: i64) -> bool {
    now - trigger_at > REMINDER_LATE_MS
}

/// 把毫秒时间戳格式化为当前生效时区的可读文本
pub fn format_trigger(ms: i64) -> String {
    if let Some(tz) = manual_tz() {
        let dt = Utc.timestamp_millis_opt(ms);
        return match dt {
            chrono::LocalResult::Single(dt) => {
                dt.with_timezone(&tz).format("%Y-%m-%d %H:%M").to_string()
            }
            _ => String::from("--"),
        };
    }
    match Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        _ => String::from("--"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};
    use std::sync::Mutex;

    /// 全局时区是进程级状态，测试并行运行时需要串行化，避免相互干扰
    static TZ_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_tz() -> std::sync::MutexGuard<'static, ()> {
        let guard = TZ_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_timezone(None);
        guard
    }

    #[test]
    fn reminder_without_date_is_none() {
        let _g = lock_tz();
        assert_eq!(compute_reminder_at(None, Some("09:00"), Some(0)), None);
    }

    #[test]
    fn reminder_at_due_time() {
        let _g = lock_tz();
        let ms = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(0)).unwrap();
        let dt = Local.timestamp_millis_opt(ms).unwrap();
        assert_eq!(dt.hour(), 20);
        assert_eq!(dt.minute(), 30);
        assert_eq!(dt.day(), 23);
    }

    #[test]
    fn reminder_before_due_time() {
        let _g = lock_tz();
        let base = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(0)).unwrap();
        let before = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(30)).unwrap();
        assert_eq!(base - before, 30 * 60_000);
    }

    #[test]
    fn reminder_uses_default_nine_when_no_time() {
        let _g = lock_tz();
        let ms = compute_reminder_at(Some("2026-11-23"), None, Some(0)).unwrap();
        let dt = Local.timestamp_millis_opt(ms).unwrap();
        assert_eq!(dt.hour(), 9);
        assert_eq!(dt.minute(), 0);
    }

    #[test]
    fn manual_timezone_affects_wall_clock() {
        let _g = lock_tz();
        set_timezone(Some("Asia/Tokyo".into()));
        let ms = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(0)).unwrap();
        let tokyo = Utc
            .timestamp_millis_opt(ms)
            .unwrap()
            .with_timezone(&chrono_tz::Asia::Tokyo);
        assert_eq!(tokyo.hour(), 20);
        assert_eq!(tokyo.minute(), 30);
        assert_eq!(format_trigger(ms), "2026-11-23 20:30");
        set_timezone(None);
    }

    #[test]
    fn invalid_timezone_falls_back_to_system() {
        let _g = lock_tz();
        set_timezone(Some("Not/AZone".into()));
        assert_eq!(current_timezone(), None);
        assert!(!is_valid_timezone("Not/AZone"));
    }

    #[test]
    fn dst_gap_returns_none_and_ambiguous_picks_earlier() {
        let _g = lock_tz();
        set_timezone(Some("America/New_York".into()));
        // 2026-03-08 02:30 处于夏令时跳变缺口，不存在
        assert_eq!(
            compute_reminder_at(Some("2026-03-08"), Some("02:30"), Some(0)),
            None
        );
        // 2026-11-01 01:30 出现两次，取较早的 EDT（UTC-4）
        let ms = compute_reminder_at(Some("2026-11-01"), Some("01:30"), Some(0)).unwrap();
        assert_eq!(format_trigger(ms), "2026-11-01 01:30");
        let utc = Utc.timestamp_millis_opt(ms).unwrap();
        assert_eq!(utc.hour(), 5);
        assert_eq!(utc.minute(), 30);
        set_timezone(None);
    }
}
