use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use super::recurrence::parse_date;

/// 当前本地时间（UTC 毫秒时间戳）
pub fn local_now_ms() -> i64 {
    Local::now().timestamp_millis()
}

/// 今天的本地日期字符串
pub fn today_string() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn now_date() -> NaiveDate {
    Local::now().date_naive()
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
    let date = parse_date(due_date?)?;
    let offset = offset_minutes?;
    let time = due_time
        .and_then(|t| NaiveTime::parse_from_str(t.trim(), "%H:%M").ok())
        .unwrap_or_else(|| NaiveTime::from_hms_opt(9, 0, 0).expect("09:00 合法"));

    let naive: NaiveDateTime = date.and_time(time);
    let base_ms = match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt.timestamp_millis(),
        chrono::LocalResult::Ambiguous(dt, _) => dt.timestamp_millis(),
        chrono::LocalResult::None => return None,
    };
    Some(base_ms - offset.max(0) * 60_000)
}

/// 把毫秒时间戳格式化为本地可读文本
pub fn format_trigger(ms: i64) -> String {
    match Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        _ => String::from("--"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    #[test]
    fn reminder_without_date_is_none() {
        assert_eq!(compute_reminder_at(None, Some("09:00"), Some(0)), None);
    }

    #[test]
    fn reminder_at_due_time() {
        let ms = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(0)).unwrap();
        let dt = Local.timestamp_millis_opt(ms).unwrap();
        assert_eq!(dt.hour(), 20);
        assert_eq!(dt.minute(), 30);
        assert_eq!(dt.day(), 23);
    }

    #[test]
    fn reminder_before_due_time() {
        let base = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(0)).unwrap();
        let before = compute_reminder_at(Some("2026-11-23"), Some("20:30"), Some(30)).unwrap();
        assert_eq!(base - before, 30 * 60_000);
    }

    #[test]
    fn reminder_uses_default_nine_when_no_time() {
        let ms = compute_reminder_at(Some("2026-11-23"), None, Some(0)).unwrap();
        let dt = Local.timestamp_millis_opt(ms).unwrap();
        assert_eq!(dt.hour(), 9);
        assert_eq!(dt.minute(), 0);
    }
}
