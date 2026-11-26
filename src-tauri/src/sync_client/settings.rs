//! Android 本地设置。
//!
//! 这些设置只属于手机端：保存在独立的 `settings.json` 中，
//! 不进入镜像数据库，也不参与 Windows → Android 数据同步。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 提醒提前量的上限（7 天），避免异常输入
const MAX_LEAD_MINUTES: i64 = 7 * 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileSettings {
    /// Android 通知开关（仅本机，不影响 Windows 任务数据）
    pub notifications_enabled: bool,
    /// 提前多少分钟提醒，0 表示准时
    pub reminder_lead_minutes: i64,
}

impl Default for MobileSettings {
    fn default() -> Self {
        Self {
            notifications_enabled: true,
            reminder_lead_minutes: 0,
        }
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// 读取本地设置：缺失或损坏时回退默认值，不影响同步数据
pub fn load(dir: &Path) -> MobileSettings {
    std::fs::read_to_string(path(dir))
        .ok()
        .and_then(|text| serde_json::from_str::<MobileSettings>(&text).ok())
        .map(normalize)
        .unwrap_or_default()
}

/// 写入本地设置（临时文件 + rename，避免写入中断留下半个文件）
pub fn save(dir: &Path, settings: &MobileSettings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("创建设置目录失败：{e}"))?;
    let target = path(dir);
    let tmp = target.with_extension("json.tmp");
    let text = serde_json::to_vec_pretty(settings)
        .map_err(|e| format!("序列化本地设置失败：{e}"))?;
    std::fs::write(&tmp, text).map_err(|e| format!("写入本地设置失败：{e}"))?;
    if std::fs::rename(&tmp, &target).is_err() {
        let _ = std::fs::remove_file(&target);
        std::fs::rename(&tmp, &target).map_err(|e| format!("保存本地设置失败：{e}"))?;
    }
    Ok(())
}

/// 约束取值范围，防止前端传入异常值
pub fn normalize(mut settings: MobileSettings) -> MobileSettings {
    settings.reminder_lead_minutes = settings.reminder_lead_minutes.clamp(0, MAX_LEAD_MINUTES);
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ltd-mobile-settings-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_falls_back_to_default() {
        let dir = temp_dir("missing");
        assert_eq!(load(&dir), MobileSettings::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        let settings = MobileSettings {
            notifications_enabled: false,
            reminder_lead_minutes: 15,
        };
        save(&dir, &settings).unwrap();
        assert_eq!(load(&dir), settings);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_falls_back_to_default() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(path(&dir), b"{not json").unwrap();
        assert_eq!(load(&dir), MobileSettings::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lead_minutes_are_clamped() {
        let normalized = normalize(MobileSettings {
            notifications_enabled: true,
            reminder_lead_minutes: -5,
        });
        assert_eq!(normalized.reminder_lead_minutes, 0);
        let normalized = normalize(MobileSettings {
            notifications_enabled: true,
            reminder_lead_minutes: 999_999,
        });
        assert_eq!(normalized.reminder_lead_minutes, MAX_LEAD_MINUTES);
    }
}
