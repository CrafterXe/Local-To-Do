import { useRef, useState } from "react";
import type { SidebarCounts, TaskList, View } from "../api";
import { WeekPicker } from "./WeekPicker";
import {
  IconAll,
  IconCalendar,
  IconCheckCircle,
  IconFolder,
  IconHistory,
  IconList,
  IconNote,
  IconPlus,
  IconSearch,
  IconSettings,
  IconStar,
  IconSun,
  IconTrash,
  IconMore,
} from "./Icons";

interface Props {
  lists: TaskList[];
  counts: SidebarCounts | null;
  view: View;
  search: string;
  onSearch: (s: string) => void;
  onSelect: (v: View) => void;
  onCreateList: () => void;
  onManageList: (l: TaskList) => void;
  onReorderLists: (ids: string[]) => void;
  weekLabel: string;
  weekBaseDate: string | null;
  onSetWeekBase: (date: string) => void;
}

export function Sidebar({
  lists,
  counts,
  view,
  search,
  onSearch,
  onSelect,
  onCreateList,
  onManageList,
  onReorderLists,
  weekLabel,
  weekBaseDate,
  onSetWeekBase,
}: Props) {
  const is = (kind: string, listId?: string) =>
    view.kind === kind && (listId === undefined || view.listId === listId);

  const [dragId, setDragId] = useState<string | null>(null);
  const [dropMark, setDropMark] = useState<{ id: string; after: boolean } | null>(null);
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  const dropRef = useRef<{ id: string; after: boolean } | null>(null);
  const didDragRef = useRef(false);

  const startDrag = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    const startX = e.clientX;
    const startY = e.clientY;
    let active = false;

    const onMove = (ev: PointerEvent) => {
      if (!active) {
        if (Math.abs(ev.clientY - startY) < 4 && Math.abs(ev.clientX - startX) < 4) return;
        active = true;
        setDragId(id);
      }
      let hit: { id: string; after: boolean } | null = null;
      rowRefs.current.forEach((el, rid) => {
        if (rid === id) return;
        const r = el.getBoundingClientRect();
        if (ev.clientY >= r.top && ev.clientY <= r.bottom) {
          hit = { id: rid, after: ev.clientY > r.top + r.height / 2 };
        }
      });
      dropRef.current = hit;
      setDropMark(hit);
    };

    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      if (active) {
        didDragRef.current = true;
        const mark = dropRef.current;
        if (mark) {
          const ids = lists.map((l) => l.id);
          const from = ids.indexOf(id);
          if (from >= 0) {
            ids.splice(from, 1);
            let to = ids.indexOf(mark.id);
            if (to >= 0) {
              if (mark.after) to += 1;
              ids.splice(to, 0, id);
              if (ids.some((v, i) => v !== lists[i].id)) onReorderLists(ids);
            }
          }
        }
      }
      dropRef.current = null;
      setDragId(null);
      setDropMark(null);
    };

    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const moveByKeyboard = (id: string, dir: number) => {
    const ids = lists.map((l) => l.id);
    const i = ids.indexOf(id);
    const j = i + dir;
    if (i < 0 || j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j], ids[i]];
    onReorderLists(ids);
  };

  return (
    <aside className="sidebar">
      <div className="sidebar-header">
        <div className="avatar">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
            <path d="m5 12.5 4.2 4.2L19 6.8" />
          </svg>
        </div>
        <div className="brand">
          <div className="brand-name">Local To Do</div>
        </div>
        <WeekPicker label={weekLabel} anchorDate={weekBaseDate} onSelect={onSetWeekBase} />
      </div>

      <div className="search-box">
        <IconSearch size={15} />
        <input
          value={search}
          placeholder="搜索任务与笔记"
          onChange={(e) => onSearch(e.target.value)}
        />
      </div>

      <div className="nav-section">
        <button className={`nav-item ${is("all") ? "active" : ""}`} onClick={() => onSelect({ kind: "all" })}>
          <span className="nav-icon"><IconAll size={17} /></span>
          <span className="nav-text">全部任务</span>
          {counts && counts.allOpen > 0 && <span className="nav-count">{counts.allOpen}</span>}
        </button>
        <button className={`nav-item ${is("myDay") ? "active" : ""}`} onClick={() => onSelect({ kind: "myDay" })}>
          <span className="nav-icon"><IconSun size={17} /></span>
          <span className="nav-text">我的一天</span>
          {counts && counts.myDay > 0 && <span className="nav-count">{counts.myDay}</span>}
        </button>
        <button className={`nav-item ${is("important") ? "active" : ""}`} onClick={() => onSelect({ kind: "important" })}>
          <span className="nav-icon"><IconStar size={17} /></span>
          <span className="nav-text">重要</span>
          {counts && counts.important > 0 && <span className="nav-count">{counts.important}</span>}
        </button>
        <button className={`nav-item ${is("completed") ? "active" : ""}`} onClick={() => onSelect({ kind: "completed" })}>
          <span className="nav-icon"><IconCheckCircle size={17} /></span>
          <span className="nav-text">已完成</span>
          {counts && counts.completed > 0 && <span className="nav-count">{counts.completed}</span>}
        </button>
        <button className={`nav-item ${is("calendar") ? "active" : ""}`} onClick={() => onSelect({ kind: "calendar" })}>
          <span className="nav-icon"><IconCalendar size={17} /></span>
          <span className="nav-text">日历</span>
        </button>
        <button className={`nav-item ${is("planned") ? "active" : ""}`} onClick={() => onSelect({ kind: "planned" })}>
          <span className="nav-icon"><IconCalendar size={17} /></span>
          <span className="nav-text">计划内</span>
          {counts && counts.planned > 0 && <span className="nav-count">{counts.planned}</span>}
        </button>

        <div className="nav-group-title">
          <span>我的清单</span>
          <button title="新建清单" onClick={onCreateList}>
            <IconPlus size={15} />
          </button>
        </div>

        {lists.map((l) => (
          <div
            key={l.id}
            ref={(el) => {
              if (el) rowRefs.current.set(l.id, el);
              else rowRefs.current.delete(l.id);
            }}
            className={`list-row ${dragId === l.id ? "dragging" : ""} ${
              dropMark?.id === l.id ? (dropMark.after ? "drag-after" : "drag-before") : ""
            }`}
          >
            <div
              className={`nav-item ${is("list", l.id) ? "active" : ""}`}
              role="button"
              tabIndex={0}
              title="拖动调整顺序（Alt + ↑/↓）"
              onPointerDown={(e) => startDrag(e, l.id)}
              onClick={() => {
                if (didDragRef.current) {
                  didDragRef.current = false;
                  return;
                }
                onSelect({ kind: "list", listId: l.id });
              }}
              onKeyDown={(e) => {
                if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
                  e.preventDefault();
                  moveByKeyboard(l.id, e.key === "ArrowUp" ? -1 : 1);
                  return;
                }
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect({ kind: "list", listId: l.id });
                }
              }}
            >
              <span className="nav-icon">
                <span className="dot" style={{ background: l.color }} />
              </span>
              <span className="nav-text">{l.name}</span>
              <span className="nav-count" style={{ marginRight: 22 }}>{l.openCount}</span>
            </div>
            <button
              className="icon-btn"
              title="管理清单"
              style={{ position: "absolute", right: 4, top: 4, width: 26, height: 26, opacity: 0.75 }}
              onClick={(e) => {
                e.stopPropagation();
                onManageList(l);
              }}
            >
              <IconMore size={15} />
            </button>
          </div>
        ))}

        <div className="nav-group-title">
          <span>其他</span>
        </div>
        <button className={`nav-item ${is("notes") ? "active" : ""}`} onClick={() => onSelect({ kind: "notes" })}>
          <span className="nav-icon"><IconNote size={17} /></span>
          <span className="nav-text">备忘录</span>
          {counts && counts.notes > 0 && <span className="nav-count">{counts.notes}</span>}
        </button>
        <button className={`nav-item ${is("history") ? "active" : ""}`} onClick={() => onSelect({ kind: "history" })}>
          <span className="nav-icon"><IconHistory size={17} /></span>
          <span className="nav-text">历史记录</span>
        </button>
        <button className={`nav-item ${is("deleted") ? "active" : ""}`} onClick={() => onSelect({ kind: "deleted" })}>
          <span className="nav-icon"><IconTrash size={17} /></span>
          <span className="nav-text">最近删除</span>
          {counts && counts.deleted > 0 && <span className="nav-count">{counts.deleted}</span>}
        </button>
      </div>

      <div className="sidebar-footer">
        <button className={`nav-item ${is("settings") ? "active" : ""}`} onClick={() => onSelect({ kind: "settings" })}>
          <span className="nav-icon"><IconSettings size={17} /></span>
          <span className="nav-text">设置</span>
        </button>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px 4px", color: "var(--muted-2)", fontSize: 11 }}>
          <IconFolder size={13} />
          <span>数据保存在本机 SQLite</span>
        </div>
      </div>
    </aside>
  );
}

export function viewTitle(view: View, lists: TaskList[]): string {
  switch (view.kind) {
    case "myDay":
      return "我的一天";
    case "important":
      return "重要";
    case "planned":
      return "计划内";
    case "all":
      return "全部任务";
    case "completed":
      return "已完成";
    case "deleted":
      return "最近删除";
    case "notes":
      return "备忘录";
    case "settings":
      return "设置";
    case "history":
      return "历史记录";
    case "calendar":
      return "日历";
    case "list":
      return lists.find((l) => l.id === view.listId)?.name ?? "清单";
    default:
      return "任务";
  }
}

export function viewIcon(view: View, lists: TaskList[]) {
  switch (view.kind) {
    case "myDay":
      return <IconSun size={20} />;
    case "important":
      return <IconStar size={20} />;
    case "planned":
      return <IconCalendar size={20} />;
    case "all":
      return <IconAll size={20} />;
    case "completed":
      return <IconCheckCircle size={20} />;
    case "deleted":
      return <IconTrash size={20} />;
    case "notes":
      return <IconNote size={20} />;
    case "settings":
      return <IconSettings size={20} />;
    case "history":
      return <IconHistory size={20} />;
    case "calendar":
    case "day":
      return <IconCalendar size={20} />;
    case "list": {
      const color = lists.find((l) => l.id === view.listId)?.color;
      return <span className="dot" style={{ background: color ?? "var(--accent)" }} />;
    }
    default:
      return <IconList size={20} />;
  }
}
