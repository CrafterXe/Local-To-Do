use chrono::{Datelike, Duration, NaiveDate};

/// 解析 YYYY-MM-DD 本地日期
pub fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()
}

/// 某年某月的天数
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("合法月份");
    (first_next - Duration::days(1)).day()
}

/// 重复规则（与 UI、数据库解耦的纯逻辑）
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// daily | weekdays | weekly | monthly | yearly | custom
    pub kind: String,
    pub interval: i64,
    /// 1=周一 .. 7=周日，空表示沿用基准日期星期
    pub weekdays: Vec<u32>,
    pub month_day: Option<u32>,
    pub base_date: NaiveDate,
}

impl Rule {
    pub fn new(
        kind: &str,
        interval: i64,
        weekdays: Vec<u32>,
        month_day: Option<u32>,
        base_date: NaiveDate,
    ) -> Self {
        Self {
            kind: kind.to_lowercase(),
            interval: interval.max(1),
            weekdays,
            month_day,
            base_date,
        }
    }

    fn month_target_day(&self, year: i32, month: u32) -> u32 {
        let target = self.month_day.unwrap_or(self.base_date.day());
        target.min(days_in_month(year, month))
    }

    /// 判断某个日期是否命中重复规则
    pub fn matches(&self, d: NaiveDate) -> bool {
        if d < self.base_date {
            return false;
        }
        let days = (d - self.base_date).num_days();
        match self.kind.as_str() {
            "daily" => days % self.interval == 0,
            "weekdays" => d.weekday().num_days_from_monday() < 5,
            "weekly" => {
                let base_monday = self
                    .base_date
                    .checked_sub_signed(Duration::days(
                        self.base_date.weekday().num_days_from_monday() as i64,
                    ))
                    .unwrap_or(self.base_date);
                let d_monday = d
                    .checked_sub_signed(Duration::days(d.weekday().num_days_from_monday() as i64))
                    .unwrap_or(d);
                let weeks = (d_monday - base_monday).num_days() / 7;
                if weeks < 0 || weeks % self.interval != 0 {
                    return false;
                }
                let wd = d.weekday().number_from_monday();
                if self.weekdays.is_empty() {
                    wd == self.base_date.weekday().number_from_monday()
                } else {
                    self.weekdays.contains(&wd)
                }
            }
            "monthly" => {
                let months = (d.year() as i64 - self.base_date.year() as i64) * 12
                    + d.month() as i64
                    - self.base_date.month() as i64;
                months >= 0
                    && months % self.interval == 0
                    && d.day() == self.month_target_day(d.year(), d.month())
            }
            "yearly" => {
                let years = d.year() - self.base_date.year();
                let months = years as i64 * 12 + d.month() as i64 - self.base_date.month() as i64;
                months >= 0
                    && months % (self.interval * 12) == 0
                    && d.month() == self.base_date.month()
                    && d.day() == self.month_target_day(d.year(), d.month())
            }
            // 自定义：星期集合优先，其次月内日期，否则按天间隔
            "custom" => {
                if !self.weekdays.is_empty() {
                    let mut weekly = self.clone();
                    weekly.kind = "weekly".into();
                    weekly.matches(d)
                } else if self.month_day.is_some() {
                    let mut monthly = self.clone();
                    monthly.kind = "monthly".into();
                    monthly.matches(d)
                } else {
                    days % self.interval == 0
                }
            }
            _ => false,
        }
    }

    /// 返回严格晚于 `after` 的下一次发生日期
    pub fn next_after(&self, after: NaiveDate) -> Option<NaiveDate> {
        let mut d = after + Duration::days(1);
        for _ in 0..7320 {
            if self.matches(d) {
                return Some(d);
            }
            d += Duration::days(1);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        parse_date(s).unwrap()
    }

    #[test]
    fn daily_interval_two() {
        let r = Rule::new("daily", 2, vec![], None, d("2026-01-01"));
        assert_eq!(r.next_after(d("2026-01-01")), Some(d("2026-01-03")));
        assert_eq!(r.next_after(d("2026-01-02")), Some(d("2026-01-03")));
        assert!(r.matches(d("2026-01-05")));
        assert!(!r.matches(d("2026-01-06")));
    }

    #[test]
    fn weekdays_skips_weekend() {
        // 2026-11-20 是周五
        let r = Rule::new("weekdays", 1, vec![], None, d("2026-11-02"));
        assert_eq!(r.next_after(d("2026-11-20")), Some(d("2026-11-23")));
        assert!(!r.matches(d("2026-11-21")));
        assert!(!r.matches(d("2026-11-22")));
    }

    #[test]
    fn weekly_mon_wed_fri() {
        // 2026-11-18 周三
        let r = Rule::new("weekly", 1, vec![1, 3, 5], None, d("2026-11-16"));
        assert_eq!(r.next_after(d("2026-11-18")), Some(d("2026-11-20")));
        assert_eq!(r.next_after(d("2026-11-20")), Some(d("2026-11-23")));
    }

    #[test]
    fn monthly_31_clamps_to_february() {
        let r = Rule::new("monthly", 1, vec![], Some(31), d("2026-01-31"));
        assert_eq!(r.next_after(d("2026-01-31")), Some(d("2026-02-28")));
        // 闰年 2 月 29 日
        let leap = Rule::new("monthly", 1, vec![], Some(31), d("2024-01-31"));
        assert_eq!(leap.next_after(d("2024-01-31")), Some(d("2024-02-29")));
        // 4 月只有 30 天
        assert_eq!(r.next_after(d("2026-03-31")), Some(d("2026-04-30")));
    }

    #[test]
    fn yearly_leap_day() {
        let r = Rule::new("yearly", 1, vec![], None, d("2024-02-29"));
        assert_eq!(r.next_after(d("2024-02-29")), Some(d("2025-02-28")));
        assert_eq!(r.next_after(d("2027-02-28")), Some(d("2028-02-29")));
    }

    #[test]
    fn monthly_interval_two() {
        let r = Rule::new("monthly", 2, vec![], Some(15), d("2026-01-15"));
        assert_eq!(r.next_after(d("2026-01-15")), Some(d("2026-03-15")));
    }

    #[test]
    fn weekly_interval_two() {
        let r = Rule::new("weekly", 2, vec![1], None, d("2026-11-02"));
        assert_eq!(r.next_after(d("2026-11-02")), Some(d("2026-11-16")));
    }

    #[test]
    fn custom_weekdays_and_month_day() {
        let r = Rule::new("custom", 1, vec![1, 3, 5], None, d("2026-11-16"));
        assert_eq!(r.next_after(d("2026-11-16")), Some(d("2026-11-18")));

        let m = Rule::new("custom", 1, vec![], Some(15), d("2026-01-15"));
        assert_eq!(m.next_after(d("2026-01-15")), Some(d("2026-02-15")));
    }

    #[test]
    fn days_in_month_check() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }
}
