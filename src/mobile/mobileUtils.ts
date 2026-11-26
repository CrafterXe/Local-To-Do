import type { Task } from "../api";
import { diffDays, todayString } from "../utils/date";

/** 全部任务页的状态筛选 */
export type TaskStatusFilter = "open" | "completed" | "all";
/** 全部任务页的日期筛选 */
export type TaskDateFilter = "any" | "today" | "week" | "overdue" | "undated";
/** 全部任务页的排序方式 */
export type TaskSort = "due" | "created" | "title";

export interface TaskFilterOptions {
  search?: string;
  status?: TaskStatusFilter;
  date?: TaskDateFilter;
  listId?: string | null;
}

/** 把 Tauri 命令错误转换为可读文案（去掉 "Error: " 前缀） */
export function errorText(error: unknown): string {
  const text =
    typeof error === "string"
      ? error
      : error instanceof Error
        ? error.message
        : String(error);
  return text.replace(/^Error:\s*/, "");
}

function includesText(task: Task, needle: string): boolean {
  if (!needle) return true;
  return (
    task.title.toLowerCase().includes(needle) ||
    task.note.toLowerCase().includes(needle) ||
    task.listName.toLowerCase().includes(needle)
  );
}

/** 只过滤展示结果，不修改任务数据 */
export function filterTasks(tasks: Task[], options: TaskFilterOptions = {}): Task[] {
  const status = options.status ?? "all";
  const date = options.date ?? "any";
  const listId = options.listId ?? null;
  const needle = (options.search ?? "").trim().toLowerCase();
  const today = todayString();

  return tasks.filter((task) => {
    if (listId && task.listId !== listId) return false;
    if (status === "open" && task.isCompleted) return false;
    if (status === "completed" && !task.isCompleted) return false;
    if (!includesText(task, needle)) return false;

    switch (date) {
      case "today":
        return task.dueDate === today;
      case "week": {
        if (!task.dueDate) return false;
        const diff = diffDays(today, task.dueDate);
        return diff >= 0 && diff <= 6;
      }
      case "overdue":
        return !task.isCompleted && !!task.dueDate && diffDays(today, task.dueDate) < 0;
      case "undated":
        return !task.dueDate;
      default:
        return true;
    }
  });
}

/** 排序只影响当前列表展示：未完成优先 */
export function sortTasks(tasks: Task[], sort: TaskSort = "due"): Task[] {
  const copy = [...tasks];
  copy.sort((a, b) => {
    if (a.isCompleted !== b.isCompleted) return a.isCompleted ? 1 : -1;
    if (sort === "title") return a.title.localeCompare(b.title, "zh-CN");
    if (sort === "created") return b.createdAt - a.createdAt;
    const aDue = a.dueDate ?? "9999-12-31";
    const bDue = b.dueDate ?? "9999-12-31";
    if (aDue !== bDue) return aDue < bDue ? -1 : 1;
    const aTime = a.dueTime ?? "99:99";
    const bTime = b.dueTime ?? "99:99";
    if (aTime !== bTime) return aTime < bTime ? -1 : 1;
    return b.createdAt - a.createdAt;
  });
  return copy;
}

/** 任务计数摘要（用于「我的今天」标题） */
export function summarizeTasks(tasks: Task[]): { open: number; completed: number } {
  let open = 0;
  let completed = 0;
  for (const task of tasks) {
    if (task.isCompleted) completed += 1;
    else open += 1;
  }
  return { open, completed };
}
