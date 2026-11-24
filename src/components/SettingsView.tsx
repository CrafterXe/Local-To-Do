import { useEffect, useMemo, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  api,
  type AppInfo,
  type AppSettings,
  type BackupEntry,
  type DbStats,
  type RestoreReport,
} from "../api";
import { formatBytes, formatRelative, formatTimestamp } from "../utils/date";
import { ConfirmModal } from "./Modal";
import {
  IconBell,
  IconDatabase,
  IconDownload,
  IconFolder,
  IconRefresh,
  IconUpload,
} from "./Icons";

interface Props {
  settings: AppSettings;
  info: AppInfo | null;
  stats: DbStats | null;
  backups: BackupEntry[];
  nextReminder: number | null;
  onUpdate: (patch: Partial<AppSettings>) => Promise<void>;
  onReset: () => Promise<void>;
  onRefresh: () => Promise<void>;
  notify: (msg: string, error?: boolean) => void;
}

function systemTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

const COMMON_TIMEZONES = [
  "Asia/Shanghai",
  "Asia/Tokyo",
  "America/Los_Angeles",
  "America/New_York",
  "Europe/London",
  "UTC",
];

const TIMEZONES: string[] = (() => {
  let list: string[] = [];
  try {
    const intl = Intl as unknown as { supportedValuesOf?: (key: string) => string[] };
    list = intl.supportedValuesOf?.("timeZone") ?? [];
  } catch {
    list = [];
  }
  return Array.from(new Set([...COMMON_TIMEZONES, ...list]));
})();

function Switch({ on, onClick }: { on: boolean; onClick: () => void }) {
  return <button className={`switch ${on ? "on" : ""}`} onClick={onClick} aria-pressed={on} />;
}

export function SettingsView({
  settings,
  info,
  stats,
  backups,
  nextReminder,
  onUpdate,
  onReset,
  onRefresh,
  notify,
}: Props) {
  const [pendingRestore, setPendingRestore] = useState<{ path: string; report: RestoreReport } | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const [soundOk, setSoundOk] = useState(true);
  const [tzQuery, setTzQuery] = useState("");

  useEffect(() => {
    let cancelled = false;
    api
      .checkSoundFile(settings.soundPath)
      .then((ok) => {
        if (!cancelled) setSoundOk(ok);
      })
      .catch(() => {
        if (!cancelled) setSoundOk(false);
      });
    return () => {
      cancelled = true;
    };
  }, [settings.soundPath]);

  const timezoneOptions = useMemo(() => {
    const q = tzQuery.trim().toLowerCase();
    const base = q
      ? TIMEZONES.filter((z) => z.toLowerCase().includes(q))
      : TIMEZONES;
    if (settings.timezone && !base.includes(settings.timezone)) {
      return [settings.timezone, ...base];
    }
    return base;
  }, [tzQuery, settings.timezone]);

  const testNotification = async () => {
    try {
      const ok = await api.sendTestNotification();
      notify(ok ? "测试通知已发送，请查看 Windows 通知中心" : "通知发送失败，请检查系统通知设置", !ok);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const chooseSound = async () => {
    try {
      const path = await open({
        title: "选择提示音",
        multiple: false,
        filters: [{ name: "WAV 音频", extensions: ["wav"] }],
      });
      if (!path || Array.isArray(path)) return;
      await api.previewSound(path);
      await onUpdate({ soundPath: path });
      notify("提示音已更新");
    } catch (e) {
      notify(`无法使用该提示音：${String(e)}`, true);
    }
  };

  const previewSound = async () => {
    try {
      await api.previewSound(settings.soundPath);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const resetSound = async () => {
    await onUpdate({ soundPath: null });
    notify("已恢复默认提示音");
  };

  const doBackup = async () => {
    setBusy(true);
    try {
      const entry = await api.createBackup();
      await onRefresh();
      notify(`备份完成：${entry.fileName}`);
    } catch (e) {
      notify(`备份失败：${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  };

  const exportJson = async () => {
    const path = await save({
      title: "导出 JSON 备份",
      defaultPath: `${info?.backupDir ?? ""}\\local_to_do_export.json`,
      filters: [{ name: "JSON 备份", extensions: ["json"] }],
    });
    if (!path) return;
    try {
      await api.exportJsonFile(path);
      notify("JSON 导出完成");
      await onRefresh();
    } catch (e) {
      notify(String(e), true);
    }
  };

  const exportCsv = async () => {
    const path = await save({
      title: "导出任务 CSV",
      defaultPath: `${info?.backupDir ?? ""}\\local_to_do_tasks.csv`,
      filters: [{ name: "CSV 表格", extensions: ["csv"] }],
    });
    if (!path) return;
    try {
      await api.exportCsvFile(path);
      notify("CSV 导出完成");
      await onRefresh();
    } catch (e) {
      notify(String(e), true);
    }
  };

  const chooseRestore = async () => {
    const path = await open({
      title: "选择备份文件",
      multiple: false,
      filters: [{ name: "Local To Do 备份", extensions: ["json"] }],
    });
    if (!path || Array.isArray(path)) return;
    try {
      const report = await api.validateBackup(path);
      setPendingRestore({ path, report });
    } catch (e) {
      notify(`无法读取备份：${String(e)}`, true);
    }
  };

  const doRestore = async (path: string) => {
    setBusy(true);
    try {
      const report = await api.restoreBackup(path);
      setPendingRestore(null);
      await onRefresh();
      notify(`恢复完成：${report.lists} 个清单、${report.tasks} 个任务、${report.notes} 条笔记`);
    } catch (e) {
      notify(`恢复失败：${String(e)}`, true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="settings-scroll">
      <section className="settings-section">
        <h2 className="settings-title">外观</h2>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">主题</div>
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            <button
              className={`btn small ${settings.theme === "dark" ? "primary" : ""}`}
              onClick={() => void onUpdate({ theme: "dark" })}
            >
              深色
            </button>
            <button
              className={`btn small ${settings.theme === "light" ? "primary" : ""}`}
              onClick={() => void onUpdate({ theme: "light" })}
            >
              浅色
            </button>
          </div>
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-title">时间与时区</h2>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">自动设置时区</div>
            <div className="setting-hint">
              {settings.timezoneAuto
                ? `跟随 Windows 系统时区（${systemTimeZone()}）`
                : "使用下方手动选择的时区"}
            </div>
          </div>
          <Switch
            on={settings.timezoneAuto}
            onClick={() =>
              void onUpdate(
                settings.timezoneAuto
                  ? { timezoneAuto: false, timezone: settings.timezone ?? systemTimeZone() }
                  : { timezoneAuto: true },
              )
            }
          />
        </div>
        {!settings.timezoneAuto && (
          <div className="setting-row timezone-row">
            <div className="setting-text">
              <div className="setting-name">选择时区</div>
              <div className="setting-hint">当前：{settings.timezone ?? systemTimeZone()}</div>
            </div>
            <div className="timezone-picker">
              <input
                className="input"
                placeholder="搜索时区"
                value={tzQuery}
                onChange={(e) => setTzQuery(e.target.value)}
              />
              <select
                className="select"
                value={settings.timezone ?? systemTimeZone()}
                onChange={(e) => void onUpdate({ timezone: e.target.value })}
              >
                {timezoneOptions.map((tz) => (
                  <option key={tz} value={tz}>
                    {tz}
                  </option>
                ))}
              </select>
            </div>
          </div>
        )}
      </section>

      <section className="settings-section">
        <h2 className="settings-title">通知与提醒</h2>
        <div className="settings-inline-hint">
          下一次提醒：{nextReminder ? formatTimestamp(nextReminder) : "暂无计划中的提醒"}
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">提示音</div>
          </div>
          <Switch
            on={settings.soundEnabled}
            onClick={() => void onUpdate({ soundEnabled: !settings.soundEnabled })}
          />
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">提示音文件</div>
            <div
              className={`setting-hint sound-path ${!soundOk ? "missing" : ""}`}
              title={settings.soundPath ?? "系统默认提示音"}
            >
              {settings.soundPath ?? "系统默认提示音"}
            </div>
            {!soundOk && <div className="setting-hint missing">文件不存在，请重新选择</div>}
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            <button
              className="btn small"
              disabled={!settings.soundEnabled}
              onClick={() => void chooseSound()}
            >
              选择
            </button>
            <button
              className="btn small"
              disabled={!settings.soundEnabled}
              onClick={() => void previewSound()}
            >
              试听
            </button>
            {settings.soundPath && (
              <button
                className="btn small"
                disabled={!settings.soundEnabled}
                onClick={() => void resetSound()}
              >
                恢复默认
              </button>
            )}
          </div>
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">发送测试通知</div>
          </div>
          <button className="btn small" onClick={() => void testNotification()}>
            <IconBell size={14} />
            发送测试
          </button>
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-title">启动与窗口</h2>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">关闭窗口时最小化到系统托盘</div>
            <div className="setting-hint">关闭后仍在托盘运行，提醒不中断。</div>
          </div>
          <Switch
            on={settings.closeToTray}
            onClick={() => void onUpdate({ closeToTray: !settings.closeToTray })}
          />
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">开机自动启动</div>
          </div>
          <Switch on={settings.autostart} onClick={() => void onUpdate({ autostart: !settings.autostart })} />
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">开机启动时最小化到托盘</div>
            <div className="setting-hint">需先开启开机自动启动。</div>
          </div>
          <Switch
            on={settings.startMinimized}
            onClick={() => void onUpdate({ startMinimized: !settings.startMinimized })}
          />
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-title">数据与备份</h2>
        <p className="settings-desc">数据保存在本机，不上传任何信息。</p>

        <div className="setting-row" style={{ flexDirection: "column", alignItems: "stretch" }}>
          <div className="setting-text">
            <div className="setting-name">数据存储位置</div>
            <div className="path-box">数据库：{info?.dbPath ?? "加载中…"}</div>
            <div className="path-box">备份目录：{info?.backupDir ?? "加载中…"}</div>
            <div className="path-box">日志目录：{info?.logDir ?? "加载中…"}</div>
          </div>
          <div style={{ display: "flex", gap: 8, marginTop: 10, flexWrap: "wrap" }}>
            <button className="btn small" onClick={() => void api.openDataDir().catch((e) => notify(String(e), true))}>
              <IconFolder size={14} />
              打开数据目录
            </button>
            <button className="btn small" onClick={() => void api.openBackupDir().catch((e) => notify(String(e), true))}>
              <IconFolder size={14} />
              打开备份目录
            </button>
            {stats && (
              <span style={{ marginLeft: "auto", fontSize: 11.5, color: "var(--muted-2)", alignSelf: "center" }}>
                {stats.tasks} 个任务 · {stats.lists} 个清单 · {stats.notes} 条笔记 · 数据库 {formatBytes(stats.databaseBytes)}
              </span>
            )}
          </div>
        </div>

        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">自动备份</div>
            <div className="setting-hint">
              上次备份：{formatRelative(settings.lastBackupAt)}（最多保留 20 份）
            </div>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <select
              className="select"
              style={{ width: 116 }}
              value={settings.autoBackupIntervalHours}
              onChange={(e) => void onUpdate({ autoBackupIntervalHours: Number(e.target.value) })}
            >
              {[6, 12, 24, 48, 168].map((h) => (
                <option key={h} value={h}>
                  每 {h} 小时
                </option>
              ))}
            </select>
            <Switch
              on={settings.autoBackupEnabled}
              onClick={() => void onUpdate({ autoBackupEnabled: !settings.autoBackupEnabled })}
            />
          </div>
        </div>

        <div style={{ display: "flex", gap: 8, flexWrap: "wrap", margin: "10px 0 16px" }}>
          <button className="btn" disabled={busy} onClick={() => void doBackup()}>
            <IconDatabase size={15} />
            立即备份
          </button>
          <button className="btn" onClick={() => void exportJson()}>
            <IconDownload size={15} />
            导出 JSON
          </button>
          <button className="btn" onClick={() => void exportCsv()}>
            <IconDownload size={15} />
            导出 CSV
          </button>
          <button className="btn" onClick={() => void chooseRestore()}>
            <IconUpload size={15} />
            从备份恢复
          </button>
          <button className="btn" onClick={() => void onRefresh()}>
            <IconRefresh size={15} />
            刷新
          </button>
        </div>

        <div className="section-label">最近的备份文件</div>
        {backups.length === 0 ? (
          <div style={{ fontSize: 12, color: "var(--muted-2)", padding: "8px 4px" }}>
            暂无备份文件，点击「立即备份」创建第一份。
          </div>
        ) : (
          <div>
            {backups.slice(0, 20).map((b) => (
              <div key={b.path} className="backup-row">
                <span className="name">{b.fileName}</span>
                <span className="size">{formatBytes(b.size)}</span>
                <span className="size">{formatTimestamp(b.createdAt)}</span>
                {b.format === "json" && (
                  <button
                    className="btn small"
                    onClick={async () => {
                      try {
                        const report = await api.validateBackup(b.path);
                        setPendingRestore({ path: b.path, report });
                      } catch (e) {
                        notify(String(e), true);
                      }
                    }}
                  >
                    恢复
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="settings-section">
        <h2 className="settings-title">关于</h2>
        <p className="settings-desc">
          Local To Do v{info?.version ?? "1.1.1"} · 本地优先，完全离线。
          <br />
          不联网、无账号、无遥测，数据归您所有。
        </p>
      </section>

      <section className="settings-section settings-reset">
        <button className="btn" onClick={() => setConfirmReset(true)}>
          <IconRefresh size={15} />
          重置设置
        </button>
      </section>

      {confirmReset && (
        <ConfirmModal
          title="确认重置设置"
          message="将恢复应用设置的初始状态：默认主题、默认提示音、默认启动与窗口设置，以及自动跟随系统时区。不会删除任何任务、清单、备忘录、历史记录或最近删除数据，也不会重置任务自身的到期时间、提醒、重要与完成状态。"
          actions={[
            {
              label: "重置设置",
              kind: "danger",
              onClick: async () => {
                await onReset();
                setConfirmReset(false);
              },
            },
          ]}
          onClose={() => setConfirmReset(false)}
        />
      )}

      {pendingRestore && (
        <ConfirmModal
          title="确认恢复数据"
          message={`即将从备份恢复数据，这会覆盖当前所有任务、清单和笔记，且无法撤销。\n\n备份内容：${pendingRestore.report.lists} 个清单、${pendingRestore.report.tasks} 个任务、${pendingRestore.report.notes} 条笔记。\n\n建议先点击「立即备份」保存当前数据。`}
          actions={[
            { label: "取消", kind: "default", onClick: () => setPendingRestore(null) },
            {
              label: "覆盖并恢复",
              kind: "danger",
              onClick: () => doRestore(pendingRestore.path),
            },
          ]}
          onClose={() => setPendingRestore(null)}
        />
      )}
    </div>
  );
}
