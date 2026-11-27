import { useMemo, useRef, useState } from "react";

import {
  api,
  type AppInfo,
  type AppSettings,
  type BackupEntry,
  type DbStats,
  type ReminderStatus,
  type RestoreReport,
  type TaskList,
} from "../api";
import {
  IconBell,
  IconDatabase,
  IconDownload,
  IconInfo,
  IconUpload,
} from "../components/Icons";
import { ConfirmModal } from "../components/Modal";
import { formatBytes, formatTimestamp } from "../utils/date";

interface Props {
  settings: AppSettings;
  lists: TaskList[];
  info: AppInfo | null;
  stats: DbStats | null;
  backups: BackupEntry[];
  reminderStatus: ReminderStatus | null;
  onSettings: (settings: AppSettings) => void;
  onDataChanged: () => Promise<void>;
  onSystemRefresh: () => Promise<void>;
  notify: (message: string, error?: boolean) => void;
}

type PendingRestore = {
  name: string;
  report: RestoreReport;
} & ({ kind: "path"; path: string } | { kind: "text"; text: string });

const FALLBACK_TIMEZONES = [
  "Asia/Shanghai",
  "Asia/Tokyo",
  "Asia/Singapore",
  "Europe/London",
  "Europe/Berlin",
  "America/New_York",
  "America/Los_Angeles",
  "Australia/Sydney",
  "UTC",
];

export function MobileOfflineSettings({
  settings,
  lists,
  info,
  stats,
  backups,
  reminderStatus,
  onSettings,
  onDataChanged,
  onSystemRefresh,
  notify,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [pendingRestore, setPendingRestore] = useState<PendingRestore | null>(null);
  const [resetOpen, setResetOpen] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const timezones = useMemo(() => {
    const intl = Intl as unknown as { supportedValuesOf?: (key: "timeZone") => string[] };
    const values = intl.supportedValuesOf?.("timeZone") ?? FALLBACK_TIMEZONES;
    return settings.timezone && !values.includes(settings.timezone)
      ? [settings.timezone, ...values]
      : values;
  }, [settings.timezone]);

  const update = async (patch: Partial<AppSettings>) => {
    setBusy(true);
    try {
      if (patch.notificationsEnabled) await api.mobileReminderRequestNotifications();
      const updated = await api.updateSettings({ ...settings, ...patch });
      onSettings(updated);
      const reminder = await api.mobileRemindersResync();
      if (!reminder.notificationsAllowed && updated.notificationsEnabled) {
        notify("系统通知权限尚未开启，任务提醒可能无法显示", true);
      }
      await onSystemRefresh();
    } catch (error) {
      notify(String(error), true);
    } finally {
      setBusy(false);
    }
  };

  const createBackup = async (share = false) => {
    setBusy(true);
    try {
      const backup = await api.createBackup();
      if (share) await api.mobileBackupShare(backup.path);
      onSettings(await api.getSettings());
      await onSystemRefresh();
      notify(share ? "备份已生成，正在打开系统分享" : "本地备份已创建");
    } catch (error) {
      notify(String(error), true);
    } finally {
      setBusy(false);
    }
  };

  const exportCsv = async () => {
    if (!info) return;
    setBusy(true);
    try {
      const stamp = new Date().toISOString().replace(/[-:]/g, "").replace(/\..+$/, "");
      const path = `${info.backupDir}/local_to_do_tasks_${stamp}.csv`;
      await api.exportCsvFile(path);
      await api.mobileBackupShare(path);
      notify("CSV 已导出，正在打开系统分享");
    } catch (error) {
      notify(String(error), true);
    } finally {
      setBusy(false);
    }
  };

  const preparePathRestore = async (backup: BackupEntry) => {
    setBusy(true);
    try {
      const report = await api.validateBackup(backup.path);
      setPendingRestore({ kind: "path", path: backup.path, name: backup.fileName, report });
    } catch (error) {
      notify(String(error), true);
    } finally {
      setBusy(false);
    }
  };

  const chooseImport = async (file: File | undefined) => {
    if (!file) return;
    setBusy(true);
    try {
      if (file.size > 32 * 1024 * 1024) throw new Error("备份文件超过 32 MB，无法导入");
      const text = await file.text();
      const bundle = JSON.parse(text) as { format?: string; version?: number };
      if (bundle.format !== "local-to-do-backup") throw new Error("不是有效的 Local To Do 备份文件");
      const data = (bundle as { data?: Record<string, unknown[]> }).data ?? {};
      const report = {
        lists: data.lists?.length ?? 0,
        tasks: data.tasks?.length ?? 0,
        notes: data.notes?.length ?? 0,
        recurrences: data.recurrences?.length ?? 0,
      };
      setPendingRestore({ kind: "text", text, name: file.name, report });
    } catch (error) {
      notify(`无法读取备份：${String(error)}`, true);
    } finally {
      setBusy(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  };

  const restore = async () => {
    if (!pendingRestore) return;
    try {
      const report =
        pendingRestore.kind === "path"
          ? await api.restoreBackup(pendingRestore.path)
          : await api.restoreBackupText(pendingRestore.text);
      setPendingRestore(null);
      await onDataChanged();
      await onSystemRefresh();
      notify(`恢复完成：${report.tasks} 个任务、${report.lists} 个清单、${report.notes} 条备忘录`);
    } catch (error) {
      notify(`恢复失败，原数据未改变：${String(error)}`, true);
    }
  };

  const jsonBackups = backups.filter((backup) => backup.format === "json");

  return (
    <div className="offline-settings">
      <section className="offline-setting-section">
        <h2>外观与日期</h2>
        <div className="offline-setting-row">
          <span><strong>主题</strong><small>浅色和深色模式会保存在本机</small></span>
          <select value={settings.theme} disabled={busy} onChange={(event) => void update({ theme: event.target.value })} aria-label="主题">
            <option value="dark">深色</option>
            <option value="light">浅色</option>
          </select>
        </div>
        <div className="offline-setting-row">
          <span><strong>默认清单</strong><small>快速添加任务时使用</small></span>
          <select value={settings.defaultListId ?? ""} disabled={busy} onChange={(event) => void update({ defaultListId: event.target.value || null })} aria-label="默认清单">
            {lists.map((list) => <option key={list.id} value={list.id}>{list.name}</option>)}
          </select>
        </div>
        <div className="offline-setting-row">
          <span><strong>自动跟随系统时区</strong><small>关闭后可指定提醒计算时区</small></span>
          <input type="checkbox" checked={settings.timezoneAuto} disabled={busy} onChange={(event) => void update({ timezoneAuto: event.target.checked })} aria-label="自动跟随系统时区" />
        </div>
        {!settings.timezoneAuto ? (
          <div className="offline-setting-row">
            <span><strong>提醒时区</strong><small>使用标准 IANA 时区</small></span>
            <select value={settings.timezone ?? "Asia/Shanghai"} disabled={busy} onChange={(event) => void update({ timezone: event.target.value })} aria-label="提醒时区">
              {timezones.map((zone) => <option key={zone} value={zone}>{zone}</option>)}
            </select>
          </div>
        ) : null}
        <label className="offline-setting-column">
          <span><strong>周数基准日期</strong><small>该日期所在自然周为第 1 周，周一作为每周开始</small></span>
          <input
            type="date"
            value={settings.weekCustomized ? settings.weekBaseDate ?? "" : ""}
            disabled={busy}
            onChange={(event) => event.target.value && void update({ weekBaseDate: event.target.value, weekCustomized: true })}
          />
        </label>
        {settings.weekCustomized ? (
          <button className="offline-link-button" disabled={busy} onClick={() => void update({ weekCustomized: false, weekBaseDate: null })}>恢复默认周数</button>
        ) : null}
      </section>

      <section className="offline-setting-section">
        <h2><IconBell size={17} /> 提醒与系统权限</h2>
        <div className="offline-setting-row">
          <span><strong>任务提醒</strong><small>由 Android 系统闹钟调度，无需保持应用运行</small></span>
          <input type="checkbox" checked={settings.notificationsEnabled} disabled={busy} onChange={(event) => void update({ notificationsEnabled: event.target.checked })} aria-label="任务提醒" />
        </div>
        <div className="offline-permission-grid">
          <span>通知权限<strong>{reminderStatus?.notificationsAllowed ? "已允许" : "未允许"}</strong></span>
          <span>精确闹钟<strong>{reminderStatus?.exactAlarmAllowed ? "已允许" : "未允许"}</strong></span>
          <span>已注册提醒<strong>{reminderStatus?.scheduled ?? 0} 条</strong></span>
          <span>电池优化白名单<strong>{reminderStatus?.ignoringBatteryOptimizations ? "已加入" : "未加入"}</strong></span>
        </div>
        {!reminderStatus?.notificationsAllowed ? (
          <button className="offline-secondary-button" disabled={busy} onClick={() => void update({ notificationsEnabled: true })}>请求通知权限</button>
        ) : null}
        {!reminderStatus?.exactAlarmAllowed ? (
          <button className="offline-secondary-button" onClick={() => void api.mobileReminderOpenExactSettings().catch((error) => notify(String(error), true))}>打开精确闹钟设置</button>
        ) : null}
        {!reminderStatus?.ignoringBatteryOptimizations ? (
          <button className="offline-secondary-button" onClick={() => void api.mobileReminderOpenBatterySettings().catch((error) => notify(String(error), true))}>打开电池优化设置</button>
        ) : null}
        <button className="offline-secondary-button" disabled={busy} onClick={() => {
          setBusy(true);
          void api.sendTestNotification()
            .then((sent) => notify(sent ? "测试通知已发送" : "系统未接受测试通知，请检查权限", !sent))
            .catch((error) => notify(String(error), true))
            .finally(() => setBusy(false));
        }}>发送测试通知</button>
        {reminderStatus?.truncated ? (
          <p className="offline-setting-note">当前有 {reminderStatus.truncated} 条更晚提醒暂未注册（系统计划上限 200 条）。完成较早任务或重新校准后会滚动补充。</p>
        ) : null}
        <p className="offline-setting-note">未授予精确闹钟时仍会使用系统非精确闹钟，但 Doze、省电策略和厂商后台限制可能造成延迟，无法承诺所有设备绝对准时。</p>
      </section>

      <section className="offline-setting-section">
        <h2><IconDatabase size={17} /> 自动备份</h2>
        <div className="offline-setting-row">
          <span><strong>自动备份</strong><small>打开应用或校准提醒时，按间隔在私有目录生成备份</small></span>
          <input type="checkbox" checked={settings.autoBackupEnabled} disabled={busy} onChange={(event) => void update({ autoBackupEnabled: event.target.checked })} aria-label="自动备份" />
        </div>
        <div className="offline-setting-row">
          <span><strong>备份间隔（小时）</strong><small>最近备份：{formatTimestamp(settings.lastBackupAt) || "从未"}</small></span>
          <input className="offline-number-input" type="number" min={1} max={8760} value={settings.autoBackupIntervalHours} disabled={busy} onChange={(event) => void update({ autoBackupIntervalHours: Math.max(1, Number(event.target.value) || 1) })} aria-label="备份间隔小时" />
        </div>
      </section>

      <section className="offline-setting-section">
        <h2><IconDownload size={17} /> 备份、导出与恢复</h2>
        <p className="offline-setting-note">备份包含清单、任务、步骤、重复规则、备忘录和设置。恢复会覆盖当前数据，所有插入在同一事务中完成，失败不会留下部分数据。</p>
        <div className="offline-button-grid">
          <button className="offline-secondary-button" disabled={busy} onClick={() => void createBackup()}>立即备份到本机</button>
          <button className="offline-secondary-button" disabled={busy} onClick={() => void createBackup(true)}>备份并分享 JSON</button>
          <button className="offline-secondary-button" disabled={busy || !info} onClick={() => void exportCsv()}>导出并分享 CSV</button>
          <button className="offline-secondary-button" disabled={busy} onClick={() => fileRef.current?.click()}><IconUpload size={16} /> 导入 JSON 备份</button>
        </div>
        <input ref={fileRef} className="offline-file-input" type="file" accept=".json,application/json" onChange={(event) => void chooseImport(event.target.files?.[0])} />
        <div className="offline-backup-list">
          {jsonBackups.length === 0 ? <p className="offline-setting-note">还没有本机 JSON 备份。</p> : jsonBackups.map((backup) => (
            <div className="offline-backup-row" key={backup.path}>
              <span><strong>{backup.fileName}</strong><small>{formatTimestamp(backup.createdAt)} · {formatBytes(backup.size)}</small></span>
              <button onClick={() => void api.mobileBackupShare(backup.path).catch((error) => notify(String(error), true))}>分享</button>
              <button disabled={busy} onClick={() => void preparePathRestore(backup)}>恢复</button>
            </div>
          ))}
        </div>
      </section>

      <section className="offline-setting-section">
        <h2><IconInfo size={17} /> 数据与使用说明</h2>
        {stats ? (
          <div className="offline-permission-grid">
            <span>未删除任务<strong>{stats.tasks}</strong></span>
            <span>清单<strong>{stats.lists}</strong></span>
            <span>备忘录<strong>{stats.notes}</strong></span>
            <span>数据库大小<strong>{formatBytes(stats.databaseBytes)}</strong></span>
          </div>
        ) : null}
        <p className="offline-setting-note">数据仅保存在此 Android 应用的私有 SQLite 数据库中，不需要账号、网络、Windows 电脑或旧版伴侣应用。卸载应用会删除本机数据，请先分享 JSON 备份。</p>
        <p className="offline-setting-note">点击任务或备忘录打开右侧详情；左上角展开完整导航。系统返回键优先关闭当前面板。重复任务完成后由共享业务核心推进下一周期并重建提醒。</p>
        <p className="offline-setting-note">版本 {info?.version ?? "—"} · 数据库版本不兼容时应用会拒绝危险写入。升级前建议先创建并分享备份。</p>
        <button className="offline-link-button danger" disabled={busy} onClick={() => setResetOpen(true)}>恢复默认应用设置（不删除任务）</button>
      </section>

      {pendingRestore ? (
        <ConfirmModal
          title="确认覆盖当前数据"
          message={`将从「${pendingRestore.name}」恢复 ${pendingRestore.report.lists} 个清单、${pendingRestore.report.tasks} 个任务、${pendingRestore.report.notes} 条备忘录和 ${pendingRestore.report.recurrences} 条重复规则。\n\n当前数据将被替换。建议先创建备份。`}
          onClose={() => setPendingRestore(null)}
          actions={[{ label: "覆盖并恢复", kind: "danger", onClick: restore }]}
        />
      ) : null}

      {resetOpen ? (
        <ConfirmModal
          title="恢复默认设置"
          message="只重置主题、提醒、时区、周数和备份偏好，不会删除任何任务、清单或备忘录。"
          onClose={() => setResetOpen(false)}
          actions={[{
            label: "恢复默认设置",
            kind: "danger",
            onClick: async () => {
              try {
                const updated = await api.resetSettings();
                onSettings(updated);
                await api.mobileRemindersResync();
                await onDataChanged();
                setResetOpen(false);
                notify("已恢复默认设置");
              } catch (error) {
                notify(String(error), true);
              }
            },
          }]}
        />
      ) : null}
    </div>
  );
}
