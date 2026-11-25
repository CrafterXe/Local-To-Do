import { useEffect, useMemo, useRef } from "react";
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

const MAX_MARKERS = 3;

/**
 * 月历视图：以日期背景色作为任务主提示，底部小标识体现任务数量。
 * 普通任务优先使用标准蓝色，重复任务使用更淡的蓝色。
 */
export function CalendarView({ year, month, days, onPrev, onNext, onOpenDay }: Props) {
  const today = todayString();
  const cells = useMemo(() => monthGrid(year, month), [year, month]);
  const byDate = useMemo(() => new Map(days.map((d) => [d.date, d])), [days]);

  const viewRef = useRef<HTMLDivElement>(null);
  const handlersRef = useRef({ onPrev, onNext });
  handlersRef.current = { onPrev, onNext };
  // 滚轮手势状态：累积阈值 + 冷却时间，避免一次滚动跳过多个月
  const wheelRef = useRef({ acc: 0, last: 0, lock: 0 });

  useEffect(() => {
    const el = viewRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      // 允许 Ctrl + 滚轮缩放，不拦截
      if (e.ctrlKey) return;
      e.preventDefault();
      const now = performance.now();
      const state = wheelRef.current;
      // 新的滚动手势重新累积
      if (now - state.last > 320) state.acc = 0;
      state.last = now;
      // 冷却期间忽略后续事件，防止一次普通滚动连续跳月
      if (now < state.lock) return;
      state.acc += e.deltaY;
      if (Math.abs(state.acc) < 36) return;
      const direction = state.acc > 0 ? 1 : -1;
      state.acc = 0;
      state.lock = now + 180;
      if (direction > 0) handlersRef.current.onNext();
      else handlersRef.current.onPrev();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  return (
    <div className="month-view" ref={viewRef}>
      <div className="month-panel">
        <div className="month-head">
          <button type="button" className="month-nav" title="上个月" onClick={onPrev}>
            <IconChevronLeft size={18} />
          </button>
          <div className="month-title">{monthTitle(year, month)}</div>
          <button type="button" className="month-nav" title="下个月" onClick={onNext}>
            <IconChevronRight size={18} />
          </button>
        </div>

        <div className="month-grid">
          {WEEKDAY_SHORT.map((w) => (
            <div className="month-dow" key={w}>
              周{w}
            </div>
          ))}

          {cells.map((date) => {
            const info = byDate.get(date);
            const d = parseDateString(date);
            const outside = d.getFullYear() !== year || d.getMonth() !== month;
            const hasNormal = !!info && info.nonRepeat > 0;
            const repeatOnly = !!info && !hasNormal && info.repeat > 0;
            // 普通任务优先；只有重复任务时才展示重复标识
            const markerCount = info
              ? Math.min(hasNormal ? info.nonRepeat : info.repeat, MAX_MARKERS)
              : 0;
            return (
              <div
                key={date}
                className={`month-cell ${outside ? "out" : ""} ${
                  date === today ? "today" : ""
                } ${hasNormal ? "has-normal" : ""} ${repeatOnly ? "has-repeat" : ""} ${
                  info?.hasImportant ? "important" : ""
                }`}
                title={info ? `${info.total}个任务` : undefined}
                onClick={() => {
                  // 没有任务的日期不触发任何跳转
                  if (info) onOpenDay(date);
                }}
              >
                {info?.hasImportant && (
                  <span className="month-star">
                    <IconStar size={12} filled />
                  </span>
                )}
                <span className="month-num">{d.getDate()}</span>
                {markerCount > 0 && (
                  <span className="month-markers">
                    {Array.from({ length: markerCount }).map((_, i) => (
                      <i className={`month-marker ${repeatOnly ? "repeat" : ""}`} key={i} />
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
