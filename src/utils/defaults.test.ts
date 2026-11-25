import { describe, expect, it } from "vitest";
import { DEFAULT_SORT_KIND, DEFAULT_VIEW } from "./defaults";
import { sortTasks } from "./sort";

describe("默认状态", () => {
  it("启动默认页面为「全部任务」", () => {
    expect(DEFAULT_VIEW).toEqual({ kind: "all" });
  });

  it("默认排序为按创建时间，并使「查看」菜单勾选创建时间", () => {
    expect(DEFAULT_SORT_KIND).toBe("created");
    const tasks = [
      { title: "后创建", createdAt: 200, dueDate: null, dueTime: null },
      { title: "先创建", createdAt: 100, dueDate: null, dueTime: null },
    ];
    expect(sortTasks(tasks, DEFAULT_SORT_KIND).map((t) => t.title)).toEqual([
      "先创建",
      "后创建",
    ]);
  });
});
