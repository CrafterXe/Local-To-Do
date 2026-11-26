import { useEffect, useState } from "react";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

import { api, type MobileSettings } from "../api";
import { IconBell, IconX } from "../components/Icons";
import { errorText } from "./mobileUtils";

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
 * 当前版本仅提供通知开关与测试通知，定时提醒调度留待后续阶段。
 */
export function MobileSettingsSheet({ onClose }: MobileSettingsSheetProps) {
  const [settings, setSettings] = useState<MobileSettings | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .mobileSettingsGet()
      .then((value) => alive && setSettings(value))
      .catch((e) => alive && setMessage(errorText(e)));
    return () => {
      alive = false;
    };
  }, []);

  const apply = async (patch: Partial<MobileSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      setSettings(await api.mobileSettingsUpdate(next));
      setMessage(null);
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
        body: "通知已启用，任务提醒将在后续版本提供。",
      });
      setMessage("已发送测试通知");
    } catch (e) {
      setMessage(errorText(e));
    } finally {
      setTesting(false);
    }
  };

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
                    <strong>接收通知</strong>
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

                <button
                  className="mobile-button"
                  onClick={() => void sendTest()}
                  disabled={testing}
                >
                  <IconBell size={16} /> {testing ? "正在发送…" : "发送测试通知"}
                </button>
                {message && <p className="mobile-hint">{message}</p>}
                <p className="mobile-warning">
                  定时提醒调度尚未在本版本实现，将在后续阶段提供；当前仅保留通知权限与测试通知。
                </p>
              </section>

              <section className="mobile-sheet-section">
                <h3>关于</h3>
                <p className="mobile-hint">
                  Android 端是 Windows Local To Do
                  的只读伴侣，任务数据只由电脑单向同步，手机上不能创建或修改任务。
                </p>
              </section>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
