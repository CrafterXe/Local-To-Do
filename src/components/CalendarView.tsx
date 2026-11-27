import { useEffect, useMemo, useRef } from "react";
import type { CalendarDay } from "../api";
import { monthGrid, parseDateString, monthTitle, todayString, WEEKDAY_SHORT } from "../utils/date";
import { addMonthWheelListener } from "../utils/wheel";
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
const SWIPE_THRESHOLD = 50;

/**
 * 月历视图：以日期背景色作为任务主提示，底部小标识体现任务数量。
 * 普通任务优先使用标准蓝色，重复任务使用更淡的蓝色。
 * 支持鼠标滚轮和触摸左右滑动切换月份。
 */
export function CalendarView({ year, month, days, onPrev, onNext, onOpenDay }: Props) {
  const today = todayString();
  const cells = useMemo(() => monthGrid(year, month), [year, month]);
  const byDate = useMemo(() => new Map(days.map((d) => [d.date, d])), [days]);

  const viewRef = useRef<HTMLDivElement>(null);
  const handlersRef = useRef({ onPrev, onNext });
  handlersRef.current = { onPrev, onNext };

  // 滚轮切换月份：每个有效滚动事件切换一个月，仅做极短去重避免一次输入跳多月
  useEffect(() => {
    const el = viewRef.current;
    if (!el) return;
    return addMonthWheelListener(
      el,
      () => handlersRef.current.onPrev(),
      () => handlersRef.current.onNext(),
    );
  }, []);

  // 触摸左右滑动切换月份
  useEffect(() => {
    const el = viewRef.current;
    if (!el) return;
    let startX = 0;
    let startY = 0;
    let tracking = false;

    const onTouchStart = (e: TouchEvent) => {
      if (e.touches.length !== 1) return;
      startX = e.touches[0].clientX;
      startY = e.touches[0].clientY;
      tracking = true;
    };

    const onTouchEnd = (e: TouchEvent) => {
      if (!tracking || e.changedTouches.length !== 1) {
        tracking = false;
        return;
      }
      tracking = false;
      const endX = e.changedTouches[0].clientX;
      const endY = e.changedTouches[0].clientY;
      const dx = endX - startX;
      const dy = endY - startY;
      if (Math.abs(dx) > Math.abs(dy) && Math.abs(dx) > SWIPE_THRESHOLD) {
        if (dx > 0) handlersRef.current.onPrev();
        else handlersRef.current.onNext();
      }
    };

    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchend", onTouchEnd, { passive: true });
    return () => {
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchend", onTouchEnd);
    };
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
