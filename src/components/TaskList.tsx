import { useEffect, useRef, useState } from "react";
import type { Task, View } from "../api";
import { formatDueLabel, formatTimestamp, todayString } from "../utils/date";
import { IconCheck, IconNote, IconPlus, IconRepeat, IconStar, IconTrash, IconUpload } from "./Icons";
import { EmptyState } from "./EmptyState";

interface Props {
  tasks: Task[];
  view: View;
  selectedId: string | null;
  searchActive: boolean;
  showListTag: boolean;
  focusSignal: number;
  onSelect: (id: string) => void;
  onToggleComplete: (task: Task) => void;
  onToggleImportant: (task: Task) => void;
  onQuickAdd: (title: string) => void | Promise<void>;
  onRestore?: (task: Task) => void;
  onPurge?: (task: Task) => void;
}

export function TaskList({
  tasks,
  view,
  selectedId,
  searchActive,
  showListTag,
  focusSignal,
  onSelect,
  onToggleComplete,
  onToggleImportant,
  onQuickAdd,
  onRestore,
  onPurge,
}: Props) {
  const [draft, setDraft] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const today = todayString();

  useEffect(() => {
    if (focusSignal > 0) inputRef.current?.focus();
  }, [focusSignal]);

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
    <div className="task-scroll">
      {showQuickAdd && (
        <div className="quick-add">
          <span className="plus-wrap"><IconPlus size={17} /></span>
          <input
            ref={inputRef}
            value={draft}
            placeholder="添加任务，按 Enter 保存"
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
              if (e.key === "Escape") setDraft("");
            }}
          />
          {draft.trim() ? (
            <button className="quick-add-btn" onClick={() => void submit()}>
              添加
            </button>
          ) : (
            <span className="quick-hint">Enter 添加</span>
          )}
        </div>
      )}

      {tasks.length === 0 ? (
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
        tasks.map((t) => {
          const overdue =
            !t.isCompleted && t.dueDate !== null && t.dueDate < today;
          const isToday = t.dueDate === today;
          const stepDone = t.steps.filter((s) => s.isCompleted).length;
          return (
            <div
              key={t.id}
              className={`task-row ${t.isCompleted ? "completed" : ""} ${
                selectedId === t.id ? "selected" : ""
              }`}
              onClick={() => onSelect(t.id)}
            >
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
    </div>
  );
}
