import type { HistoryEntry } from "../api";
import { formatTimestamp } from "../utils/date";
import { EmptyState } from "./EmptyState";

const ACTION_LABELS: Record<string, string> = {
  created: "创建",
  updated: "编辑",
  completed: "完成",
  uncompleted: "恢复未完成",
  deleted: "删除",
  deleted_series: "删除系列",
  restored: "恢复",
  important: "标记重要",
  unimportant: "取消重要",
  my_day: "加入我的一天",
  my_day_removed: "移出我的一天",
  moved: "移动清单",
  skipped: "跳过本次",
  recurrence_created: "生成下一周期",
};

export function HistoryView({ entries }: { entries: HistoryEntry[] }) {
  if (entries.length === 0) {
    return <EmptyState text="还没有历史记录。完成任务、修改任务后会记录在这里。" />;
  }
  return (
    <div className="task-scroll">
      {entries.map((e) => (
        <div key={e.id} className="history-row">
          <span className="h-action">{ACTION_LABELS[e.action] ?? e.action}</span>
          <span className="h-title">
            {e.taskTitle}
            {e.detail ? <span style={{ color: "var(--muted-2)" }}> · {e.detail}</span> : null}
          </span>
          <span className="h-time">{formatTimestamp(e.at)}</span>
        </div>
      ))}
    </div>
  );
}
