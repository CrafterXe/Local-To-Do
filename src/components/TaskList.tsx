import { useEffect, useMemo, useRef, useState } from "react";
import type { DeletedList, Task, View } from "../api";
import { formatDueLabel, formatTimestamp, todayString } from "../utils/date";
import { sortTasks, type SortKind } from "../utils/sort";
import {
  IconCheck,
  IconChevronRight,
  IconList,
  IconNote,
  IconPlus,
  IconRepeat,
  IconStar,
  IconTrash,
  IconUpload,
} from "./Icons";
import { EmptyState } from "./EmptyState";

const SORT_OPTIONS: { key: Exclude<SortKind, null>; label: string }[] = [
  { key: "name", label: "按名称排序" },
  { key: "created", label: "按创建时间排序" },
  { key: "due", label: "按到期日期排序" },
];

interface Props {
  tasks: Task[];
  view: View;
  deletedLists?: DeletedList[];
  selectedId: string | null;
  searchActive: boolean;
  showListTag: boolean;
  focusSignal: number;
  sortBy: SortKind;
  onSort: (kind: Exclude<SortKind, null>) => void;
  onSelect: (id: string) => void;
  onToggleComplete: (task: Task) => void;
  onToggleImportant: (task: Task) => void;
  onQuickAdd: (title: string) => void | Promise<void>;
  onRestore?: (task: Task) => void;
  onPurge?: (task: Task) => void;
  onRestoreList?: (list: DeletedList) => void;
  onPurgeList?: (list: DeletedList) => void;
  onEmptyDeleted?: () => void;
}

export function TaskList({
  tasks,
  view,
  deletedLists = [],
  selectedId,
  searchActive,
  showListTag,
  focusSignal,
  sortBy,
  onSort,
  onSelect,
  onToggleComplete,
  onToggleImportant,
  onQuickAdd,
  onRestore,
  onPurge,
  onRestoreList,
  onPurgeList,
  onEmptyDeleted,
}: Props) {
  const [draft, setDraft] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [subOpen, setSubOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const today = todayString();

  useEffect(() => {
    if (focusSignal > 0) inputRef.current?.focus();
  }, [focusSignal]);

  useEffect(() => {
    if (!menu) return;
    const close = () => {
      setMenu(null);
      setSubOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", close);
    };
  }, [menu]);

  const ordered = useMemo(() => sortTasks(tasks, sortBy), [tasks, sortBy]);

  const openMenu = (e: React.MouseEvent) => {
    const el = e.target as HTMLElement;
    if (el.closest("input, textarea, select, [contenteditable='true']")) return;
    e.preventDefault();
    setSubOpen(false);
    setMenu({
      x: Math.min(e.clientX, window.innerWidth - 360),
      y: Math.min(e.clientY, window.innerHeight - 180),
    });
  };

  const readOnly = view.kind === "completed" || view.kind === "deleted";
  const showQuickAdd = !readOnly && !searchActive;

  const submit = async () => {
    const t = draft.trim();
    if (!t) return;
    setDraft("");
    await onQuickAdd(t);
    inputRef.current?.focus();
  };

  return (
    <div className="task-scroll" onContextMenu={openMenu}>
      {showQuickAdd && (
        <div className="quick-add">
          <span className="plus-wrap"><IconPlus size={17} /></span>
          <input
            ref={inputRef}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
              if (e.key === "Escape") setDraft("");
            }}
          />
          {draft.trim() && (
            <button className="quick-add-btn" onClick={() => void submit()}>
              添加
            </button>
          )}
        </div>
      )}

      {view.kind === "deleted" && (deletedLists.length > 0 || tasks.length > 0) && (
        <div className="deleted-toolbar">
          <span className="deleted-toolbar-title">最近删除</span>
          <button className="text-btn danger" onClick={() => onEmptyDeleted?.()}>
            清空
          </button>
        </div>
      )}

      {deletedLists.map((l) => (
        <div key={l.id} className="task-row deleted-list-row">
          <span className="nav-icon" style={{ marginLeft: 4 }}>
            <span className="dot" style={{ background: l.color }} />
          </span>
          <div className="task-body">
            <div className="task-title">{l.name}</div>
            <div className="task-meta">
              <span className="meta-item">清单</span>
              <span className="meta-item">{l.taskCount} 个任务</span>
              {l.deletedAt && (
                <span className="meta-item">删除于 {formatTimestamp(l.deletedAt)}</span>
              )}
            </div>
          </div>
          <div className="task-side">
            <button
              className="icon-btn"
              title="恢复清单"
              onClick={() => onRestoreList?.(l)}
            >
              <IconUpload size={15} />
            </button>
            <button
              className="icon-btn"
              title="永久删除清单"
              onClick={() => onPurgeList?.(l)}
            >
              <IconTrash size={15} />
            </button>
          </div>
        </div>
      ))}

      {view.kind === "deleted" && deletedLists.length > 0 && tasks.length > 0 && (
        <div className="deleted-subtitle">已删除任务</div>
      )}

      {tasks.length === 0 && deletedLists.length === 0 ? (
        <EmptyState
          text={
            searchActive
              ? "没有找到匹配的任务"
              : view.kind === "completed"
                ? "还没有已完成的任务"
                : view.kind === "deleted"
                  ? "回收站是空的"
                  : view.kind === "myDay"
                    ? "把任务加入「我的一天」，专注今天要做的事"
                    : "这里还没有任务，在上方输入框添加第一条"
          }
        />
      ) : (
        ordered.map((t) => {
          const overdue =
            !t.isCompleted && t.dueDate !== null && t.dueDate < today;
          const isToday = t.dueDate === today;
          const stepDone = t.steps.filter((s) => s.isCompleted).length;
          return (
            <div
              key={t.id}
              data-task-id={t.id}
              className={`task-row ${t.isCompleted ? "completed" : ""} ${
                selectedId === t.id ? "selected" : ""
              }`}
              onClick={() => {
                if (view.kind !== "deleted") onSelect(t.id);
              }}
            >
              {view.kind === "deleted" ? (
                <span className="check-spacer" />
              ) : (
                <button
                  className={`check ${t.isCompleted ? "checked" : ""}`}
                  title={t.isCompleted ? "恢复为未完成" : "标记完成"}
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleComplete(t);
                  }}
                >
                  <IconCheck size={12} />
                </button>
              )}

              <div className="task-body">
                <div className="task-title">{t.title}</div>
                <div className="task-meta">
                  {t.dueDate && (
                    <span className={`meta-item ${overdue ? "overdue" : isToday ? "today" : ""}`}>
                      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
                        <rect x="3.5" y="5" width="17" height="16" rx="2.5" />
                        <path d="M3.5 9.5h17M8 3v4M16 3v4" />
                      </svg>
                      {formatDueLabel(t.dueDate, t.dueTime)}
                    </span>
                  )}
                  {t.recurrence && (
                    <span className="meta-item">
                      <IconRepeat size={12} />
                      重复
                    </span>
                  )}
                  {t.steps.length > 0 && (
                    <span className="meta-item">
                      {stepDone}/{t.steps.length} 步骤
                    </span>
                  )}
                  {t.note.trim() && (
                    <span className="meta-item" title="包含备注">
                      <IconNote size={12} />
                    </span>
                  )}
                  {t.isCompleted && t.completedAt && (
                    <span className="meta-item">完成于 {formatTimestamp(t.completedAt)}</span>
                  )}
                  {view.kind === "deleted" && t.deletedAt && (
                    <span className="meta-item">删除于 {formatTimestamp(t.deletedAt)}</span>
                  )}
                  {showListTag && t.listName && <span className="list-tag">{t.listName}</span>}
                </div>
              </div>

              <div className="task-side">
                {view.kind === "deleted" ? (
                  <>
                    <button
                      className="icon-btn"
                      title="恢复任务"
                      onClick={(e) => {
                        e.stopPropagation();
                        onRestore?.(t);
                      }}
                    >
                      <IconUpload size={15} />
                    </button>
                    <button
                      className="icon-btn"
                      title="永久删除"
                      onClick={(e) => {
                        e.stopPropagation();
                        onPurge?.(t);
                      }}
                    >
                      <IconTrash size={15} />
                    </button>
                  </>
                ) : (
                  <button
                    className={`star-btn ${t.isImportant ? "on" : ""}`}
                    title={t.isImportant ? "取消重要" : "标记为重要"}
                    onClick={(e) => {
                      e.stopPropagation();
                      onToggleImportant(t);
                    }}
                  >
                    <IconStar size={16} filled={t.isImportant} />
                  </button>
                )}
              </div>
            </div>
          );
        })
      )}

      {menu && (
        <div
          className="context-menu"
          style={{ left: menu.x, top: menu.y }}
          onClick={(e) => e.stopPropagation()}
        >
          <div
            className="context-menu-item has-sub"
            onMouseEnter={() => setSubOpen(true)}
            onClick={() => setSubOpen((v) => !v)}
          >
            <span className="cm-icon"><IconList size={14} /></span>
            <span className="cm-label">查看</span>
            <IconChevronRight size={13} className="cm-arrow" />
            {subOpen && (
              <div className="context-submenu">
                {SORT_OPTIONS.map((o) => (
                  <button
                    key={o.key}
                    className={`context-menu-item ${sortBy === o.key ? "selected" : ""}`}
                    onClick={() => {
                      onSort(o.key);
                      setMenu(null);
                      setSubOpen(false);
                    }}
                  >
                    <span className="cm-label">{o.label}</span>
                    {sortBy === o.key && <IconCheck size={13} />}
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
