import type { View } from "../api";
import type { SortKind } from "./sort";

/** 默认排序：按创建时间 */
export const DEFAULT_SORT_KIND: Exclude<SortKind, null> = "created";

/** 应用启动后的默认页面：全部任务 */
export const DEFAULT_VIEW: View = { kind: "all" };
