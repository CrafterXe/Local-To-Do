import type { ReactNode } from "react";
import { useCallback, useEffect, useRef, useState } from "react";

import { api, type MobileSyncState } from "../api";
import {
  IconAll,
  IconCalendar,
  IconList,
  IconSettings,
  IconSun,
} from "../components/Icons";
import { MobileAllTasks } from "./MobileAllTasks";
import { MobileCalendar } from "./MobileCalendar";
import { MobileLists } from "./MobileLists";
import { MobileSettingsSheet } from "./MobileSettingsSheet";
import { MobileSyncSheet } from "./MobileSyncSheet";
import { MobileTaskDetail } from "./MobileTaskDetail";
import { MobileToday } from "./MobileToday";
import { PairingPanel } from "./PairingPanel";
import { errorText } from "./mobileUtils";

type MobileTab = "today" | "all" | "calendar" | "lists";

const TABS: { id: MobileTab; label: string; icon: ReactNode }[] = [
  { id: "today", label: "我的今天", icon: <IconSun size={21} /> },
  { id: "all", label: "全部任务", icon: <IconAll size={21} /> },
  { id: "calendar", label: "日历", icon: <IconCalendar size={21} /> },
  { id: "lists", label: "我的清单", icon: <IconList size={21} /> },
];

const TAB_TITLE: Record<MobileTab, string> = {
  today: "我的今天",
  all: "全部任务",
  calendar: "日历",
  lists: "我的清单",
};

const PHASE_LABEL: Record<string, string> = {
  unpaired: "未配对",
  pairing: "配对中",
  paired: "已配对",
  firstSync: "首次同步中",
  syncing: "同步中",
  synced: "同步成功",
  offline: "离线",
  failed: "同步失败",
  needsPairing: "需要重新配对",
};

/**
 * Android 只读伴侣的根组件。
 *
 * 数据全部来自 Windows 单向同步的本地镜像，界面不提供任何任务/清单写操作：
 * 只允许查看、搜索、筛选、排序与查看同步状态。
 */
export function MobileSyncApp() {
  const [state, setState] = useState<MobileSyncState | null>(null);
  const [tab, setTab] = useState<MobileTab>("today");
  const [detailId, setDetailId] = useState<string | null>(null);
  const [sheet, setSheet] = useState<"sync" | "settings" | null>(null);
  const [forcePairing, setForcePairing] = useState(false);
  const [version, setVersion] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const autoSynced = useRef(false);

  // 移动端不经过桌面设置页，这里直接使用深色主题变量
  useEffect(() => {
    document.documentElement.dataset.theme = "dark";
  }, []);

  const refresh = useCallback(async () => {
    try {
      setState(await api.mobileSyncState());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  // 打开应用时按本地镜像重建提醒：即使离线、无法同步，也能恢复已同步任务的提醒
  useEffect(() => {
    void api.mobileRemindersResync().catch(() => {});
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // 提示信息几秒后自动消失，避免长期占用页面顶部
  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 2600);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const syncNow = useCallback(async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const next = await api.mobileSyncNow();
      setState(next);
      setVersion((value) => value + 1);
      if (next.phase === "synced") setNotice("同步完成");
    } catch (e) {
      setError(errorText(e));
      void refresh();
    } finally {
      setBusy(false);
    }
  }, [refresh]);

  // 应用启动（或刚完成配对）时自动同步一次
  useEffect(() => {
    if (!state?.paired || autoSynced.current) return;
    autoSynced.current = true;
    void syncNow();
  }, [state?.paired, syncNow]);

  const unpair = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await api.mobileUnpair();
      setState(next);
      setVersion((value) => value + 1);
      setSheet(null);
      setNotice("已解除配对，本地数据仍可离线查看");
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }, []);

  if (!state) {
    return (
      <div className="mobile-page">
        <p className="mobile-hint">正在载入…</p>
      </div>
    );
  }

  const hasData =
    state.counts.tasks > 0 || state.counts.lists > 0 || state.lastSyncAt !== null;

  if (!state.paired && (!hasData || forcePairing)) {
    return (
      <PairingPanel
        state={state}
        onPaired={(next) => {
          setState(next);
          setForcePairing(false);
          // 配对完成后立即同步一次，避免启动副作用重复触发
          autoSynced.current = true;
          void syncNow();
        }}
        onBack={hasData ? () => setForcePairing(false) : undefined}
      />
    );
  }

  const phase = state.phase;
  const phaseLabel = PHASE_LABEL[phase] ?? phase;

  return (
    <div className="mobile-shell">
      <header className="mobile-topbar">
        <div className="mobile-topbar-text">
          <h1>{TAB_TITLE[tab]}</h1>
          <p>{state.paired ? "由 Windows 电脑单向同步" : "未连接电脑 · 离线浏览"}</p>
        </div>
        <div className="mobile-topbar-actions">
          <button
            className={`mobile-phase-chip phase-${phase}`}
            onClick={() => setSheet("sync")}
          >
            <span className="mobile-phase-dot" />
            {phaseLabel}
          </button>
          <button
            className="mobile-icon-button"
            onClick={() => setSheet("settings")}
            aria-label="设置"
          >
            <IconSettings size={20} />
          </button>
        </div>
      </header>

      {(error || state.error) && (
        <div className="mobile-alert error">{error ?? state.error}</div>
      )}
      {notice && <div className="mobile-alert">{notice}</div>}

      <main className="mobile-content">
        {tab === "today" && <MobileToday version={version} onOpenTask={setDetailId} />}
        {tab === "all" && <MobileAllTasks version={version} onOpenTask={setDetailId} />}
        {tab === "calendar" && <MobileCalendar version={version} onOpenTask={setDetailId} />}
        {tab === "lists" && <MobileLists version={version} onOpenTask={setDetailId} />}
      </main>

      <nav className="mobile-tabbar">
        {TABS.map((item) => (
          <button
            key={item.id}
            className={`mobile-tab${tab === item.id ? " active" : ""}`}
            onClick={() => setTab(item.id)}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        ))}
      </nav>

      {detailId && (
        <MobileTaskDetail taskId={detailId} onClose={() => setDetailId(null)} />
      )}

      {sheet === "sync" && (
        <MobileSyncSheet
          state={state}
          busy={busy}
          onClose={() => setSheet(null)}
          onSync={() => void syncNow()}
          onUnpair={() => void unpair()}
          onRepair={() => {
            setSheet(null);
            setForcePairing(true);
          }}
        />
      )}

      {sheet === "settings" && <MobileSettingsSheet onClose={() => setSheet(null)} />}
    </div>
  );
}
