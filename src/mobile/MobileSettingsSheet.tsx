import { useEffect, useState } from "react";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

import { api, type MobileSettings, type ReminderStatus } from "../api";
import { IconBell, IconX } from "../components/Icons";
import { errorText } from "./mobileUtils";
import { MobileGuideSheet } from "./MobileGuideSheet";

const LEAD_OPTIONS = [
  { value: 0, label: "准时" },
  { value: 5, label: "提前 5 分钟" },
  { value: 10, label: "提前 10 分钟" },
  { value: 15, label: "提前 15 分钟" },
  { value: 30, label: "提前 30 分钟" },
  { value: 60, label: "提前 1 小时" },
  { value: 1440, label: "提前 1 天" },
];

interface MobileSettingsSheetProps {
  onClose: () => void;
}

/**
 * Android 本地设置：只保存到手机本地文件，不写入任务表，也不参与同步。
 * 提醒调度由原生闹钟完成，这里只负责开关、提前量与权限状态展示。
 */
export function MobileSettingsSheet({ onClose }: MobileSettingsSheetProps) {
  const [settings, setSettings] = useState<MobileSettings | null>(null);
  const [reminder, setReminder] = useState<ReminderStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [guideOpen, setGuideOpen] = useState(false);

  const refreshStatus = async () => {
    try {
      setReminder(await api.mobileReminderStatus());
    } catch {
      // 状态读取失败不影响设置项本身
    }
  };

  useEffect(() => {
    let alive = true;
    api
      .mobileSettingsGet()
      .then((value) => {
        if (alive) setSettings(value);
      })
      .catch((e) => alive && setMessage(errorText(e)));
    void refreshStatus();
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const apply = async (patch: Partial<MobileSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      setSettings(await api.mobileSettingsUpdate(next));
      setMessage(null);
      await refreshStatus();
    } catch (e) {
      setMessage(errorText(e));
    }
  };

  const resync = async () => {
    try {
      setReminder(await api.mobileRemindersResync());
      setMessage(null);
    } catch (e) {
      setMessage(errorText(e));
    }
  };

  const openExactSettings = async () => {
    try {
      await api.mobileReminderOpenExactSettings();
      setMessage("已打开系统设置，返回后点「刷新状态」查看结果");
    } catch (e) {
      setMessage(errorText(e));
    }
  };

  const openBatterySettings = async () => {
    try {
      await api.mobileReminderOpenBatterySettings();
      setMessage("已打开电池优化设置，允许后点「刷新状态」查看结果");
    } catch (e) {
      setMessage(errorText(e));
    }
  };

  const sendTest = async () => {
    setTesting(true);
    setMessage(null);
    try {
      let granted = await isPermissionGranted();
      if (!granted) {
        granted = (await requestPermission()) === "granted";
      }
      if (!granted) {
        throw new Error("未授予通知权限，请在系统设置中允许 Local To Do 发送通知");
      }
      await sendNotification({
        title: "Local To Do",
        body: "通知已启用，任务到点后会在这里提醒你。",
      });
      setMessage("已发送测试通知");
      await refreshStatus();
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setTesting(false);
    }
  };

  const exactHint = reminder
    ? reminder.exactAlarmAllowed
      ? "已允许，提醒可准点触发"
      : "未允许，系统可能延迟触发提醒"
    : "正在读取…";
  const notifyHint = reminder
    ? reminder.notificationsAllowed
      ? "已允许显示通知"
      : "通知已被系统关闭，提醒不会显示"
    : "正在读取…";

  return (
    <div className="mobile-sheet" role="dialog" aria-modal="true">
      <div className="mobile-sheet-backdrop" onClick={onClose} />
      <div className="mobile-sheet-panel">
        <header className="mobile-sheet-header">
          <h2>手机设置</h2>
          <button className="mobile-icon-button" onClick={onClose} aria-label="关闭">
            <IconX size={20} />
          </button>
        </header>

        <div className="mobile-sheet-body">
          {!settings ? (
            <div className="mobile-empty">正在载入设置…</div>
          ) : (
            <>
              <section className="mobile-sheet-section">
                <h3>通知与提醒</h3>
                <label className="mobile-switch-row">
                  <span>
                    <strong>接收任务提醒</strong>
                    <small>仅本机设置，不影响 Windows 任务数据</small>
                  </span>
                  <input
                    type="checkbox"
                    className="mobile-switch"
                    checked={settings.notificationsEnabled}
                    onChange={(event) =>
                      void apply({ notificationsEnabled: event.target.checked })
                    }
                  />
                </label>

                <label className="mobile-field-row">
                  <span className="mobile-field-label">提前提醒</span>
                  <select
                    className="mobile-select"
                    value={settings.reminderLeadMinutes}
                    disabled={!settings.notificationsEnabled}
                    onChange={(event) =>
                      void apply({ reminderLeadMinutes: Number(event.target.value) })
                    }
                  >
                    {LEAD_OPTIONS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </label>

                {settings.notificationsEnabled && (
                  <div className="mobile-reminder-status">
                    <div className="mobile-reminder-row">
                      <span>已安排提醒</span>
                      <strong>{reminder ? `${reminder.scheduled} 条` : "…"}</strong>
                    </div>
                    <div className="mobile-reminder-row">
                      <span>精确闹钟</span>
                      <strong className={reminder?.exactAlarmAllowed ? "" : "mobile-warn-text"}>
                        {exactHint}
                      </strong>
                    </div>
                    <div className="mobile-reminder-row">
                      <span>通知权限</span>
                      <strong className={reminder?.notificationsAllowed ? "" : "mobile-warn-text"}>
                        {notifyHint}
                      </strong>
                    </div>
                    <div className="mobile-reminder-row">
                      <span>电池优化</span>
                      <strong
                        className={
                          reminder?.ignoringBatteryOptimizations ? "" : "mobile-warn-text"
                        }
                      >
                        {reminder
                          ? reminder.ignoringBatteryOptimizations
                            ? "已允许后台运行"
                            : "未允许，熄屏后提醒可能被系统拦截"
                          : "正在读取…"}
                      </strong>
                    </div>
                    {!reminder?.exactAlarmAllowed && (
                      <button className="mobile-button" onClick={() => void openExactSettings()}>
                        去开启精确闹钟
                      </button>
                    )}
                    {!reminder?.ignoringBatteryOptimizations && (
                      <button className="mobile-button" onClick={() => void openBatterySettings()}>
                        去关闭电池优化
                      </button>
                    )}
                    <button className="mobile-button" onClick={() => void resync()}>
                      重新安排提醒
                    </button>
                  </div>
                )}

                <button
                  className="mobile-button"
                  onClick={() => void sendTest()}
                  disabled={testing}
                >
                  <IconBell size={16} /> {testing ? "正在发送…" : "发送测试通知"}
                </button>
                {message && <p className="mobile-hint">{message}</p>}
                <p className="mobile-hint">
                  提醒依据最近一次同步的任务与到期时间；电脑端修改任务后，手机同步时会自动更新提醒。
                </p>
              </section>

              <section className="mobile-sheet-section">
                <h3>说明</h3>
                <button
                  className="mobile-setting-entry"
                  onClick={() => setGuideOpen(true)}
                >
                  <span>使用说明</span>
                  <span aria-hidden="true">›</span>
                </button>
                <p className="mobile-hint">
                  Android 端是 Windows Local To Do
                  的只读伴侣，任务数据只由电脑单向同步，手机上不能创建或修改任务。
                </p>
              </section>
            </>
          )}
        </div>
      </div>
      {guideOpen && <MobileGuideSheet onClose={() => setGuideOpen(false)} />}
    </div>
  );
}
