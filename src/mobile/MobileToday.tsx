import { useEffect, useState } from "react";

import { api, type Task } from "../api";
import { diffDays, todayString } from "../utils/date";
import { errorText, summarizeTasks } from "./mobileUtils";
import { MobileTaskRow } from "./MobileTaskRow";

interface MobileTodayProps {
  version: number;
  onOpenTask: (id: string) => void;
}

/** 我的今天：只展示今天需要处理的任务，不提供任何完成操作 */
export function MobileToday({ version, onOpenTask }: MobileTodayProps) {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const today = todayString();

  useEffect(() => {
    let alive = true;
    setTasks(null);
    setError(null);
    api
      .mobileTasks({ scope: "today" })
      .then((list) => alive && setTasks(list))
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [version]);

  if (error) return <div className="mobile-alert error">{error}</div>;
  if (tasks === null) return <div className="mobile-empty">正在载入今天的任务…</div>;

  const overdue: Task[] = [];
  const todayTasks: Task[] = [];
  for (const task of tasks) {
    if (!task.isCompleted && task.dueDate && diffDays(today, task.dueDate) < 0) {
      overdue.push(task);
    } else {
      todayTasks.push(task);
    }
  }
  const summary = summarizeTasks(todayTasks);

  if (tasks.length === 0) {
    return (
      <div className="mobile-empty">
        <strong>今天没有需要处理的任务</strong>
        <span>在 Windows 上添加到期日或加入「我的一天」后会显示在这里。</span>
      </div>
    );
  }

  return (
    <div className="mobile-view">
      <p className="mobile-summary">
        {summary.open} 项待办
        {summary.completed > 0 ? ` · ${summary.completed} 项已完成` : ""}
      </p>

      {overdue.length > 0 && (
        <section className="mobile-section">
          <h2 className="mobile-section-title danger">已逾期 {overdue.length}</h2>
          <div className="mobile-list">
            {overdue.map((task) => (
              <MobileTaskRow key={task.id} task={task} onOpen={onOpenTask} />
            ))}
          </div>
        </section>
      )}

      <section className="mobile-section">
        <h2 className="mobile-section-title">今天</h2>
        {todayTasks.length === 0 ? (
          <p className="mobile-hint">今天没有剩余任务。</p>
        ) : (
          <div className="mobile-list">
            {todayTasks.map((task) => (
              <MobileTaskRow key={task.id} task={task} onOpen={onOpenTask} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
