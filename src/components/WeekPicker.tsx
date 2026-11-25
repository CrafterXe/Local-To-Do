import { useEffect, useRef, useState } from "react";
import { monthGrid, monthTitle, parseDateString, todayString, WEEKDAY_SHORT } from "../utils/date";
import { addMonthWheelListener } from "../utils/wheel";
import { IconChevronLeft, IconChevronRight } from "./Icons";

interface Props {
  label: string;
  /** 用户自定义的第一周基准日期；null 表示默认（当年 1 月 1 日） */
  anchorDate: string | null;
  onSelect: (date: string) => void;
}

/** 侧边栏周数 + 应用内自定义月历选择器 */
export function WeekPicker({ label, anchorDate, onSelect }: Props) {
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(() => {
    const d = parseDateString(todayString());
    return { year: d.getFullYear(), month: d.getMonth() };
  });
  const panelRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  const today = todayString();
  const selected = anchorDate ?? today;

  const shiftMonth = (delta: number) => {
    setCursor((c) => {
      const next = c.month + delta;
      return { year: c.year + Math.floor(next / 12), month: ((next % 12) + 12) % 12 };
    });
  };

  // 每次打开都从今天所在的月份开始浏览
  const toggle = () => {
    if (!open) {
      const d = parseDateString(today);
      setCursor({ year: d.getFullYear(), month: d.getMonth() });
    }
    setOpen((v) => !v);
  };

  // 点击外部或按 Esc 关闭
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  // 面板内滚轮切换月份（仅在面板内响应，避免影响任务列表滚动）
  // 与 CalendarView 共用同一套灵敏度：每个有效滚动事件切换一个月
  useEffect(() => {
    if (!open) return;
    const el = panelRef.current;
    if (!el) return;
    return addMonthWheelListener(
      el,
      () => shiftMonth(-1),
      () => shiftMonth(1),
    );
  }, [open]);

  const cells = monthGrid(cursor.year, cursor.month);

  return (
    <div className="sidebar-week-wrap" ref={rootRef}>
      <button
        type="button"
        className="sidebar-week-label"
        title="点击设置第一周基准日期"
        onClick={toggle}
      >
        {label}
      </button>

      {open && (
        <div className="week-popover" ref={panelRef}>
          <div className="week-popover-head">
            <button
              type="button"
              className="week-popover-nav"
              title="上个月"
              onClick={() => shiftMonth(-1)}
            >
              <IconChevronLeft size={16} />
            </button>
            <div className="week-popover-title">{monthTitle(cursor.year, cursor.month)}</div>
            <button
              type="button"
              className="week-popover-nav"
              title="下个月"
              onClick={() => shiftMonth(1)}
            >
              <IconChevronRight size={16} />
            </button>
          </div>

          <div className="week-popover-grid">
            {WEEKDAY_SHORT.map((w) => (
              <div className="week-popover-dow" key={w}>
                {w}
              </div>
            ))}
            {cells.map((date) => {
              const d = parseDateString(date);
              const outside = d.getFullYear() !== cursor.year || d.getMonth() !== cursor.month;
              return (
                <button
                  type="button"
                  key={date}
                  className={`week-popover-day ${outside ? "out" : ""} ${
                    date === selected ? "selected" : date === today ? "today" : ""
                  }`}
                  onClick={() => {
                    onSelect(date);
                    setOpen(false);
                  }}
                >
                  {d.getDate()}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
