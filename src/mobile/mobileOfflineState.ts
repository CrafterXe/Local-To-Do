import type { View } from "../api";

/** 每个主视图拥有独立的搜索、排序和滚动状态。 */
export function viewKey(view: View): string {
  if (view.kind === "list") return `list:${view.listId ?? ""}`;
  if (view.kind === "day") return `day:${view.date ?? ""}`;
  return view.kind;
}

/** 清单被删除后回到仍然有效的全部任务视图。 */
export function validViewAfterListsChange(view: View, listIds: string[]): View {
  return view.kind === "list" && (!view.listId || !listIds.includes(view.listId))
    ? { kind: "all" }
    : view;
}
