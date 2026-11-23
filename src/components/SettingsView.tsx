import { useState } from "react";
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
  onRefresh: () => Promise<void>;
  notify: (msg: string, error?: boolean) => void;
}

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
  onRefresh,
  notify,
}: Props) {
  const [pendingRestore, setPendingRestore] = useState<{ path: string; report: RestoreReport } | null>(null);
  const [busy, setBusy] = useState(false);

  const testNotification = async () => {
    try {
      const ok = await api.sendTestNotification();
      notify(ok ? "测试通知已发送，请查看 Windows 通知中心" : "通知发送失败，请检查系统通知设置", !ok);
    } catch (e) {
      notify(String(e), true);
    }
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
        <p className="settings-desc">默认使用深色主题，可切换为浅色主题。</p>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">主题</div>
            <div className="setting-hint">深色适合长时间使用，浅色在明亮环境下更清晰。</div>
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
        <h2 className="settings-title">通知与提醒</h2>
        <p className="settings-desc">
          提醒由应用内的后台调度线程驱动，并使用 Windows 原生通知。关闭主窗口后应用仍在系统托盘中运行，提醒不会中断。
        </p>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">启用提醒通知</div>
            <div className="setting-hint">
              下一次提醒：
              {nextReminder ? formatTimestamp(nextReminder) : "暂无计划中的提醒"}
            </div>
          </div>
          <Switch
            on={settings.notificationsEnabled}
            onClick={() => void onUpdate({ notificationsEnabled: !settings.notificationsEnabled })}
          />
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">发送测试通知</div>
            <div className="setting-hint">验证 Windows 通知权限是否正常。</div>
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
            <div className="setting-hint">
              开启后点击关闭按钮不会退出应用，提醒调度继续在后台运行；从托盘菜单「退出」才会真正结束程序。
            </div>
          </div>
          <Switch
            on={settings.closeToTray}
            onClick={() => void onUpdate({ closeToTray: !settings.closeToTray })}
          />
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">开机自动启动</div>
            <div className="setting-hint">仅在您主动开启后写入系统启动项，可随时关闭。</div>
          </div>
          <Switch on={settings.autostart} onClick={() => void onUpdate({ autostart: !settings.autostart })} />
        </div>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-name">开机启动时最小化到托盘</div>
            <div className="setting-hint">需要先开启开机自动启动。开机后不弹出窗口，仅在托盘中运行。</div>
          </div>
          <Switch
            on={settings.startMinimized}
            onClick={() => void onUpdate({ startMinimized: !settings.startMinimized })}
          />
        </div>
      </section>

      <section className="settings-section">
        <h2 className="settings-title">数据与备份</h2>
        <p className="settings-desc">
          所有数据均保存在本机 SQLite 数据库中，完全离线，不上传任何信息。建议定期备份到其他磁盘。
        </p>

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
              上次备份：{formatRelative(settings.lastBackupAt)}。备份以 JSON + 数据库快照形式保存在备份目录，最多保留 20 份。
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
          Local To Do v{info?.version ?? "1.0.0"} · 本地优先的个人任务与备忘录管理软件。
          <br />
          不联网、不需要账号、不收集数据、无广告与遥测。所有数据归您所有。
        </p>
      </section>

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
