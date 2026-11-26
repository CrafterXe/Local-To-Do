import { useEffect, useMemo, useState } from "react";

import { api, type CalendarDay, type Task } from "../api";
import { IconChevronLeft, IconChevronRight } from "../components/Icons";
import {
  formatDayTitle,
  monthGrid,
  monthTitle,
  parseDateString,
  todayString,
} from "../utils/date";
import { errorText } from "./mobileUtils";
import { MobileTaskRow } from "./MobileTaskRow";

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];

interface MobileCalendarProps {
  version: number;
  onOpenTask: (id: string) => void;
}

/** 竖屏月历：选择日期查看当天任务，只读展示 */
export function MobileCalendar({ version, onOpenTask }: MobileCalendarProps) {
  const today = todayString();
  const [cursor, setCursor] = useState(() => {
    const now = parseDateString(today);
    return { year: now.getFullYear(), month: now.getMonth() };
  });
  const cells = useMemo(() => monthGrid(cursor.year, cursor.month), [cursor]);
  const [days, setDays] = useState<Record<string, CalendarDay>>({});
  const [selected, setSelected] = useState(today);
  const [dayTasks, setDayTasks] = useState<Task[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setError(null);
    api
      .mobileCalendar(cells[0], cells[cells.length - 1])
      .then((list) => {
        if (!alive) return;
        const map: Record<string, CalendarDay> = {};
        for (const day of list) map[day.date] = day;
        setDays(map);
      })
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [version, cells]);

  useEffect(() => {
    let alive = true;
    setDayTasks(null);
    api
      .mobileTasksForDay(selected)
      .then((list) => alive && setDayTasks(list))
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, [version, selected]);

  const shiftMonth = (delta: number) => {
    const next = new Date(cursor.year, cursor.month + delta, 1);
    const nextCursor = { year: next.getFullYear(), month: next.getMonth() };
    setCursor(nextCursor);
    // 切换月份时直接选中该月 1 号，避免停留在上个月的日期
    const month = String(nextCursor.month + 1).padStart(2, "0");
    setSelected(`${nextCursor.year}-${month}-01`);
  };

  const goToday = () => {
    const now = parseDateString(today);
    setCursor({ year: now.getFullYear(), month: now.getMonth() });
    setSelected(today);
  };

  return (
    <div className="mobile-view">
      <div className="mobile-cal-header">
        <button className="mobile-cal-nav" onClick={() => shiftMonth(-1)} aria-label="上个月">
          <IconChevronLeft size={18} />
        </button>
        <button className="mobile-cal-title" onClick={goToday}>
          {monthTitle(cursor.year, cursor.month)}
        </button>
        <button className="mobile-cal-nav" onClick={() => shiftMonth(1)} aria-label="下个月">
          <IconChevronRight size={18} />
        </button>
      </div>

      <div className="mobile-cal-grid">
        {WEEKDAYS.map((label) => (
          <span key={label} className="mobile-cal-weekday">
            {label}
          </span>
        ))}
        {cells.map((date) => {
          const day = parseDateString(date);
          const inMonth = day.getMonth() === cursor.month;
          const info = days[date];
          const classes = ["mobile-cal-cell"];
          if (!inMonth) classes.push("outside");
          if (date === today) classes.push("today");
          if (date === selected) classes.push("selected");
          return (
            <button
              key={date}
              className={classes.join(" ")}
              onClick={() => setSelected(date)}
              aria-label={`${date}${info ? `，${info.total} 个任务` : ""}`}
            >
              <span className="mobile-cal-day">{day.getDate()}</span>
              {info && info.total > 0 ? (
                <span className={`mobile-cal-count${info.hasImportant ? " important" : ""}`}>
                  {info.total}
                </span>
              ) : (
                <span className="mobile-cal-count empty" />
              )}
            </button>
          );
        })}
      </div>

      {error && <div className="mobile-alert error">{error}</div>}

      <section className="mobile-section">
        <h2 className="mobile-section-title">
          {formatDayTitle(selected)}
          {selected === today ? " · 今天" : ""}
        </h2>
        {dayTasks === null ? (
          <div className="mobile-empty">正在载入当天任务…</div>
        ) : dayTasks.length === 0 ? (
          <p className="mobile-hint">这一天没有任务。</p>
        ) : (
          <div className="mobile-list">
            {dayTasks.map((task) => (
              <MobileTaskRow key={task.id} task={task} onOpen={onOpenTask} />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
