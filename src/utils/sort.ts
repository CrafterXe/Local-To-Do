export type SortKind = "name" | "created" | "due" | null;

interface SortableTask {
  title: string;
  createdAt: number;
  dueDate: string | null;
  dueTime: string | null;
}

const collator = new Intl.Collator("zh-Hans-CN", { numeric: true, sensitivity: "base" });

/** 按指定方式排序任务，不修改原数组；相同排序值保持原有顺序 */
export function sortTasks<T extends SortableTask>(tasks: T[], kind: SortKind): T[] {
  if (!kind) return tasks;
  const arr = tasks.slice();
  if (kind === "name") {
    arr.sort((a, b) => collator.compare(a.title, b.title));
  } else if (kind === "created") {
    arr.sort((a, b) => a.createdAt - b.createdAt);
  } else {
    arr.sort((a, b) => {
      if (!a.dueDate && !b.dueDate) return 0;
      if (!a.dueDate) return 1;
      if (!b.dueDate) return -1;
      const ka = `${a.dueDate} ${a.dueTime ?? "00:00"}`;
      const kb = `${b.dueDate} ${b.dueTime ?? "00:00"}`;
      return ka < kb ? -1 : ka > kb ? 1 : 0;
    });
  }
  return arr;
}
