import { useEffect, useMemo, useState } from "react";

import { api, type Task, type TaskList } from "../api";
import { IconSearch } from "../components/Icons";
import {
  errorText,
  filterTasks,
  sortTasks,
  type TaskDateFilter,
  type TaskSort,
  type TaskStatusFilter,
} from "./mobileUtils";
import { MobileTaskRow } from "./MobileTaskRow";

interface MobileAllTasksProps {
  version: number;
  onOpenTask: (id: string) => void;
}

const STATUS_OPTIONS: { value: TaskStatusFilter; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "open", label: "未完成" },
  { value: "completed", label: "已完成" },
];

const DATE_OPTIONS: { value: TaskDateFilter; label: string }[] = [
  { value: "any", label: "全部日期" },
  { value: "today", label: "今天" },
  { value: "week", label: "未来 7 天" },
  { value: "overdue", label: "已逾期" },
  { value: "undated", label: "无日期" },
];

const SORT_OPTIONS: { value: TaskSort; label: string }[] = [
  { value: "due", label: "按到期时间" },
  { value: "created", label: "按创建时间" },
  { value: "title", label: "按标题" },
];

/** 全部任务：搜索、筛选、排序只影响本地展示，不修改任何任务数据 */
export function MobileAllTasks({ version, onOpenTask }: MobileAllTasksProps) {
  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [lists, setLists] = useState<TaskList[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState<TaskStatusFilter>("all");
  const [date, setDate] = useState<TaskDateFilter>("any");
  const [listId, setListId] = useState("");
  const [sort, setSort] = useState<TaskSort>("due");

  useEffect(() => {
    let alive = true;
    setTasks(null);
    setError(null);
    Promise.all([
      api.mobileTasks({ scope: "all", includeCompleted: true }),
      api.mobileLists(),
    ])
      .then(([taskList, listList]) => {
        if (!alive) return;
        setTasks(taskList);
        setLists(listList);
      })
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [version]);

  const shown = useMemo(
    () =>
      sortTasks(
        filterTasks(tasks ?? [], { search, status, date, listId: listId || null }),
        sort,
      ),
    [tasks, search, status, date, listId, sort],
  );

  return (
    <div className="mobile-view">
      <div className="mobile-search">
        <IconSearch size={16} />
        <input
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          placeholder="搜索任务、备注或清单"
          aria-label="搜索任务"
        />
        {search && (
          <button className="mobile-search-clear" onClick={() => setSearch("")} aria-label="清除">
            ×
          </button>
        )}
      </div>

      <div className="mobile-chips">
        {STATUS_OPTIONS.map((option) => (
          <button
            key={option.value}
            className={`mobile-chip${status === option.value ? " active" : ""}`}
            onClick={() => setStatus(option.value)}
          >
            {option.label}
          </button>
        ))}
      </div>

      <div className="mobile-chips">
        {DATE_OPTIONS.map((option) => (
          <button
            key={option.value}
            className={`mobile-chip${date === option.value ? " active" : ""}`}
            onClick={() => setDate(option.value)}
          >
            {option.label}
          </button>
        ))}
      </div>

      <div className="mobile-select-row">
        <select
          className="mobile-select"
          value={listId}
          onChange={(event) => setListId(event.target.value)}
          aria-label="按清单筛选"
        >
          <option value="">所有清单</option>
          {lists.map((list) => (
            <option key={list.id} value={list.id}>
              {list.name}
            </option>
          ))}
        </select>
        <select
          className="mobile-select"
          value={sort}
          onChange={(event) => setSort(event.target.value as TaskSort)}
          aria-label="排序方式"
        >
          {SORT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      {error && <div className="mobile-alert error">{error}</div>}
      {tasks === null ? (
        <div className="mobile-empty">正在载入任务…</div>
      ) : shown.length === 0 ? (
        <div className="mobile-empty">
          <strong>没有符合条件的任务</strong>
          <span>试试调整搜索或筛选条件。</span>
        </div>
      ) : (
        <>
          <p className="mobile-summary">共 {shown.length} 项</p>
          <div className="mobile-list">
            {shown.map((task) => (
              <MobileTaskRow key={task.id} task={task} onOpen={onOpenTask} />
            ))}
          </div>
        </>
      )}
    </div>
  );
}
