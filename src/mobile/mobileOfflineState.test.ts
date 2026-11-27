import { describe, expect, it } from "vitest";

import { validViewAfterListsChange, viewKey } from "./mobileOfflineState";

describe("移动视图状态", () => {
  it("为普通视图、清单和单日生成独立缓存键", () => {
    expect(viewKey({ kind: "myDay" })).toBe("myDay");
    expect(viewKey({ kind: "list", listId: "a" })).toBe("list:a");
    expect(viewKey({ kind: "day", date: "2026-11-26" })).toBe("day:2026-11-26");
  });

  it("删除当前清单后回到全部任务，其他视图保持原对象", () => {
    expect(validViewAfterListsChange({ kind: "list", listId: "deleted" }, ["active"])).toEqual({ kind: "all" });
    const view = { kind: "list", listId: "active" } as const;
    expect(validViewAfterListsChange(view, ["active"])).toBe(view);
    const myDay = { kind: "myDay" } as const;
    expect(validViewAfterListsChange(myDay, [])).toBe(myDay);
  });
});
