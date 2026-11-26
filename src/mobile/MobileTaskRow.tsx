import type { Task } from "../api";
import { IconCheckCircle, IconRepeat, IconStar } from "../components/Icons";
import { formatDueLabel } from "../utils/date";

interface MobileTaskRowProps {
  task: Task;
  onOpen: (id: string) => void;
}

/**
 * 只读任务行：点击仅打开详情，不提供完成/编辑等任何写操作。
 */
export function MobileTaskRow({ task, onOpen }: MobileTaskRowProps) {
  const due = formatDueLabel(task.dueDate, task.dueTime);
  const completedSteps = task.steps.filter((step) => step.isCompleted).length;

  return (
    <button
      type="button"
      className={`mobile-task${task.isCompleted ? " completed" : ""}`}
      onClick={() => onOpen(task.id)}
    >
      <span className="mobile-task-status" aria-hidden="true">
        {task.isCompleted ? (
          <IconCheckCircle size={21} />
        ) : (
          <span className="mobile-task-ring" style={{ borderColor: task.listColor || undefined }} />
        )}
      </span>
      <span className="mobile-task-main">
        <span className="mobile-task-title">{task.title}</span>
        <span className="mobile-task-meta">
          {task.isImportant && (
            <span className="mobile-task-icon star" title="重要">
              <IconStar size={13} filled />
            </span>
          )}
          {due && <span className="mobile-task-due">{due}</span>}
          <span className="mobile-task-list">
            <i style={{ background: task.listColor || "var(--muted)" }} />
            {task.listName}
          </span>
          {task.recurrence && (
            <span className="mobile-task-icon" title="重复任务">
              <IconRepeat size={13} />
            </span>
          )}
          {task.steps.length > 0 && (
            <span className="mobile-task-steps">
              {completedSteps}/{task.steps.length}
            </span>
          )}
        </span>
      </span>
      {task.isCompleted && <span className="mobile-task-flag">已完成</span>}
    </button>
  );
}
