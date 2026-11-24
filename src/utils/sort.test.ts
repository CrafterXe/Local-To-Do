import { describe, expect, it } from "vitest";
import { sortTasks } from "./sort";

const task = (
  title: string,
  createdAt: number,
  dueDate: string | null = null,
  dueTime: string | null = null,
) => ({ title, createdAt, dueDate, dueTime });

describe("任务排序", () => {
  it("按名称使用中文自然顺序", () => {
    const chinese = [task("任务10", 3), task("任务2", 2), task("任务1", 1)];
    expect(sortTasks(chinese, "name").map((t) => t.title)).toEqual([
      "任务1",
      "任务2",
      "任务10",
    ]);
    const mixed = [task("item10", 3), task("Beta", 2), task("item2", 1), task("Alpha", 4)];
    expect(sortTasks(mixed, "name").map((t) => t.title)).toEqual([
      "Alpha",
      "Beta",
      "item2",
      "item10",
    ]);
  });

  it("按创建时间升序", () => {
    const input = [task("c", 300), task("a", 100), task("b", 200)];
    expect(sortTasks(input, "created").map((t) => t.title)).toEqual(["a", "b", "c"]);
  });

  it("按到期日期升序且无到期排在最后", () => {
    const input = [
      task("none", 1),
      task("late", 2, "2026-11-25", "09:00"),
      task("early", 3, "2026-11-24", "18:00"),
      task("early2", 4, "2026-11-24", "08:00"),
    ];
    expect(sortTasks(input, "due").map((t) => t.title)).toEqual([
      "early2",
      "early",
      "late",
      "none",
    ]);
  });

  it("相同排序值保持稳定顺序", () => {
    const input = [task("b", 1, "2026-11-24"), task("a", 1, "2026-11-24")];
    expect(sortTasks(input, "due").map((t) => t.title)).toEqual(["b", "a"]);
    expect(sortTasks(input, "created").map((t) => t.title)).toEqual(["b", "a"]);
  });

  it("不修改原数组且不排序时返回原引用", () => {
    const input = [task("b", 2), task("a", 1)];
    const sorted = sortTasks(input, "created");
    expect(sorted).not.toBe(input);
    expect(input.map((t) => t.title)).toEqual(["b", "a"]);
    expect(sortTasks(input, null)).toBe(input);
  });
});
