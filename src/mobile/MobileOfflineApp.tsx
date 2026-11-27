import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import {
  api,
  type AppInfo,
  type AppSettings,
  type BackupEntry,
  type CalendarDay,
  type DbStats,
  type DeletedList,
  type HistoryEntry,
  type Note,
  type ReminderStatus,
  type SidebarCounts,
  type Task,
  type TaskInput,
  type TaskList as TaskListModel,
  type View,
} from "../api";
import { CalendarView } from "../components/CalendarView";
import { HistoryView } from "../components/HistoryView";
import {
  IconAll,
  IconArrowLeft,
  IconBell,
  IconCalendar,
  IconCheckCircle,
  IconChevronDown,
  IconChevronRight,
  IconChevronUp,
  IconClock,
  IconHistory,
  IconList,
  IconMore,
  IconNote,
  IconPlus,
  IconSearch,
  IconSettings,
  IconStar,
  IconSun,
  IconTrash,
} from "../components/Icons";
import { ListModal } from "../components/ListModal";
import { ConfirmModal } from "../components/Modal";
import { TaskDetails } from "../components/TaskDetails";
import { TaskList } from "../components/TaskList";
import { WeekPicker } from "../components/WeekPicker";
import { DEFAULT_SORT_KIND } from "../utils/defaults";
import {
  currentWeekNumber,
  formatDayTitle,
  monthGrid,
  parseDateString,
  setDisplayTimezone,
  todayString,
} from "../utils/date";
import type { SortKind } from "../utils/sort";
import { MobileNoteDetail } from "./MobileNoteDetail";
import { MobileOfflineSettings } from "./MobileOfflineSettings";
import { validViewAfterListsChange, viewKey } from "./mobileOfflineState";

type Panel =
  | { type: "nav" }
  | { type: "task"; id: string }
  | { type: "note"; id: string }
  | null;

type ModalState =
  | { type: "list"; list?: TaskListModel }
  | { type: "deleteTask"; task: Task }
  | {
      type: "confirm";
      title: string;
      message: string;
      label: string;
      action: () => Promise<void>;
    }
  | null;

interface NavItem {
  view: View;
  label: string;
  icon: ReactNode;
  count?: number;
  rail?: boolean;
}

const EMPTY_COUNTS: SidebarCounts = {
  myDay: 0,
  important: 0,
  planned: 0,
  allOpen: 0,
  completed: 0,
  deleted: 0,
  notes: 0,
};

function readSort(key: string): SortKind {
  try {
    const saved = window.localStorage.getItem(`ltd.sort.${key}`);
    return saved === "name" || saved === "created" || saved === "due"
      ? saved
      : DEFAULT_SORT_KIND;
  } catch {
    return DEFAULT_SORT_KIND;
  }
}

function isTaskView(view: View): boolean {
  return !["notes", "settings", "history", "calendar", "lists"].includes(view.kind);
}

export function MobileOfflineApp() {
  const [booted, setBooted] = useState(false);
  const [bootError, setBootError] = useState<string | null>(null);
  const [lists, setLists] = useState<TaskListModel[]>([]);
  const [counts, setCounts] = useState<SidebarCounts>(EMPTY_COUNTS);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [stats, setStats] = useState<DbStats | null>(null);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [reminderStatus, setReminderStatus] = useState<ReminderStatus | null>(null);
  const [view, setView] = useState<View>({ kind: "myDay" });
  const [tasks, setTasks] = useState<Task[]>([]);
  const [tasksLoading, setTasksLoading] = useState(false);
  const [deletedLists, setDeletedLists] = useState<DeletedList[]>([]);
  const [notes, setNotes] = useState<Note[]>([]);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [notifiedTasks, setNotifiedTasks] = useState<Task[]>([]);
  const [reminderOpen, setReminderOpen] = useState(false);
  const [calendarDays, setCalendarDays] = useState<CalendarDay[]>([]);
  const [calendarMonth, setCalendarMonth] = useState(() => {
    const date = parseDateString(todayString());
    return { year: date.getFullYear(), month: date.getMonth() };
  });
  const [revision, setRevision] = useState(0);
  const [searches, setSearches] = useState<Record<string, string>>({});
  const [sortBy, setSortBy] = useState<SortKind>(() => readSort("myDay"));
  const [focusSignal, setFocusSignal] = useState(0);
  const [panel, setPanel] = useState<Panel>(null);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [modal, setModal] = useState<ModalState>(null);
  const [toast, setToast] = useState<{ message: string; error: boolean } | null>(null);

  const panelRef = useRef<Panel>(null);
  const taskCache = useRef(new Map<string, Task[]>());
  const scrollPositions = useRef(new Map<string, number>());
  const completionLock = useRef(new Map<string, number>());
  const currentKey = viewKey(view);
  const search = searches[currentKey] ?? "";

  const notify = useCallback((message: string, error = false) => {
    setToast({ message, error });
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3600);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings?.theme === "light" ? "light" : "dark";
    setDisplayTimezone(settings?.timezoneAuto === false ? settings.timezone : null);
  }, [settings?.theme, settings?.timezone, settings?.timezoneAuto]);

  const refreshMeta = useCallback(async () => {
    const [nextLists, nextCounts, nextSettings] = await Promise.all([
      api.listLists(),
      api.sidebarCounts(),
      api.getSettings(),
    ]);
    setLists(nextLists);
    setCounts(nextCounts);
    setSettings(nextSettings);
    return nextSettings;
  }, []);

  const refreshSystem = useCallback(async () => {
    const [nextInfo, nextStats, nextBackups, nextReminder] = await Promise.all([
      api.getAppInfo(),
      api.getDbStats(),
      api.listBackups(),
      api.mobileReminderStatus(),
    ]);
    setInfo(nextInfo);
    setStats(nextStats);
    setBackups(nextBackups);
    setReminderStatus(nextReminder);
  }, []);

  const bump = useCallback(() => setRevision((value) => value + 1), []);

  const bootstrap = useCallback(async () => {
    setBootError(null);
    try {
      await api.mobileRemindersResync();
      const currentSettings = await refreshMeta();
      if (currentSettings.notificationsEnabled) {
        await api.mobileReminderRequestNotifications().catch(() => null);
      }
      await refreshSystem();
      bump();
    } catch (error) {
      setBootError(String(error));
    } finally {
      setBooted(true);
    }
  }, [bump, refreshMeta, refreshSystem]);

  useEffect(() => {
    void bootstrap();
  }, [bootstrap]);

  useEffect(() => {
    if (!booted || view.kind !== "settings") return;
    void refreshSystem().catch((error) => notify(String(error), true));
  }, [booted, view.kind, revision, refreshSystem, notify]);

  useEffect(() => {
    if (!booted) return;
    const resync = () => {
      if (document.visibilityState !== "visible") return;
      void api
        .mobileRemindersResync()
        .then((status) => {
          setReminderStatus(status);
          bump();
        })
        .catch(() => {});
    };
    window.addEventListener("focus", resync);
    document.addEventListener("visibilitychange", resync);
    return () => {
      window.removeEventListener("focus", resync);
      document.removeEventListener("visibilitychange", resync);
    };
  }, [booted, bump]);

  useEffect(() => {
    if (!booted || !isTaskView(view)) return;
    let cancelled = false;
    setTasksLoading(true);
    const cached = taskCache.current.get(currentKey);
    if (cached) setTasks(cached);
    const query = search.trim();
    const request =
      view.kind === "day" && view.date && !query
        ? api.listTasksForDay(view.date)
        : api.listTasks(
            query
              ? { view: "all", search: query }
              : {
                  view: view.kind === "list" ? "list" : view.kind,
                  listId: view.listId ?? null,
                  includeCompleted: view.kind === "list" ? true : null,
                },
          );
    request
      .then((data) => {
        if (cancelled) return;
        if (!query) taskCache.current.set(currentKey, data);
        setTasks(data);
      })
      .catch((error) => notify(String(error), true))
      .finally(() => {
        if (!cancelled) setTasksLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [booted, view, currentKey, search, revision, notify]);

  useEffect(() => {
    if (!booted || view.kind !== "deleted") return;
    void api
      .listDeletedLists()
      .then(setDeletedLists)
      .catch((error) => notify(String(error), true));
  }, [booted, view.kind, revision, notify]);

  useEffect(() => {
    if (!booted || view.kind !== "notes") return;
    void api
      .listNotes(search.trim() || undefined)
      .then(setNotes)
      .catch((error) => notify(String(error), true));
  }, [booted, view.kind, search, revision, notify]);

  useEffect(() => {
    if (!booted || view.kind !== "history") return;
    void api
      .listHistory(300)
      .then(setHistory)
      .catch((error) => notify(String(error), true));
  }, [booted, view.kind, revision, notify]);

  useEffect(() => {
    if (!booted) return;
    void api.listNotifiedTasks().then(setNotifiedTasks).catch(() => {});
  }, [booted, revision]);

  useEffect(() => {
    if (!booted || view.kind !== "calendar") return;
    let cancelled = false;
    const cells = monthGrid(calendarMonth.year, calendarMonth.month);
    api
      .calendarMonth(cells[0], cells[cells.length - 1])
      .then((data) => {
        if (!cancelled) setCalendarDays(data);
      })
      .catch((error) => notify(String(error), true));
    return () => {
      cancelled = true;
    };
  }, [booted, view.kind, calendarMonth, revision, notify]);

  useEffect(() => {
    setSortBy(readSort(currentKey));
    const timer = window.setTimeout(() => {
      const element = document.querySelector<HTMLElement>(".offline-main .task-scroll");
      if (element) element.scrollTop = scrollPositions.current.get(currentKey) ?? 0;
    }, 0);
    return () => window.clearTimeout(timer);
  }, [currentKey]);

  useEffect(() => {
    const onPopState = () => {
      panelRef.current = null;
      setPanel(null);
      setSelectedTask(null);
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  const openPanel = useCallback((next: Exclude<Panel, null>) => {
    if (!panelRef.current) {
      window.history.pushState({ ltdMobilePanel: true }, "");
    }
    panelRef.current = next;
    setPanel(next);
  }, []);

  const closePanel = useCallback(() => {
    if (!panelRef.current) return;
    panelRef.current = null;
    setPanel(null);
    setSelectedTask(null);
    if (window.history.state?.ltdMobilePanel) window.history.back();
  }, []);

  const openTask = useCallback(
    async (id: string) => {
      openPanel({ type: "task", id });
      setSelectedTask(null);
      try {
        const task = await api.getTask(id);
        if (panelRef.current?.type === "task" && panelRef.current.id === id) {
          setSelectedTask(task);
        }
      } catch (error) {
        notify(String(error), true);
        closePanel();
      }
    },
    [closePanel, notify, openPanel],
  );

  const selectView = useCallback(
    (next: View) => {
      const element = document.querySelector<HTMLElement>(".offline-main .task-scroll");
      if (element) scrollPositions.current.set(currentKey, element.scrollTop);
      panelRef.current = null;
      setPanel(null);
      setSelectedTask(null);
      if (window.history.state?.ltdMobilePanel) window.history.back();
      setView(next);
    },
    [currentKey],
  );

  useEffect(() => {
    if (!booted || view.kind !== "list") return;
    const next = validViewAfterListsChange(view, lists.map((list) => list.id));
    if (next !== view) selectView(next);
  }, [booted, lists, selectView, view]);

  const refreshAfterMutation = useCallback(async () => {
    taskCache.current.clear();
    bump();
    await refreshMeta();
  }, [bump, refreshMeta]);

  const quickAdd = useCallback(
    async (title: string) => {
      const listId =
        view.kind === "list" && view.listId
          ? view.listId
          : settings?.defaultListId ?? lists[0]?.id ?? null;
      const input: TaskInput = {
        title,
        listId,
        note: null,
        isImportant: view.kind === "important" ? true : null,
        dueDate:
          view.kind === "day"
            ? view.date ?? null
            : view.kind === "planned"
              ? todayString()
              : null,
        dueTime: null,
        reminderOffset: null,
        reminderEnabled: false,
        myDay: view.kind === "myDay" ? todayString() : null,
        recurrence: null,
      };
      try {
        await api.createTask(input);
        await refreshAfterMutation();
      } catch (error) {
        notify(String(error), true);
      }
    },
    [lists, notify, refreshAfterMutation, settings?.defaultListId, view],
  );

  const saveTask = useCallback(
    async (id: string, input: TaskInput) => {
      const updated = await api.updateTask(id, input);
      setSelectedTask(updated);
      await refreshAfterMutation();
    },
    [refreshAfterMutation],
  );

  const toggleComplete = useCallback(
    async (task: Task) => {
      const now = Date.now();
      if (now - (completionLock.current.get(task.id) ?? 0) < 500) return;
      completionLock.current.set(task.id, now);
      try {
        const updated = await api.completeTask(
          task.id,
          task.recurrence ? true : !task.isCompleted,
        );
        if (updated && selectedTask?.id === task.id) setSelectedTask(updated);
        await refreshAfterMutation();
        if (panelRef.current?.type === "task" && panelRef.current.id === task.id) {
          setSelectedTask(await api.getTask(task.id));
        }
      } catch (error) {
        notify(String(error), true);
      }
    },
    [notify, refreshAfterMutation, selectedTask?.id],
  );

  const toggleImportant = useCallback(
    async (task: Task) => {
      try {
        const updated = await api.setImportant(task.id, !task.isImportant);
        setTasks((items) => items.map((item) => (item.id === updated.id ? updated : item)));
        if (selectedTask?.id === updated.id) setSelectedTask(updated);
        await refreshAfterMutation();
      } catch (error) {
        notify(String(error), true);
      }
    },
    [notify, refreshAfterMutation, selectedTask?.id],
  );

  const toggleMyDay = useCallback(
    async (task: Task) => {
      try {
        const updated = await api.setMyDay(task.id, !task.myDay);
        if (selectedTask?.id === updated.id) setSelectedTask(updated);
        await refreshAfterMutation();
        notify(updated.myDay ? "已加入「我的一天」" : "已从「我的一天」移除");
      } catch (error) {
        notify(String(error), true);
      }
    },
    [notify, refreshAfterMutation, selectedTask?.id],
  );

  const deleteTask = useCallback(
    async (task: Task, scope: "single" | "series") => {
      await api.deleteTask(task.id, scope);
      closePanel();
      setModal(null);
      await refreshAfterMutation();
      notify(scope === "series" ? "整个重复系列已移入最近删除" : "任务已移入最近删除");
    },
    [closePanel, notify, refreshAfterMutation],
  );

  const requestDeleteTask = useCallback(
    (task: Task) => {
      if (task.recurrence) setModal({ type: "deleteTask", task });
      else void deleteTask(task, "single").catch((error) => notify(String(error), true));
    },
    [deleteTask, notify],
  );

  const createNote = useCallback(async () => {
    try {
      const note = await api.createNote("", "");
      setNotes((items) => [note, ...items.filter((item) => item.id !== note.id)]);
      openPanel({ type: "note", id: note.id });
      await refreshAfterMutation();
    } catch (error) {
      notify(String(error), true);
    }
  }, [notify, openPanel, refreshAfterMutation]);

  const openNote = useCallback(
    (id: string) => openPanel({ type: "note", id }),
    [openPanel],
  );

  const moveList = async (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= lists.length) return;
    const next = lists.slice();
    [next[index], next[target]] = [next[target], next[index]];
    setLists(next);
    try {
      await api.reorderLists(next.map((list) => list.id));
      await refreshMeta();
    } catch (error) {
      notify(String(error), true);
      await refreshMeta();
    }
  };

  const changeSort = (kind: Exclude<SortKind, null>) => {
    setSortBy(kind);
    try {
      window.localStorage.setItem(`ltd.sort.${currentKey}`, kind);
    } catch {
      // 系统 WebView 禁用存储时仍保留本次会话排序。
    }
  };

  const navItems: NavItem[] = useMemo(
    () => [
      { view: { kind: "all" }, label: "全部任务", icon: <IconAll />, count: counts.allOpen, rail: true },
      { view: { kind: "myDay" }, label: "我的一天", icon: <IconSun />, count: counts.myDay, rail: true },
      { view: { kind: "calendar" }, label: "日历", icon: <IconCalendar />, rail: true },
      { view: { kind: "lists" }, label: "我的清单", icon: <IconList />, count: lists.length, rail: true },
      { view: { kind: "important" }, label: "重要任务", icon: <IconStar />, count: counts.important, rail: true },
      { view: { kind: "planned" }, label: "已计划", icon: <IconClock />, count: counts.planned, rail: true },
      { view: { kind: "completed" }, label: "已完成", icon: <IconCheckCircle />, count: counts.completed },
      { view: { kind: "notes" }, label: "备忘录", icon: <IconNote />, count: counts.notes, rail: true },
      { view: { kind: "deleted" }, label: "最近删除", icon: <IconTrash />, count: counts.deleted },
      { view: { kind: "history" }, label: "历史记录", icon: <IconHistory /> },
      { view: { kind: "settings" }, label: "设置", icon: <IconSettings />, rail: true },
    ],
    [counts, lists.length],
  );

  const title = useMemo(() => {
    if (search.trim()) return "搜索结果";
    if (view.kind === "list") return lists.find((list) => list.id === view.listId)?.name ?? "清单";
    if (view.kind === "day" && view.date) return formatDayTitle(view.date);
    return navItems.find((item) => viewKey(item.view) === currentKey)?.label ?? "Local To Do";
  }, [currentKey, lists, navItems, search, view]);

  const activeNote = panel?.type === "note" ? notes.find((note) => note.id === panel.id) : undefined;

  useEffect(() => {
    if (panel?.type !== "note" || activeNote) return;
    void api
      .getNote(panel.id)
      .then((note) => setNotes((items) => [note, ...items.filter((item) => item.id !== note.id)]))
      .catch((error) => {
        notify(String(error), true);
        closePanel();
      });
  }, [activeNote, closePanel, notify, panel]);

  if (!booted) {
    return (
      <div className="offline-boot">
        <strong>Local To Do</strong>
        <span>正在打开本地数据库…</span>
      </div>
    );
  }

  const showTaskList = isTaskView(view);
  const selectedList = view.kind === "list" ? lists.find((list) => list.id === view.listId) : undefined;

  return (
    <div className="offline-shell">
      <nav className="offline-rail" aria-label="快捷导航">
        <button className="offline-rail-menu" onClick={() => openPanel({ type: "nav" })} aria-label="展开导航">
          <IconMore size={22} />
        </button>
        <div className="offline-rail-items">
          {navItems.filter((item) => item.rail).map((item) => (
            <button
              key={viewKey(item.view)}
              className={currentKey === viewKey(item.view) ? "active" : ""}
              onClick={() => selectView(item.view)}
              aria-label={item.label}
              title={item.label}
            >
              {item.icon}
            </button>
          ))}
        </div>
      </nav>

      <main className="offline-main">
        <header className="offline-header">
          <div className="offline-title-row">
            {view.kind === "day" ? (
              <button className="offline-icon" onClick={() => selectView({ kind: "calendar" })} aria-label="返回日历">
                <IconArrowLeft size={20} />
              </button>
            ) : null}
            <div className="offline-title-text">
              <h1>{title}</h1>
              <p>
                {view.kind === "notes"
                  ? `${notes.length} 条备忘录 · 自动保存`
                  : view.kind === "lists"
                    ? `${lists.length} 个清单 · 本地离线保存`
                    : view.kind === "settings"
                      ? "提醒、数据与应用偏好"
                      : view.kind === "history"
                        ? "最近 300 条任务操作"
                        : view.kind === "calendar"
                          ? "选择有任务的日期查看详情"
                          : `${tasks.length} 个任务${tasksLoading ? " · 更新中" : ""}`}
              </p>
            </div>
            {view.kind === "notes" || showTaskList || view.kind === "lists" ? (
              <button
                className="offline-icon accent"
                aria-label={view.kind === "notes" ? "新建备忘录" : view.kind === "lists" ? "新建清单" : "新建任务"}
                onClick={() => {
                  if (view.kind === "notes") void createNote();
                  else if (view.kind === "lists") setModal({ type: "list" });
                  else setFocusSignal((value) => value + 1);
                }}
              >
                <IconPlus size={21} />
              </button>
            ) : null}
            {selectedList ? (
              <button className="offline-icon" aria-label="管理清单" onClick={() => setModal({ type: "list", list: selectedList })}>
                <IconSettings size={19} />
              </button>
            ) : null}
          </div>
          {view.kind !== "settings" && view.kind !== "history" && view.kind !== "calendar" && view.kind !== "lists" ? (
            <label className="offline-search">
              <IconSearch size={17} />
              <input
                value={search}
                placeholder={view.kind === "notes" ? "搜索备忘录" : "搜索任务和备注"}
                onChange={(event) =>
                  setSearches((items) => ({ ...items, [currentKey]: event.target.value }))
                }
              />
              {search ? (
                <button onClick={() => setSearches((items) => ({ ...items, [currentKey]: "" }))} aria-label="清除搜索">
                  ×
                </button>
              ) : null}
            </label>
          ) : null}
        </header>

        {bootError ? (
          <div className="offline-alert error">
            <span>{bootError}</span>
            <button onClick={() => void bootstrap()}>重试</button>
          </div>
        ) : null}

        {notifiedTasks.length > 0 && showTaskList ? (
          <div className="offline-fired-reminders">
            <button
              className="offline-fired-summary"
              onClick={() => {
                if (notifiedTasks.length === 1) void openTask(notifiedTasks[0].id);
                else setReminderOpen((open) => !open);
              }}
            >
              <IconBell size={17} />
              <span>{notifiedTasks[0].title}</span>
              <small>{notifiedTasks.length > 1 ? `${notifiedTasks.length} 条待处理` : "提醒已触发"}</small>
            </button>
            {reminderOpen && notifiedTasks.length > 1 ? (
              <div className="offline-fired-list">
                {notifiedTasks.map((task) => (
                  <button key={task.id} onClick={() => {
                    setReminderOpen(false);
                    void openTask(task.id);
                  }}>
                    <span>{task.title}</span>
                    <small>{task.listName}</small>
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        ) : null}

        <div className="offline-content">
          {view.kind === "calendar" ? (
            <CalendarView
              year={calendarMonth.year}
              month={calendarMonth.month}
              days={calendarDays}
              onPrev={() =>
                setCalendarMonth((current) => {
                  const date = new Date(current.year, current.month - 1, 1);
                  return { year: date.getFullYear(), month: date.getMonth() };
                })
              }
              onNext={() =>
                setCalendarMonth((current) => {
                  const date = new Date(current.year, current.month + 1, 1);
                  return { year: date.getFullYear(), month: date.getMonth() };
                })
              }
              onOpenDay={(date) => selectView({ kind: "day", date })}
            />
          ) : view.kind === "lists" ? (
            <div className="offline-list-page">
              {lists.map((list, index) => (
                <div className="offline-list-card" key={list.id}>
                  <button className="offline-list-open" onClick={() => selectView({ kind: "list", listId: list.id })}>
                    <span className="dot" style={{ background: list.color }} />
                    <span>
                      <strong>{list.name}</strong>
                      <small>{list.openCount} 个待办 · 共 {list.totalCount} 个任务</small>
                    </span>
                    <IconChevronRight size={18} />
                  </button>
                  <div className="offline-list-order">
                    <button disabled={index === 0} aria-label={`上移清单${list.name}`} onClick={() => void moveList(index, -1)}>
                      <IconChevronUp size={16} />
                    </button>
                    <button disabled={index === lists.length - 1} aria-label={`下移清单${list.name}`} onClick={() => void moveList(index, 1)}>
                      <IconChevronDown size={16} />
                    </button>
                  </div>
                  <button className="offline-icon" aria-label={`管理${list.name}`} onClick={() => setModal({ type: "list", list })}>
                    <IconSettings size={18} />
                  </button>
                </div>
              ))}
              <button className="offline-secondary-button" onClick={() => setModal({ type: "list" })}>
                <IconPlus size={17} /> 新建清单
              </button>
            </div>
          ) : view.kind === "notes" ? (
            <div className="offline-notes-list">
              {notes.length === 0 ? (
                <div className="offline-empty">还没有备忘录，点击右上角新建。</div>
              ) : (
                notes.map((note) => (
                  <button className="offline-note-card" key={note.id} onClick={() => openNote(note.id)}>
                    <strong>{note.title.trim() || "无标题备忘录"}</strong>
                    <span>{note.body.trim() || "无附加内容"}</span>
                    <small>{new Date(note.updatedAt).toLocaleString("zh-CN", { hour12: false })}</small>
                  </button>
                ))
              )}
            </div>
          ) : view.kind === "settings" && settings ? (
            <MobileOfflineSettings
              settings={settings}
              lists={lists}
              info={info}
              stats={stats}
              backups={backups}
              reminderStatus={reminderStatus}
              onSettings={setSettings}
              onDataChanged={refreshAfterMutation}
              onSystemRefresh={refreshSystem}
              notify={notify}
            />
          ) : view.kind === "settings" ? (
            <div className="offline-empty">设置暂时不可用，请点击上方重试。</div>
          ) : view.kind === "history" ? (
            <HistoryView entries={history} />
          ) : (
            <>
              <div className="offline-task-tools">
                <span>{view.kind === "list" ? "包含已完成任务" : " "}</span>
                <select value={sortBy ?? "default"} onChange={(event) => event.target.value !== "default" && changeSort(event.target.value as Exclude<SortKind, null>)} aria-label="任务排序">
                  <option value="default">默认排序</option>
                  <option value="created">按创建时间</option>
                  <option value="due">按到期日期</option>
                  <option value="name">按名称</option>
                </select>
              </div>
              <TaskList
                tasks={tasks}
                view={view}
                deletedLists={deletedLists}
                selectedId={panel?.type === "task" ? panel.id : null}
                searchActive={Boolean(search.trim())}
                showListTag={view.kind !== "list"}
                focusSignal={focusSignal}
                sortBy={sortBy}
                onSort={changeSort}
                onSelect={(id) => void openTask(id)}
                onToggleComplete={(task) => void toggleComplete(task)}
                onToggleImportant={(task) => void toggleImportant(task)}
                onQuickAdd={quickAdd}
                onRestore={(task) =>
                  setModal({
                    type: "confirm",
                    title: "恢复任务",
                    message: `将「${task.title}」恢复到原清单？`,
                    label: "恢复",
                    action: async () => {
                      await api.restoreTask(task.id);
                      await refreshAfterMutation();
                      notify("任务已恢复");
                    },
                  })
                }
                onPurge={(task) =>
                  setModal({
                    type: "confirm",
                    title: "永久删除任务",
                    message: `「${task.title}」将被永久删除，无法恢复。`,
                    label: "永久删除",
                    action: async () => {
                      await api.purgeTask(task.id);
                      await refreshAfterMutation();
                      notify("任务已永久删除");
                    },
                  })
                }
                onRestoreList={(list) =>
                  setModal({
                    type: "confirm",
                    title: "恢复清单",
                    message: `恢复清单「${list.name}」及其中任务？`,
                    label: "恢复",
                    action: async () => {
                      await api.restoreList(list.id);
                      await refreshAfterMutation();
                      notify("清单已恢复");
                    },
                  })
                }
                onPurgeList={(list) =>
                  setModal({
                    type: "confirm",
                    title: "永久删除清单",
                    message: `清单「${list.name}」及其中 ${list.taskCount} 个任务将被永久删除。`,
                    label: "永久删除",
                    action: async () => {
                      await api.purgeList(list.id);
                      await refreshAfterMutation();
                      notify("清单已永久删除");
                    },
                  })
                }
                onEmptyDeleted={() =>
                  setModal({
                    type: "confirm",
                    title: "清空最近删除",
                    message: "最近删除中的所有任务和清单将被永久删除，无法恢复。",
                    label: "全部永久删除",
                    action: async () => {
                      await api.emptyDeleted();
                      await refreshAfterMutation();
                      notify("最近删除已清空");
                    },
                  })
                }
              />
            </>
          )}
        </div>
      </main>

      {panel ? (
        <div className={`offline-overlay ${panel.type === "nav" ? "nav" : "detail"}`}>
          <button className="offline-backdrop" onClick={closePanel} aria-label="关闭面板" />
          {panel.type === "nav" ? (
            <aside className="offline-nav-drawer">
              <div className="offline-drawer-brand">
                <div className="offline-drawer-logo" aria-hidden="true">
                  <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="m5 12.5 4.2 4.2L19 6.8" />
                  </svg>
                </div>
                <div className="offline-drawer-brand-copy">
                  <strong>Local To Do</strong>
                  <span>独立离线任务管理</span>
                </div>
                <WeekPicker
                  label={`第${currentWeekNumber(settings?.weekCustomized ? settings.weekBaseDate : null)}周`}
                  anchorDate={settings?.weekCustomized ? settings.weekBaseDate : null}
                  onSelect={(date) => {
                    if (!settings) return;
                    void api.updateSettings({
                      ...settings,
                      weekBaseDate: date,
                      weekCustomized: true,
                    })
                      .then(setSettings)
                      .catch((error) => notify(String(error), true));
                  }}
                />
              </div>
              <div className="offline-drawer-scroll">
                {navItems.map((item) => (
                  <button
                    key={viewKey(item.view)}
                    className={`offline-drawer-item ${currentKey === viewKey(item.view) ? "active" : ""}`}
                    onClick={() => selectView(item.view)}
                  >
                    {item.icon}
                    <span>{item.label}</span>
                    {item.count !== undefined ? <small>{item.count}</small> : null}
                  </button>
                ))}
                <div className="offline-drawer-section">清单</div>
                {lists.map((list) => (
                  <button
                    key={list.id}
                    className={`offline-drawer-item ${currentKey === `list:${list.id}` ? "active" : ""}`}
                    onClick={() => selectView({ kind: "list", listId: list.id })}
                  >
                    <span className="dot" style={{ background: list.color }} />
                    <span>{list.name}</span>
                    <small>{list.openCount}</small>
                  </button>
                ))}
                <button className="offline-drawer-item" onClick={() => {
                  closePanel();
                  setModal({ type: "list" });
                }}>
                  <IconPlus />
                  <span>新建清单</span>
                </button>
              </div>
              <div className="offline-drawer-footer">
                <IconBell size={15} />
                <span>{settings?.notificationsEnabled ? "提醒已开启" : "提醒已关闭"}</span>
              </div>
            </aside>
          ) : (
            <aside className="offline-detail-panel">
              {panel.type === "task" ? (
                selectedTask ? (
                  <TaskDetails
                    task={selectedTask}
                    lists={lists}
                    onClose={closePanel}
                    onSave={saveTask}
                    onDelete={requestDeleteTask}
                    onToggleComplete={(task) => void toggleComplete(task)}
                    onToggleImportant={(task) => void toggleImportant(task)}
                    onToggleMyDay={(task) => void toggleMyDay(task)}
                    onRefreshTask={(task) => {
                      setSelectedTask(task);
                      void refreshAfterMutation();
                    }}
                    notify={notify}
                  />
                ) : (
                  <div className="offline-panel-loading">正在加载任务详情…</div>
                )
              ) : activeNote ? (
                <MobileNoteDetail
                  note={activeNote}
                  onClose={closePanel}
                  onSaved={(note) => {
                    setNotes((items) => items.map((item) => (item.id === note.id ? note : item)));
                    void refreshMeta();
                  }}
                  onDelete={(note) =>
                    setModal({
                      type: "confirm",
                      title: "删除备忘录",
                      message: `确定删除「${note.title.trim() || "无标题备忘录"}」吗？`,
                      label: "删除",
                      action: async () => {
                        await api.deleteNote(note.id);
                        setNotes((items) => items.filter((item) => item.id !== note.id));
                        closePanel();
                        await refreshAfterMutation();
                        notify("备忘录已删除");
                      },
                    })
                  }
                  notify={notify}
                />
              ) : (
                <div className="offline-panel-loading">正在加载备忘录…</div>
              )}
            </aside>
          )}
        </div>
      ) : null}

      {modal?.type === "list" ? (
        <ListModal
          list={modal.list}
          lists={lists}
          onClose={() => setModal(null)}
          onDone={(message) => {
            setModal(null);
            void refreshAfterMutation().then(() => notify(message));
          }}
          notify={notify}
        />
      ) : null}

      {modal?.type === "deleteTask" ? (
        <ConfirmModal
          title="删除重复任务"
          message={`「${modal.task.title}」是重复任务。可以仅删除当前记录，或删除整个重复系列。`}
          onClose={() => setModal(null)}
          actions={[
            {
              label: "仅删除此任务",
              kind: "primary",
              onClick: () => deleteTask(modal.task, "single"),
            },
            {
              label: "删除整个系列",
              kind: "danger",
              onClick: () => deleteTask(modal.task, "series"),
            },
          ]}
        />
      ) : null}

      {modal?.type === "confirm" ? (
        <ConfirmModal
          title={modal.title}
          message={modal.message}
          onClose={() => setModal(null)}
          actions={[
            {
              label: modal.label,
              kind: "danger",
              onClick: async () => {
                try {
                  await modal.action();
                  setModal(null);
                } catch (error) {
                  notify(String(error), true);
                }
              },
            },
          ]}
        />
      ) : null}

      {toast ? <div className={`offline-toast ${toast.error ? "error" : ""}`}>{toast.message}</div> : null}
    </div>
  );
}
