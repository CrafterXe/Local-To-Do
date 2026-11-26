use chrono::{DateTime, NaiveDate};
use ltd_core::domain::{
    compute_reminder_at_in_timezone, current_timezone, parse_date, set_timezone, Rule,
};
use ltd_core::models::{AppSettings, Task};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenVectors {
    recurrences: Vec<RecurrenceVector>,
    reminders: Vec<ReminderVector>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecurrenceVector {
    name: String,
    rule: RuleVector,
    occurrences: Vec<String>,
    nth_checks: Vec<NthVector>,
    next_after: Vec<NextAfterVector>,
    index_of: Vec<IndexVector>,
    non_matches: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleVector {
    kind: String,
    interval: i64,
    weekdays: Vec<u32>,
    month_day: Option<u32>,
    base_date: String,
}

#[derive(Deserialize)]
struct NthVector {
    n: i64,
    expected: Option<String>,
}

#[derive(Deserialize)]
struct NextAfterVector {
    after: String,
    expected: Option<String>,
}

#[derive(Deserialize)]
struct IndexVector {
    target: String,
    expected: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReminderVector {
    name: String,
    timezone: String,
    due_date: Option<String>,
    due_time: Option<String>,
    offset_minutes: Option<i64>,
    expected_utc: Option<String>,
}

fn vectors() -> GoldenVectors {
    serde_json::from_str(include_str!("fixtures/core_golden.json"))
        .expect("黄金向量必须是有效 JSON")
}

fn date(value: &str) -> NaiveDate {
    parse_date(value).unwrap_or_else(|| panic!("非法测试日期: {value}"))
}

fn optional_date(value: &Option<String>) -> Option<NaiveDate> {
    value.as_deref().map(date)
}

#[test]
fn recurrence_golden_vectors_match_windows_core_behavior() {
    let vectors = vectors();
    assert!(vectors.recurrences.len() >= 12, "黄金向量覆盖不完整");

    for case in vectors.recurrences {
        let rule = Rule::new(
            &case.rule.kind,
            case.rule.interval,
            case.rule.weekdays,
            case.rule.month_day,
            date(&case.rule.base_date),
        );

        for (index, expected) in case.occurrences.iter().enumerate() {
            assert_eq!(
                rule.nth(index as i64).as_ref(),
                Some(&date(expected)),
                "{}: 第 {} 次发生日期",
                case.name,
                index
            );
        }
        for check in case.nth_checks {
            assert_eq!(
                rule.nth(check.n),
                optional_date(&check.expected),
                "{}: nth({})",
                case.name,
                check.n
            );
        }
        for check in case.next_after {
            assert_eq!(
                rule.next_after(date(&check.after)),
                optional_date(&check.expected),
                "{}: next_after({})",
                case.name,
                check.after
            );
        }
        for check in case.index_of {
            assert_eq!(
                rule.index_of(date(&check.target)),
                check.expected,
                "{}: index_of({})",
                case.name,
                check.target
            );
        }
        for value in case.non_matches {
            assert!(
                !rule.matches(date(&value)),
                "{}: 不应命中 {}",
                case.name,
                value
            );
        }
        assert!(
            !rule.matches(date("1900-01-01")),
            "{}: 不应命中基准日前",
            case.name
        );
        assert_eq!(rule.nth(-1), None, "{}: 负数次数必须返回 None", case.name);
    }
}

#[test]
fn reminder_golden_vectors_are_timezone_deterministic() {
    let vectors = vectors();
    assert!(vectors.reminders.len() >= 15, "提醒黄金向量覆盖不完整");

    for case in vectors.reminders {
        let timezone = case
            .timezone
            .parse()
            .unwrap_or_else(|_| panic!("非法测试时区: {}", case.timezone));
        let actual = compute_reminder_at_in_timezone(
            case.due_date.as_deref(),
            case.due_time.as_deref(),
            case.offset_minutes,
            timezone,
        );
        let expected = case.expected_utc.as_ref().map(|value| {
            DateTime::parse_from_rfc3339(value)
                .unwrap_or_else(|_| panic!("非法期望 UTC 时间: {value}"))
                .timestamp_millis()
        });
        assert_eq!(actual, expected, "{}", case.name);
    }
}

#[test]
fn explicit_timezone_calculation_does_not_mutate_process_timezone() {
    set_timezone(Some("Asia/Tokyo".into()));
    let actual = compute_reminder_at_in_timezone(
        Some("2026-11-01"),
        Some("01:30"),
        Some(0),
        chrono_tz::America::New_York,
    );
    let expected = DateTime::parse_from_rfc3339("2026-11-01T05:30:00Z")
        .unwrap()
        .timestamp_millis();
    assert_eq!(actual, Some(expected));
    assert_eq!(current_timezone().as_deref(), Some("Asia/Tokyo"));
    set_timezone(None);
}

#[test]
fn task_and_settings_json_roundtrip_preserves_camel_case_contract() {
    let task_json = json!({
        "id": "task-1",
        "listId": "list-1",
        "title": "黄金任务",
        "note": "备注",
        "isImportant": true,
        "isCompleted": false,
        "completedAt": null,
        "dueDate": "2028-02-29",
        "dueTime": "09:30",
        "reminderOffset": 45,
        "reminderEnabled": true,
        "reminderAt": 1_800_000_000_000_i64,
        "recurrenceId": "rec-1",
        "seriesId": "task-1",
        "parentTaskId": null,
        "recurrenceCount": 7,
        "myDay": "2028-02-29",
        "sortOrder": 3,
        "createdAt": 1000,
        "updatedAt": 2000,
        "deletedAt": null,
        "steps": [{
            "id": "step-1",
            "taskId": "task-1",
            "title": "子步骤",
            "isCompleted": true,
            "sortOrder": 0
        }],
        "recurrence": {
            "id": "rec-1",
            "kind": "yearly",
            "interval": 1,
            "weekdays": "",
            "monthDay": null,
            "baseDate": "2024-02-29",
            "createdAt": 500
        },
        "listName": "工作",
        "listColor": "#2f6fed"
    });
    let task: Task = serde_json::from_value(task_json.clone()).expect("任务 JSON 反序列化");
    let roundtrip = serde_json::to_value(task).expect("任务 JSON 序列化");
    assert_eq!(roundtrip, task_json);
    assert!(roundtrip.get("list_id").is_none());
    assert!(roundtrip.get("recurrenceCount").is_some());

    let settings_json = json!({
        "theme": "dark",
        "notificationsEnabled": true,
        "soundEnabled": true,
        "soundPath": null,
        "closeToTray": true,
        "autostart": false,
        "startMinimized": false,
        "autoBackupEnabled": true,
        "autoBackupIntervalHours": 24,
        "lastBackupAt": null,
        "defaultListId": null,
        "timezoneAuto": true,
        "timezone": null,
        "weekBaseDate": null,
        "weekCustomized": false
    });
    let settings: AppSettings =
        serde_json::from_value(settings_json.clone()).expect("设置反序列化");
    let serialized: Value = serde_json::to_value(settings).expect("设置序列化");
    assert_eq!(serialized, settings_json);
}
