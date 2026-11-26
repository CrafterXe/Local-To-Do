import { useEffect, useState } from "react";

import { api, type Task, type TaskList } from "../api";
import { IconArrowLeft, IconChevronRight } from "../components/Icons";
import { errorText } from "./mobileUtils";
import { MobileTaskRow } from "./MobileTaskRow";

interface MobileListsProps {
  version: number;
  onOpenTask: (id: string) => void;
}

/** 我的清单：查看清单与其中任务，不提供新建/编辑/删除清单 */
export function MobileLists({ version, onOpenTask }: MobileListsProps) {
  const [lists, setLists] = useState<TaskList[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tasks, setTasks] = useState<Task[] | null>(null);

  useEffect(() => {
    let alive = true;
    setError(null);
    api
      .mobileLists()
      .then((list) => alive && setLists(list))
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [version]);

  useEffect(() => {
    if (!selectedId) return;
    let alive = true;
    setTasks(null);
    api
      .mobileTasks({ scope: "list", listId: selectedId, includeCompleted: true })
      .then((list) => alive && setTasks(list))
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [version, selectedId]);

  if (error) return <div className="mobile-alert error">{error}</div>;

  if (selectedId) {
    const list = lists?.find((item) => item.id === selectedId);
    const openCount = tasks?.filter((task) => !task.isCompleted).length ?? 0;
    return (
      <div className="mobile-view">
        <div className="mobile-subheader">
          <button
            className="mobile-icon-button"
            onClick={() => {
              setSelectedId(null);
              setTasks(null);
            }}
            aria-label="返回清单列表"
          >
            <IconArrowLeft size={20} />
          </button>
          <div className="mobile-subheader-text">
            <strong>{list?.name ?? "清单"}</strong>
            <span>
              {openCount} 项待办 · 共 {tasks?.length ?? 0} 项
            </span>
          </div>
        </div>

        {tasks === null ? (
          <div className="mobile-empty">正在载入清单任务…</div>
        ) : tasks.length === 0 ? (
          <div className="mobile-empty">
            <strong>这个清单还没有任务</strong>
            <span>在 Windows 上为清单添加任务后会同步到这里。</span>
          </div>
        ) : (
          <div className="mobile-list">
            {tasks.map((task) => (
              <MobileTaskRow key={task.id} task={task} onOpen={onOpenTask} />
            ))}
          </div>
        )}
      </div>
    );
  }

  if (lists === null) return <div className="mobile-empty">正在载入清单…</div>;

  if (lists.length === 0) {
    return (
      <div className="mobile-empty">
        <strong>还没有清单</strong>
        <span>在 Windows 上创建清单后会同步到这里。</span>
      </div>
    );
  }

  return (
    <div className="mobile-view">
      <div className="mobile-list">
        {lists.map((list) => (
          <button key={list.id} className="mobile-list-row" onClick={() => setSelectedId(list.id)}>
            <span className="mobile-list-dot" style={{ background: list.color || "var(--muted)" }} />
            <span className="mobile-list-main">
              <span className="mobile-list-name">{list.name}</span>
              <span className="mobile-list-count">
                {list.openCount} 项待办 · 共 {list.totalCount} 项
              </span>
            </span>
            <IconChevronRight size={18} />
          </button>
        ))}
      </div>
    </div>
  );
}
