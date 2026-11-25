use serde::{Deserialize, Serialize};

/// 任务清单
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    pub id: String,
    pub name: String,
    pub color: String,
    pub icon: String,
    pub sort_order: i64,
    pub created_at: i64,
    pub open_count: i64,
    pub total_count: i64,
}

/// 重复规则
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recurrence {
    pub id: String,
    /// daily | weekdays | weekly | monthly | yearly | custom
    pub kind: String,
    pub interval: i64,
    /// 逗号分隔的星期数字，1=周一 .. 7=周日
    pub weekdays: String,
    pub month_day: Option<i64>,
    pub base_date: String,
    pub created_at: i64,
}

impl Recurrence {
    pub fn weekday_numbers(&self) -> Vec<u32> {
        self.weekdays
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .filter(|n| (1..=7).contains(n))
            .collect()
    }
}

/// 任务步骤（子任务）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStep {
    pub id: String,
    pub task_id: String,
    pub title: String,
    pub is_completed: bool,
    pub sort_order: i64,
}

/// 任务
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub list_id: String,
    pub title: String,
    pub note: String,
    pub is_important: bool,
    pub is_completed: bool,
    pub completed_at: Option<i64>,
    /// 本地日期 YYYY-MM-DD
    pub due_date: Option<String>,
    /// 本地时间 HH:MM
    pub due_time: Option<String>,
    /// 提前分钟数，0 表示准时，None 表示无提醒
    pub reminder_offset: Option<i64>,
    /// 该任务的提醒开关（与提醒方案相互独立）
    pub reminder_enabled: bool,
    /// 下一次提醒触发的 UTC 毫秒时间戳
    pub reminder_at: Option<i64>,
    pub recurrence_id: Option<String>,
    pub series_id: Option<String>,
    pub parent_task_id: Option<String>,
    /// 重复任务已重复次数（每次推进周期 +1）
    pub recurrence_count: i64,
    /// 加入「我的一天」的日期 YYYY-MM-DD
    pub my_day: Option<String>,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
    /// 软删除时间（最近删除）
    pub deleted_at: Option<i64>,
    pub steps: Vec<TaskStep>,
    pub recurrence: Option<Recurrence>,
    pub list_name: String,
    pub list_color: String,
}

/// 任务查询范围
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    /// my_day | important | planned | all | completed | list
    pub view: String,
    pub list_id: Option<String>,
    pub search: Option<String>,
    pub include_completed: Option<bool>,
}

/// 独立笔记
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 操作历史
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub task_id: String,
    pub task_title: String,
    pub action: String,
    pub detail: String,
    pub at: i64,
}

/// 最近删除中的清单
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedList {
    pub id: String,
    pub name: String,
    pub color: String,
    pub sort_order: i64,
    pub deleted_at: Option<i64>,
    pub task_count: i64,
}

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String,
    pub notifications_enabled: bool,
    pub sound_enabled: bool,
    /// 自定义提示音文件路径（仅支持 WAV），为空表示使用默认系统提示音
    pub sound_path: Option<String>,
    pub close_to_tray: bool,
    pub autostart: bool,
    pub start_minimized: bool,
    pub auto_backup_enabled: bool,
    pub auto_backup_interval_hours: i64,
    pub last_backup_at: Option<i64>,
    pub default_list_id: Option<String>,
    /// 自动跟随 Windows 系统时区
    pub timezone_auto: bool,
    /// 手动时区（IANA 名称，如 Asia/Shanghai），自动模式下忽略
    pub timezone: Option<String>,
    /// 周数基准日期（YYYY-MM-DD），所在自然周算第 1 周；None 表示用当年 1 月 1 日
    pub week_base_date: Option<String>,
    /// 用户是否已自定义周数基准。false 时跨年自动重置为新年 1 月 1 日
    pub week_customized: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            notifications_enabled: true,
            sound_enabled: true,
            sound_path: None,
            close_to_tray: true,
            autostart: false,
            start_minimized: false,
            auto_backup_enabled: true,
            auto_backup_interval_hours: 24,
            last_backup_at: None,
            default_list_id: None,
            timezone_auto: true,
            timezone: None,
            week_base_date: None,
            week_customized: false,
        }
    }
}

/// 应用路径信息（设置页展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub db_path: String,
    pub backup_dir: String,
    pub log_dir: String,
    pub today: String,
    pub now: i64,
}

/// 待触发提醒（调度器内部使用）
#[derive(Debug, Clone)]
pub struct PendingReminder {
    pub schedule_id: String,
    pub task_id: String,
    pub title: String,
    pub list_name: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub trigger_at: i64,
    pub is_completed: bool,
    pub is_deleted: bool,
    /// 任务的提醒时间已被改动，此条计划已过期
    pub reminder_changed: bool,
}

/// 备份记录
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub path: String,
    pub file_name: String,
    pub size: i64,
    pub created_at: i64,
    pub format: String,
}
