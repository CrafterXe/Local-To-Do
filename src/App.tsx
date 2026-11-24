import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type AppInfo,
  type AppSettings,
  type BackupEntry,
  type DbStats,
  type DeletedList,
  type HistoryEntry,
  type Note,
  type SidebarCounts,
  type Task,
  type TaskInput,
  type TaskList as TaskListModel,
  type View,
} from "./api";
import { Sidebar, viewIcon, viewTitle } from "./components/Sidebar";
import { TaskList } from "./components/TaskList";
import { TaskDetails } from "./components/TaskDetails";
import { NotesView } from "./components/NotesView";
import { SettingsView } from "./components/SettingsView";
import { HistoryView } from "./components/HistoryView";
import { ListModal } from "./components/ListModal";
import { ConfirmModal } from "./components/Modal";
import { IconBell, IconPlus, IconRefresh } from "./components/Icons";
import { formatDueLabel, setDisplayTimezone, todayString } from "./utils/date";
import type { SortKind } from "./utils/sort";

type ModalState =
  | { type: "list"; list?: TaskListModel }
  | { type: "deleteTask"; task: Task }
  | { type: "purgeTask"; task: Task }
  | { type: "purgeList"; list: DeletedList }
  | { type: "emptyDeleted" }
  | { type: "deleteNote"; note: Note }
  | null;

function viewKey(v: View): string {
  return v.kind === "list" ? `list:${v.listId ?? ""}` : v.kind;
}

function readSort(key: string): SortKind {
  try {
    const saved = window.localStorage.getItem(`ltd.sort.${key}`);
    return saved === "name" || saved === "created" || saved === "due" ? saved : null;
  } catch {
    return null;
  }
}

export default function App() {
  const [lists, setLists] = useState<TaskListModel[]>([]);
  const [counts, setCounts] = useState<SidebarCounts | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [stats, setStats] = useState<DbStats | null>(null);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [nextReminder, setNextReminder] = useState<number | null>(null);

  const [view, setView] = useState<View>({ kind: "myDay" });
  const [tasks, setTasks] = useState<Task[]>([]);
  const [deletedLists, setDeletedLists] = useState<DeletedList[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [notes, setNotes] = useState<Note[]>([]);
  const [selectedNoteId, setSelectedNoteId] = useState<string | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [search, setSearch] = useState("");
  const [toast, setToast] = useState<{ msg: string; error: boolean } | null>(null);
  const [notifiedTasks, setNotifiedTasks] = useState<Task[]>([]);
  const [reminderOpen, setReminderOpen] = useState(false);
  const [sortBy, setSortBy] = useState<SortKind>(() => readSort("myDay"));
  const [focusTaskId, setFocusTaskId] = useState<string | null>(null);
  const [modal, setModal] = useState<ModalState>(null);
  const [focusSignal, setFocusSignal] = useState(0);
  const [tick, setTick] = useState(0);
  const [booted, setBooted] = useState(false);
  const tickRef = useRef(0);

  const notify = useCallback((msg: string, error = false) => {
    setToast({ msg, error });
  }, []);

  const bump = useCallback(() => {
    tickRef.current += 1;
    setTick(tickRef.current);
  }, []);

  useEffect(() => {
    if (!toast) return;
    const t = window.setTimeout(() => setToast(null), 4200);
    return () => window.clearTimeout(t);
  }, [toast]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings?.theme === "light" ? "light" : "dark";
  }, [settings?.theme]);

  const refreshMeta = useCallback(async () => {
    try {
      const [ls, cs, st, nr] = await Promise.all([
        api.listLists(),
        api.sidebarCounts(),
        api.getSettings(),
        api.getNextReminder(),
      ]);
      setLists(ls);
      setCounts(cs);
      setSettings(st);
      setDisplayTimezone(st.timezoneAuto ? null : st.timezone);
      setNextReminder(nr);
    } catch (e) {
      notify(`加载数据失败：${String(e)}`, true);
    }
  }, [notify]);

  const loadSystemInfo = useCallback(async () => {
    try {
      const [i, s, b] = await Promise.all([api.getAppInfo(), api.getDbStats(), api.listBackups()]);
      setInfo(i);
      setStats(s);
      setBackups(b);
    } catch (e) {
      notify(String(e), true);
    }
  }, [notify]);

  useEffect(() => {
    void (async () => {
      await refreshMeta();
      await loadSystemInfo();
      setBooted(true);
    })();
  }, [refreshMeta, loadSystemInfo]);

  // 进入设置页或数据变动后刷新统计与备份列表
  useEffect(() => {
    if (!booted || view.kind !== "settings") return;
    void (async () => {
      await refreshMeta();
      await loadSystemInfo();
    })();
  }, [booted, view.kind, tick, refreshMeta, loadSystemInfo]);

  // 任务列表加载
  useEffect(() => {
    if (!booted) return;
    if (view.kind === "notes" || view.kind === "settings" || view.kind === "history") return;
    let cancelled = false;
    const q = search.trim();
    const query = q
      ? { view: "all", search: q }
      : {
          view: view.kind === "list" ? "list" : view.kind,
          listId: view.listId ?? null,
        };
    api
      .listTasks(query)
      .then((data) => {
        if (!cancelled) setTasks(data);
      })
      .catch((e) => notify(String(e), true));
    return () => {
      cancelled = true;
    };
  }, [view, search, tick, booted, notify]);

  // 最近删除中的清单加载
  useEffect(() => {
    if (!booted || view.kind !== "deleted") return;
    let cancelled = false;
    api
      .listDeletedLists()
      .then((data) => {
        if (!cancelled) setDeletedLists(data);
      })
      .catch((e) => notify(String(e), true));
    return () => {
      cancelled = true;
    };
  }, [view, tick, booted, notify]);

  // 笔记加载
  useEffect(() => {
    if (!booted) return;
    let cancelled = false;
    api
      .listNotes(search.trim() || undefined)
      .then((data) => {
        if (cancelled) return;
        setNotes(data);
        setSelectedNoteId((cur) => (cur && data.some((n) => n.id === cur) ? cur : data[0]?.id ?? null));
      })
      .catch((e) => notify(String(e), true));
    return () => {
      cancelled = true;
    };
  }, [search, tick, booted, notify]);

  // 历史加载
  useEffect(() => {
    if (!booted || view.kind !== "history") return;
    let cancelled = false;
    api
      .listHistory(300)
      .then((data) => {
        if (!cancelled) setHistory(data);
      })
      .catch((e) => notify(String(e), true));
    return () => {
      cancelled = true;
    };
  }, [view, tick, booted, notify]);

  // 已触发提醒但未完成的任务（提醒栏）
  useEffect(() => {
    if (!booted) return;
    let cancelled = false;
    api
      .listNotifiedTasks()
      .then((data) => {
        if (!cancelled) setNotifiedTasks(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [booted, tick]);

  // 每个视图独立记忆排序方式
  useEffect(() => {
    setSortBy(readSort(viewKey(view)));
  }, [view]);

  // 系统唤醒 / 窗口重新获得焦点后，按最新系统时间与时区校准提醒
  useEffect(() => {
    if (!booted) return;
    const resync = () => {
      void api
        .resyncReminders()
        .then(() => {
          bump();
          void refreshMeta();
        })
        .catch(() => {});
    };
    window.addEventListener("focus", resync);
    return () => window.removeEventListener("focus", resync);
  }, [booted, bump, refreshMeta]);

  // 系统事件
  useEffect(() => {
    const un1 = listen<{ title: string }>("reminder-fired", () => {
      bump();
      void refreshMeta();
    });
    const un2 = listen("new-task-requested", () => {
      setFocusSignal((s) => s + 1);
    });
    const un3 = listen("data-restored", () => {
      bump();
      void refreshMeta();
    });
    return () => {
      void un1.then((f) => f());
      void un2.then((f) => f());
      void un3.then((f) => f());
    };
  }, [bump, refreshMeta]);

  const refreshSelected = useCallback(
    async (id?: string) => {
      const target = id ?? selectedId;
      if (!target) return;
      try {
        const t = await api.getTask(target);
        setSelectedTask(t);
      } catch {
        setSelectedTask(null);
        setSelectedId(null);
      }
    },
    [selectedId],
  );

  const selectView = (v: View) => {
    setView(v);
    setSearch("");
    setSelectedId(null);
    setSelectedTask(null);
  };

  const selectTask = async (id: string) => {
    setSelectedId(id);
    try {
      setSelectedTask(await api.getTask(id));
    } catch (e) {
      notify(String(e), true);
    }
  };

  const changeSort = (kind: Exclude<SortKind, null>) => {
    setSortBy(kind);
    try {
      window.localStorage.setItem(`ltd.sort.${viewKey(view)}`, kind);
    } catch {
      // 忽略无法写入的存储（隐私模式等）
    }
  };

  const jumpToNotified = (task: Task) => {
    setReminderOpen(false);
    setSearch("");
    setSelectedId(null);
    setSelectedTask(null);
    setView({ kind: "all" });
    setFocusTaskId(task.id);
  };

  // 跳转后定位并柔和闪动两次
  useEffect(() => {
    if (!focusTaskId || view.kind !== "all") return;
    const el = document.querySelector<HTMLElement>(`[data-task-id="${focusTaskId}"]`);
    if (!el) return;
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    el.classList.remove("task-flash");
    void el.offsetWidth;
    el.classList.add("task-flash");
    const timer = window.setTimeout(() => {
      el.classList.remove("task-flash");
      setFocusTaskId(null);
    }, 1500);
    return () => {
      window.clearTimeout(timer);
      el.classList.remove("task-flash");
    };
  }, [focusTaskId, view.kind, tasks]);

  const quickAdd = async (title: string) => {
    const listId =
      view.kind === "list" && view.listId
        ? view.listId
        : settings?.defaultListId ?? lists[0]?.id ?? null;
    const myDay = view.kind === "myDay" ? todayString() : null;
    const input: TaskInput = {
      title,
      listId,
      note: null,
      isImportant: view.kind === "important" ? true : null,
      dueDate: null,
      dueTime: null,
      reminderOffset: null,
      myDay,
      recurrence: null,
    };
    try {
      const created = await api.createTask(input);
      bump();
      await refreshMeta();
      if (view.kind === "myDay" && !created.myDay) {
        notify("任务已创建，但未能加入我的一天", true);
      }
    } catch (e) {
      notify(String(e), true);
    }
  };

  const saveTask = useCallback(
    async (id: string, input: TaskInput) => {
      const updated = await api.updateTask(id, input);
      setSelectedTask(updated);
      bump();
      await refreshMeta();
    },
    [bump, refreshMeta],
  );

  const toggleComplete = async (task: Task) => {
    try {
      await api.completeTask(task.id, !task.isCompleted);
      bump();
      await refreshMeta();
      if (selectedId === task.id) await refreshSelected(task.id);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const toggleImportant = async (task: Task) => {
    try {
      const updated = await api.setImportant(task.id, !task.isImportant);
      setTasks((ts) => ts.map((x) => (x.id === updated.id ? updated : x)));
      if (selectedTask?.id === updated.id) setSelectedTask(updated);
      await refreshMeta();
    } catch (e) {
      notify(String(e), true);
    }
  };

  const toggleMyDay = async (task: Task) => {
    try {
      const updated = await api.setMyDay(task.id, !task.myDay);
      setTasks((ts) => ts.map((x) => (x.id === updated.id ? updated : x)));
      if (selectedTask?.id === updated.id) setSelectedTask(updated);
      await refreshMeta();
      notify(updated.myDay ? "已加入「我的一天」" : "已从「我的一天」移除");
    } catch (e) {
      notify(String(e), true);
    }
  };

  const closeDetails = () => {
    setSelectedTask(null);
    setSelectedId(null);
  };

  const requestDelete = (task: Task) => {
    if (task.recurrence) {
      setModal({ type: "deleteTask", task });
    } else {
      void (async () => {
        try {
          await api.deleteTask(task.id, "single");
          closeDetails();
          bump();
          await refreshMeta();
          notify("任务已移入「最近删除」");
        } catch (e) {
          notify(String(e), true);
        }
      })();
    }
  };

  const skipTask = async (task: Task) => {
    try {
      const updated = await api.skipOccurrence(task.id);
      setSelectedTask(updated);
      bump();
      await refreshMeta();
      notify("已跳过本次，到期时间已推进到下一次");
    } catch (e) {
      notify(String(e), true);
    }
  };

  const createNote = async () => {
    try {
      const note = await api.createNote("", "");
      bump();
      setSelectedNoteId(note.id);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const saveNote = useCallback(
    async (id: string, title: string, body: string) => {
      const updated = await api.updateNote(id, title, body);
      setNotes((ns) => ns.map((n) => (n.id === updated.id ? updated : n)));
    },
    [],
  );

  const updateSettings = async (patch: Partial<AppSettings>) => {
    if (!settings) return;
    try {
      const updated = await api.updateSettings({ ...settings, ...patch });
      setSettings(updated);
      setDisplayTimezone(updated.timezoneAuto ? null : updated.timezone);
      setNextReminder(await api.getNextReminder());
    } catch (e) {
      notify(String(e), true);
    }
  };

  const resetAllSettings = async () => {
    try {
      const updated = await api.resetSettings();
      setSettings(updated);
      setDisplayTimezone(null);
      setSortBy(null);
      try {
        Object.keys(window.localStorage)
          .filter((k) => k.startsWith("ltd.sort."))
          .forEach((k) => window.localStorage.removeItem(k));
      } catch {
        // 忽略存储异常
      }
      bump();
      await refreshMeta();
      notify("已恢复默认设置");
    } catch (e) {
      notify(`重置失败：${String(e)}`, true);
    }
  };

  const reorderLists = async (ids: string[]) => {
    const byId = new Map(lists.map((l) => [l.id, l]));
    setLists(ids.map((id) => byId.get(id)!).filter(Boolean));
    try {
      await api.reorderLists(ids);
    } catch (e) {
      notify(String(e), true);
      await refreshMeta();
    }
  };

  const restoreList = async (list: DeletedList) => {
    try {
      await api.restoreList(list.id);
      bump();
      await refreshMeta();
      notify(`清单「${list.name}」已恢复`);
    } catch (e) {
      notify(String(e), true);
    }
  };

  // 当前清单被删除或已不存在时，回到「全部任务」
  useEffect(() => {
    if (!booted || lists.length === 0) return;
    if (view.kind === "list" && view.listId && !lists.some((l) => l.id === view.listId)) {
      setView({ kind: "all" });
      setSelectedId(null);
      setSelectedTask(null);
    }
  }, [lists, view, booted]);

  const subtitle = useMemo(() => {
    if (view.kind === "settings") return "提醒、备份与系统集成";
    if (view.kind === "notes") return `${notes.length} 条笔记 · 自动保存`;
    if (view.kind === "history") return "最近 300 条任务操作记录";
    if (search.trim()) return `搜索「${search.trim()}」· ${tasks.length} 个结果`;
    if (view.kind === "completed") return `${tasks.length} 个已完成任务`;
    if (view.kind === "deleted")
      return deletedLists.length
        ? `${tasks.length} 个已删除任务 · ${deletedLists.length} 个已删除清单 · 可恢复`
        : `${tasks.length} 个已删除任务 · 可恢复`;
    return `${tasks.length} 个任务`;
  }, [view, notes.length, tasks.length, search, deletedLists.length]);

  if (!booted)
    return (
      <div
        style={{
          height: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          color: "var(--muted)",
          gap: 10,
          background: "#1b1b1b",
          fontSize: 13,
        }}
      >
        正在加载 Local To Do…
      </div>
    );

  const showTaskList =
    view.kind !== "notes" && view.kind !== "settings" && view.kind !== "history";

  return (
    <div className="app">
      <Sidebar
        lists={lists}
        counts={counts}
        view={view}
        search={search}
        onSearch={setSearch}
        onSelect={selectView}
        onCreateList={() => setModal({ type: "list" })}
        onManageList={(l) => setModal({ type: "list", list: l })}
        onReorderLists={(ids) => void reorderLists(ids)}
      />

      <main className="main">
        <header className="main-header">
          <div className="main-title-row">
            <h1 className="main-title">
              {viewIcon(view, lists)}
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {search.trim() ? "搜索结果" : viewTitle(view, lists)}
              </span>
            </h1>
            <div className="header-actions">
              {view.kind === "notes" ? (
                <button className="icon-btn" title="新建笔记" onClick={() => void createNote()}>
                  <IconPlus size={17} />
                </button>
              ) : showTaskList ? (
                <button
                  className="icon-btn"
                  title="新建任务"
                  onClick={() => setFocusSignal((s) => s + 1)}
                >
                  <IconPlus size={18} />
                </button>
              ) : null}
              <button
                className="icon-btn"
                title="刷新"
                onClick={() => {
                  bump();
                  void refreshMeta();
                  void loadSystemInfo();
                }}
              >
                <IconRefresh size={16} />
              </button>
            </div>
          </div>
          <div className="main-subtitle">{subtitle}</div>
        </header>

        {notifiedTasks.length > 0 && (
          <div className="reminder-stack">
            <div
              className={`reminder-banner ${notifiedTasks.length > 1 ? "stacked" : ""} ${
                reminderOpen ? "expanded" : ""
              }`}
              title={
                notifiedTasks.length > 1
                  ? `${notifiedTasks.length} 个任务提醒尚未完成`
                  : undefined
              }
              onClick={() => {
                if (notifiedTasks.length > 1) setReminderOpen((v) => !v);
              }}
            >
              <span className="reminder-icon">
                <IconBell size={15} />
              </span>
              <div className="reminder-main">
                <div className="reminder-title">{notifiedTasks[0].title}</div>
                <div className="reminder-sub">
                  {notifiedTasks[0].dueDate
                    ? formatDueLabel(notifiedTasks[0].dueDate, notifiedTasks[0].dueTime)
                    : notifiedTasks[0].listName}
                </div>
              </div>
              {notifiedTasks.length > 1 && (
                <span className="reminder-count">{notifiedTasks.length}</span>
              )}
            </div>

            {reminderOpen && notifiedTasks.length > 1 && (
              <div className="reminder-list">
                {notifiedTasks.map((t) => (
                  <button
                    key={t.id}
                    className="reminder-item"
                    onClick={() => jumpToNotified(t)}
                  >
                    <span className="reminder-item-title">{t.title}</span>
                    <span className="reminder-item-meta">
                      {t.dueDate ? formatDueLabel(t.dueDate, t.dueTime) : t.listName}
                    </span>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        {view.kind === "notes" ? (
          <NotesView
            notes={notes}
            selectedId={selectedNoteId}
            searchActive={!!search.trim()}
            onSelect={setSelectedNoteId}
            onCreate={createNote}
            onSave={saveNote}
            onDelete={(note) => setModal({ type: "deleteNote", note })}
            notify={notify}
          />
        ) : view.kind === "settings" && settings ? (
          <SettingsView
            settings={settings}
            info={info}
            stats={stats}
            backups={backups}
            nextReminder={nextReminder}
            onUpdate={updateSettings}
            onReset={resetAllSettings}
            onRefresh={async () => {
              await refreshMeta();
              await loadSystemInfo();
            }}
            notify={notify}
          />
        ) : view.kind === "history" ? (
          <HistoryView entries={history} />
        ) : (
          <TaskList
            tasks={tasks}
            view={view}
            deletedLists={deletedLists}
            selectedId={selectedId}
            searchActive={!!search.trim()}
            showListTag={view.kind !== "list"}
            focusSignal={focusSignal}
            sortBy={sortBy}
            onSort={changeSort}
            onSelect={(id) => void selectTask(id)}
            onToggleComplete={(t) => void toggleComplete(t)}
            onToggleImportant={(t) => void toggleImportant(t)}
            onQuickAdd={quickAdd}
            onRestore={(t) => {
              void (async () => {
                try {
                  await api.restoreTask(t.id);
                  bump();
                  await refreshMeta();
                  notify("任务已恢复");
                } catch (e) {
                  notify(String(e), true);
                }
              })();
            }}
            onPurge={(t) => setModal({ type: "purgeTask", task: t })}
            onRestoreList={(l) => void restoreList(l)}
            onPurgeList={(l) => setModal({ type: "purgeList", list: l })}
            onEmptyDeleted={() => setModal({ type: "emptyDeleted" })}
          />
        )}
      </main>

      {selectedTask && showTaskList && (
        <TaskDetails
          task={selectedTask}
          lists={lists}
          onClose={closeDetails}
          onSave={saveTask}
          onDelete={requestDelete}
          onSkip={(t) => void skipTask(t)}
          onToggleImportant={(t) => void toggleImportant(t)}
          onToggleMyDay={(t) => void toggleMyDay(t)}
          onRefreshTask={async (t) => {
            setSelectedTask(t);
            bump();
            await refreshMeta();
          }}
          notify={notify}
        />
      )}

      {modal?.type === "list" && (
        <ListModal
          list={modal.list}
          lists={lists}
          onClose={() => setModal(null)}
          onDone={async (msg) => {
            notify(msg);
            bump();
            await refreshMeta();
          }}
          notify={notify}
        />
      )}

      {modal?.type === "deleteTask" && (
        <ConfirmModal
          title="删除重复任务"
          message={`「${modal.task.title}」是一个重复任务。请选择删除方式：\n\n· 仅删除此任务：保留重复系列，不影响其他周期\n· 删除整个系列：删除该重复任务的全部未删除实例`}
          onClose={() => setModal(null)}
          actions={[
            { label: "取消", kind: "default", onClick: () => setModal(null) },
            {
              label: "仅删除此任务",
              kind: "primary",
              onClick: async () => {
                await api.deleteTask(modal.task.id, "single");
                setModal(null);
                closeDetails();
                bump();
                await refreshMeta();
                notify("任务已移入「最近删除」");
              },
            },
            {
              label: "删除整个系列",
              kind: "danger",
              onClick: async () => {
                await api.deleteTask(modal.task.id, "series");
                setModal(null);
                closeDetails();
                bump();
                await refreshMeta();
                notify("整个重复系列已移入「最近删除」");
              },
            },
          ]}
        />
      )}

      {modal?.type === "purgeTask" && (
        <ConfirmModal
          title="永久删除任务"
          message={`「${modal.task.title}」将被永久删除，无法恢复。确定继续吗？`}
          onClose={() => setModal(null)}
          actions={[
            { label: "取消", kind: "default", onClick: () => setModal(null) },
            {
              label: "永久删除",
              kind: "danger",
              onClick: async () => {
                await api.purgeTask(modal.task.id);
                setModal(null);
                bump();
                await refreshMeta();
                notify("任务已永久删除");
              },
            },
          ]}
        />
      )}

      {modal?.type === "purgeList" && (
        <ConfirmModal
          title="永久删除清单"
          message={`清单「${modal.list.name}」${modal.list.taskCount ? `及其中的 ${modal.list.taskCount} 个任务` : ""}将被永久删除，无法恢复。确定继续吗？`}
          onClose={() => setModal(null)}
          actions={[
            { label: "取消", kind: "default", onClick: () => setModal(null) },
            {
              label: "永久删除",
              kind: "danger",
              onClick: async () => {
                await api.purgeList(modal.list.id);
                setModal(null);
                bump();
                await refreshMeta();
                notify("清单已永久删除");
              },
            },
          ]}
        />
      )}

      {modal?.type === "emptyDeleted" && (
        <ConfirmModal
          title="清空最近删除"
          message="最近删除中的所有任务与清单将被永久删除，无法恢复。确定继续吗？"
          onClose={() => setModal(null)}
          actions={[
            { label: "取消", kind: "default", onClick: () => setModal(null) },
            {
              label: "永久删除",
              kind: "danger",
              onClick: async () => {
                await api.emptyDeleted();
                setModal(null);
                bump();
                await refreshMeta();
                notify("最近删除已清空");
              },
            },
          ]}
        />
      )}

      {modal?.type === "deleteNote" && (
        <ConfirmModal
          title="删除笔记"
          message={`确定删除笔记「${modal.note.title.trim() || "无标题笔记"}」吗？删除后无法恢复。`}
          onClose={() => setModal(null)}
          actions={[
            { label: "取消", kind: "default", onClick: () => setModal(null) },
            {
              label: "删除",
              kind: "danger",
              onClick: async () => {
                await api.deleteNote(modal.note.id);
                setModal(null);
                bump();
                await refreshMeta();
                notify("笔记已删除");
              },
            },
          ]}
        />
      )}

      {toast && <div className={`toast ${toast.error ? "error" : ""}`}>{toast.msg}</div>}
    </div>
  );
}
