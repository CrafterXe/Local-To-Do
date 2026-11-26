import { useEffect, useState } from "react";

import { api, type Task } from "../api";
import { IconArrowLeft, IconCheckCircle, IconRepeat, IconStar } from "../components/Icons";
import {
  formatDueFull,
  formatTimestamp,
  recurrenceLabel,
  reminderLabel,
} from "../utils/date";
import { errorText } from "./mobileUtils";

interface MobileTaskDetailProps {
  taskId: string;
  onClose: () => void;
}

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div className="mobile-field">
      <span className="mobile-field-label">{label}</span>
      <span className="mobile-field-value">{value || "—"}</span>
    </div>
  );
}

/** 任务详情：完整展示已有字段，只读，不提供任何编辑入口 */
export function MobileTaskDetail({ taskId, onClose }: MobileTaskDetailProps) {
  const [task, setTask] = useState<Task | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setTask(null);
    setError(null);
    api
      .mobileTask(taskId)
      .then((value) => alive && setTask(value))
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [taskId]);

  return (
    <div className="mobile-detail" role="dialog" aria-modal="true">
      <header className="mobile-detail-header">
        <button className="mobile-icon-button" onClick={onClose} aria-label="返回">
          <IconArrowLeft size={20} />
        </button>
        <span>任务详情</span>
        <span className="mobile-detail-spacer" />
      </header>

      <div className="mobile-detail-body">
        {error && <div className="mobile-alert error">{error}</div>}
        {!task && !error && <div className="mobile-empty">正在载入任务详情…</div>}

        {task && (
          <>
            <div className="mobile-detail-status">
              <span className={`mobile-badge${task.isCompleted ? " phase-synced" : ""}`}>
                {task.isCompleted ? "已完成" : "未完成"}
              </span>
              {task.isImportant && (
                <span className="mobile-detail-star">
                  <IconStar size={15} filled /> 重要
                </span>
              )}
              {task.recurrence && (
                <span className="mobile-detail-repeat">
                  <IconRepeat size={15} /> 重复
                </span>
              )}
            </div>

            <h1 className="mobile-detail-title">{task.title}</h1>

            <div className="mobile-detail-fields">
              <Field label="清单" value={task.listName} />
              <Field label="到期时间" value={formatDueFull(task.dueDate, task.dueTime)} />
              <Field
                label="提醒"
                value={
                  task.reminderOffset === null
                    ? "不提醒"
                    : `${reminderLabel(task.reminderOffset)}${
                        task.reminderEnabled ? "" : "（已关闭）"
                      }`
                }
              />
              <Field
                label="重复"
                value={
                  task.recurrence
                    ? `${recurrenceLabel(task.recurrence)}${
                        task.recurrenceCount > 0 ? ` · 已重复 ${task.recurrenceCount} 次` : ""
                      }`
                    : "不重复"
                }
              />
              <Field label="我的一天" value={task.myDay ?? "未加入"} />
              <Field label="创建时间" value={formatTimestamp(task.createdAt)} />
              <Field label="更新时间" value={formatTimestamp(task.updatedAt)} />
              {task.completedAt !== null && (
                <Field label="完成时间" value={formatTimestamp(task.completedAt)} />
              )}
              {task.reminderAt !== null && !task.isCompleted && (
                <Field label="下次提醒" value={formatTimestamp(task.reminderAt)} />
              )}
            </div>

            <section className="mobile-detail-section">
              <h2>步骤</h2>
              {task.steps.length === 0 ? (
                <p className="mobile-hint">没有步骤。</p>
              ) : (
                <ul className="mobile-steps">
                  {task.steps.map((step) => (
                    <li key={step.id} className={step.isCompleted ? "done" : ""}>
                      <span className="mobile-step-mark">
                        {step.isCompleted ? <IconCheckCircle size={16} /> : <span />}
                      </span>
                      <span>{step.title}</span>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="mobile-detail-section">
              <h2>备注</h2>
              {task.note.trim() ? (
                <p className="mobile-note">{task.note}</p>
              ) : (
                <p className="mobile-hint">没有备注。</p>
              )}
            </section>

            <p className="mobile-readonly-hint">此页面为只读，修改任务请回到 Windows 电脑。</p>
          </>
        )}
      </div>
    </div>
  );
}
