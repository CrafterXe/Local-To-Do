/**
 * 月份滚轮切换的轻量去抖窗口。
 * 同一帧内的重复事件视为一次物理滚动输入，避免一次输入切换多月；
 * 同时不做累积阈值，保证每个有效滚轮事件都能切换一个月。
 */
const MONTH_WHEEL_COOLDOWN_MS = 12;

/**
 * 在目标元素上挂载“滚轮切换月份”监听。
 * - 向上滚动（deltaY < 0）→ 上一个月
 * - 向下滚动（deltaY > 0）→ 下一个月
 * - Ctrl + 滚轮放行，保留浏览器缩放
 * 返回清理函数。
 */
export function addMonthWheelListener(
  el: HTMLElement,
  onPrev: () => void,
  onNext: () => void,
): () => void {
  let lastSwitch = 0;
  const onWheel = (e: WheelEvent) => {
    if (e.ctrlKey || e.deltaY === 0) return;
    e.preventDefault();
    const now = performance.now();
    if (now - lastSwitch < MONTH_WHEEL_COOLDOWN_MS) return;
    lastSwitch = now;
    if (e.deltaY > 0) onNext();
    else onPrev();
  };
  el.addEventListener("wheel", onWheel, { passive: false });
  return () => el.removeEventListener("wheel", onWheel);
}
