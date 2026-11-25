import { useMemo } from "react";
import type { CalendarDay } from "../api";
import { monthGrid, parseDateString, monthTitle, todayString, WEEKDAY_SHORT } from "../utils/date";
import { IconChevronLeft, IconChevronRight, IconStar } from "./Icons";

interface Props {
  year: number;
  month: number;
  days: CalendarDay[];
  onPrev: () => void;
  onNext: () => void;
  onOpenDay: (date: string) => void;
}

const MAX_DOTS = 3;

/** 月历视图：只展示日期与任务标识，不展示任务名称与数量文字 */
export function CalendarView({ year, month, days, onPrev, onNext, onOpenDay }: Props) {
  const today = todayString();
  const cells = useMemo(() => monthGrid(year, month), [year, month]);
  const byDate = useMemo(() => new Map(days.map((d) => [d.date, d])), [days]);

  return (
    <div className="calendar-view">
      <div className="calendar-panel">
        <div className="calendar-head">
          <button type="button" className="calendar-nav" title="上个月" onClick={onPrev}>
            <IconChevronLeft size={18} />
          </button>
          <div className="calendar-title">{monthTitle(year, month)}</div>
          <button type="button" className="calendar-nav" title="下个月" onClick={onNext}>
            <IconChevronRight size={18} />
          </button>
        </div>

        <div className="calendar-grid">
          {WEEKDAY_SHORT.map((w) => (
            <div className="calendar-dow" key={w}>
              周{w}
            </div>
          ))}

          {cells.map((date) => {
            const info = byDate.get(date);
            const d = parseDateString(date);
            const outside = d.getFullYear() !== year || d.getMonth() !== month;
            const dayNumber = d.getDate();
            const solid = info ? Math.min(info.nonRepeat, MAX_DOTS) : 0;
            const faded = info ? Math.min(info.repeat, MAX_DOTS) : 0;
            return (
              <div
                key={date}
                className={`calendar-cell ${outside ? "out" : ""} ${
                  date === today ? "today" : ""
                } ${info ? "has-task" : ""} ${info?.hasImportant ? "important" : ""}`}
                title={info ? `${info.total}个任务` : undefined}
                onClick={() => {
                  // 没有任务的日期不触发任何跳转
                  if (info) onOpenDay(date);
                }}
              >
                {info?.hasImportant && (
                  <span className="calendar-star">
                    <IconStar size={11} filled />
                  </span>
                )}
                <span className="calendar-num">{dayNumber}</span>
                {info && (
                  <span className="calendar-dots">
                    {Array.from({ length: solid }).map((_, i) => (
                      <i className="calendar-dot" key={`s${i}`} />
                    ))}
                    {Array.from({ length: faded }).map((_, i) => (
                      <i className="calendar-dot repeat" key={`r${i}`} />
                    ))}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
