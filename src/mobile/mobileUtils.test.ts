import { describe, expect, it } from "vitest";

import type { Task } from "../api";
import { addDays, todayString } from "../utils/date";
import { errorText, filterTasks, sortTasks, summarizeTasks } from "./mobileUtils";

function task({ id, title, ...overrides }: { id: string; title: string } & Partial<Task>): Task {
  return {
    id,
    listId: "l1",
    title,
    note: "",
    isImportant: false,
    isCompleted: false,
    completedAt: null,
    dueDate: null,
    dueTime: null,
    reminderOffset: null,
    reminderEnabled: false,
    reminderAt: null,
    recurrenceId: null,
    seriesId: null,
    parentTaskId: null,
    recurrenceCount: 0,
    myDay: null,
    sortOrder: 0,
    createdAt: 1,
    updatedAt: 1,
    deletedAt: null,
    steps: [],
    recurrence: null,
    listName: "工作",
    listColor: "#5b8cff",
    ...overrides,
  };
}

const today = todayString();

describe("filterTasks", () => {
  const tasks = [
    task({ id: "a", title: "写周报", dueDate: today }),
    task({ id: "b", title: "过期任务", dueDate: addDays(today, -2) }),
    task({ id: "c", title: "已完成任务", dueDate: today, isCompleted: true }),
    task({ id: "d", title: "无日期任务" }),
    task({ id: "e", title: "下周任务", dueDate: addDays(today, 10) }),
  ];

  it("按状态筛选", () => {
    expect(filterTasks(tasks, { status: "open" }).map((t) => t.id)).toEqual([
      "a",
      "b",
      "d",
      "e",
    ]);
    expect(filterTasks(tasks, { status: "completed" }).map((t) => t.id)).toEqual(["c"]);
  });

  it("按日期筛选", () => {
    expect(filterTasks(tasks, { date: "today" }).map((t) => t.id)).toEqual(["a", "c"]);
    expect(filterTasks(tasks, { date: "overdue" }).map((t) => t.id)).toEqual(["b"]);
    expect(filterTasks(tasks, { date: "undated" }).map((t) => t.id)).toEqual(["d"]);
    expect(filterTasks(tasks, { date: "week" }).map((t) => t.id)).toEqual(["a", "c"]);
  });

  it("按关键词和清单筛选", () => {
    expect(filterTasks(tasks, { search: "周报" }).map((t) => t.id)).toEqual(["a"]);
    expect(filterTasks(tasks, { search: "工作" })).toHaveLength(5);
    expect(filterTasks(tasks, { listId: "l2" })).toHaveLength(0);
  });
});

describe("sortTasks", () => {
  it("未完成优先，并按到期时间排序", () => {
    const tasks = [
      task({ id: "a", title: "晚", dueDate: addDays(today, 3) }),
      task({ id: "b", title: "已完成", isCompleted: true, dueDate: today }),
      task({ id: "c", title: "早", dueDate: today }),
    ];
    expect(sortTasks(tasks, "due").map((t) => t.id)).toEqual(["c", "a", "b"]);
  });

  it("按创建时间倒序", () => {
    const tasks = [
      task({ id: "a", title: "旧", createdAt: 1 }),
      task({ id: "b", title: "新", createdAt: 9 }),
    ];
    expect(sortTasks(tasks, "created").map((t) => t.id)).toEqual(["b", "a"]);
  });

  it("按标题排序", () => {
    const tasks = [
      task({ id: "a", title: "B 任务" }),
      task({ id: "b", title: "A 任务" }),
    ];
    expect(sortTasks(tasks, "title").map((t) => t.id)).toEqual(["b", "a"]);
  });

  it("不修改原数组", () => {
    const tasks = [task({ id: "a", title: "一" }), task({ id: "b", title: "二" })];
    sortTasks(tasks, "title");
    expect(tasks.map((t) => t.id)).toEqual(["a", "b"]);
  });
});

describe("summarizeTasks", () => {
  it("统计未完成与已完成数量", () => {
    const tasks = [
      task({ id: "a", title: "一" }),
      task({ id: "b", title: "二", isCompleted: true }),
      task({ id: "c", title: "三" }),
    ];
    expect(summarizeTasks(tasks)).toEqual({ open: 2, completed: 1 });
  });
});

describe("errorText", () => {
  it("去掉 Tauri 的 Error 前缀", () => {
    expect(errorText("Error: 离线")).toBe("离线");
    expect(errorText(new Error("同步失败"))).toBe("同步失败");
  });
});
