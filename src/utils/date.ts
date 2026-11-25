/** 日期与提醒相关的纯函数（可在浏览器环境单元测试） */

export const WEEKDAY_LABELS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
export const WEEKDAY_SHORT = ["一", "二", "三", "四", "五", "六", "日"];

/** 当前用于显示的时区（null 表示跟随系统时区） */
let displayTimezone: string | null = null;

export function setDisplayTimezone(tz: string | null): void {
  displayTimezone = tz && tz.trim() ? tz : null;
}

function zonedParts(ms: number): { date: string; time: string } | null {
  if (!displayTimezone) return null;
  try {
    const parts = new Intl.DateTimeFormat("en-CA", {
      timeZone: displayTimezone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).formatToParts(new Date(ms));
    const get = (type: string) => parts.find((p) => p.type === type)?.value ?? "";
    return {
      date: `${get("year")}-${get("month")}-${get("day")}`,
      time: `${get("hour")}:${get("minute")}`,
    };
  } catch {
    return null;
  }
}

export function toDateString(d: Date): string {
  const y = d.getFullYear();
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function parseDateString(s: string): Date {
  const [y, m, d] = s.split("-").map(Number);
  return new Date(y, (m ?? 1) - 1, d ?? 1);
}

export function todayString(): string {
  return zonedParts(Date.now())?.date ?? toDateString(new Date());
}

export function addDays(s: string, days: number): string {
  const d = parseDateString(s);
  d.setDate(d.getDate() + days);
  return toDateString(d);
}

/** 自然日的 UTC 序列值：避免夏令时导致的 23/25 小时偏差 */
function daySerial(s: string): number {
  const d = parseDateString(s);
  return Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
}

/** 两个日期相差的天数（b - a） */
export function diffDays(a: string, b: string): number {
  return Math.round((daySerial(b) - daySerial(a)) / 86_400_000);
}

export function weekdayIndex(s: string): number {
  // 0=周一 .. 6=周日
  return (parseDateString(s).getDay() + 6) % 7;
}

/** 「今天 / 明天 / 昨天 / 12月1日 周二」 */
export function formatDueLabel(date: string | null, time: string | null): string {
  if (!date) return "";
  const today = todayString();
  const diff = diffDays(today, date);
  let day: string;
  if (diff === 0) day = "今天";
  else if (diff === 1) day = "明天";
  else if (diff === 2) day = "后天";
  else if (diff === -1) day = "昨天";
  else if (diff > 2 && diff < 7) day = `周${WEEKDAY_SHORT[weekdayIndex(date)]}`;
  else {
    const d = parseDateString(date);
    day = `${d.getMonth() + 1}月${d.getDate()}日`;
  }
  if (time) return `${day} ${time}`;
  return day;
}

/** 任务列表统一简略日期：明天 / 后天 / M月D日，始终带时间 */
export function formatTaskListDue(date: string | null, time: string | null): string {
  if (!date) return "";
  const diff = diffDays(todayString(), date);
  let day: string;
  if (diff === 1) day = "明天";
  else if (diff === 2) day = "后天";
  else {
    const d = parseDateString(date);
    day = `${d.getMonth() + 1}月${d.getDate()}日`;
  }
  return time ? `${day} ${time}` : day;
}

/** 「2026年12月09日 09:00」——重复任务详情中的下次日期 */
export function formatFullDateTime(date: string | null, time: string | null): string {
  if (!date) return "";
  const d = parseDateString(date);
  const pad = (n: number) => String(n).padStart(2, "0");
  const base = `${d.getFullYear()}年${pad(d.getMonth() + 1)}月${pad(d.getDate())}日`;
  return time ? `${base} ${time}` : base;
}

export function formatDueFull(date: string | null, time: string | null): string {
  if (!date) return "";
  const d = parseDateString(date);
  const base = `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 周${
    WEEKDAY_SHORT[weekdayIndex(date)]
  }`;
  return time ? `${base} ${time}` : base;
}

export function formatTimestamp(ms: number | null): string {
  if (ms === null) return "";
  const zoned = zonedParts(ms);
  if (zoned) return `${zoned.date} ${zoned.time}`;
  const d = new Date(ms);
  const pad = (n: number) => `${n}`.padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(
    d.getHours(),
  )}:${pad(d.getMinutes())}`;
}

export function formatRelative(ms: number | null): string {
  if (!ms) return "从未";
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)} 天前`;
  return formatTimestamp(ms).slice(0, 10);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

/** 生成月份网格（周一开头），返回 6*7 个日期字符串 */
export function monthGrid(year: number, month: number): string[] {
  const first = new Date(year, month, 1);
  const offset = (first.getDay() + 6) % 7; // 周一=0
  const start = new Date(year, month, 1 - offset);
  const cells: string[] = [];
  for (let i = 0; i < 42; i++) {
    const d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i);
    cells.push(toDateString(d));
  }
  return cells;
}

/** 根据 offset 生成提醒描述（精确到天/小时/分钟） */
export function reminderLabel(offset: number | null): string {
  if (offset === null) return "不提醒";
  if (offset === 0) return "准时提醒";
  const { days, hours, minutes } = splitReminderOffset(offset);
  const parts: string[] = [];
  if (days) parts.push(`${days} 天`);
  if (hours) parts.push(`${hours} 小时`);
  if (minutes) parts.push(`${minutes} 分钟`);
  return `提前 ${parts.join(" ")}`;
}

/** 把分钟数拆成 天 / 小时 / 分钟 */
export function splitReminderOffset(offset: number): {
  days: number;
  hours: number;
  minutes: number;
} {
  const total = Math.max(0, Math.floor(offset));
  return {
    days: Math.floor(total / 1440),
    hours: Math.floor((total % 1440) / 60),
    minutes: total % 60,
  };
}

/** 把 天 / 小时 / 分钟 合并成分钟数 */
export function combineReminderOffset(days: number, hours: number, minutes: number): number {
  return Math.max(0, days) * 1440 + Math.max(0, hours) * 60 + Math.max(0, minutes);
}

export function recurrenceLabel(rec: {
  kind: string;
  interval: number;
  weekdays: string;
  monthDay: number | null;
}): string {
  const names = WEEKDAY_LABELS;
  switch (rec.kind) {
    case "daily":
      return rec.interval <= 1 ? "每天" : `每 ${rec.interval} 天`;
    case "weekdays":
      return "每个工作日";
    case "weekly": {
      const days = rec.weekdays
        .split(",")
        .filter(Boolean)
        .map((n) => names[Number(n) - 1])
        .filter(Boolean);
      const base = rec.interval <= 1 ? "每周" : `每 ${rec.interval} 周`;
      return days.length ? `${base} ${days.join("、")}` : base;
    }
    case "monthly": {
      const base = rec.interval <= 1 ? "每月" : `每 ${rec.interval} 个月`;
      return rec.monthDay ? `${base} ${rec.monthDay} 日` : base;
    }
    case "yearly":
      return "每年";
    default:
      return "自定义重复";
  }
}

export function defaultReminderOffset(): number {
  return 0;
}

/** 计算自然周（周一为一周开始）的周一日期 Date */
function weekMonday(dateStr: string): Date {
  const d = parseDateString(dateStr);
  // JS getDay() 0=周日，6=周六；转周一=0 .. 周日=6
  const dow = (d.getDay() + 6) % 7;
  d.setDate(d.getDate() - dow);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

/**
 * 周数：以 baseDate 所在自然周为第 1 周，计算 currentDate 所在自然周是第几周。
 * 输入日期格式 YYYY-MM-DD，返回整数，可正可负（current 在 base 之前时为 0 或负数）。
 */
export function weekNumberByBase(currentDate: string, baseDate: string): number {
  const cur = weekMonday(currentDate);
  const base = weekMonday(baseDate);
  const curSerial = Date.UTC(cur.getFullYear(), cur.getMonth(), cur.getDate());
  const baseSerial = Date.UTC(base.getFullYear(), base.getMonth(), base.getDate());
  const weeksDiff = Math.round((curSerial - baseSerial) / (7 * 86_400_000));
  return weeksDiff + 1;
}

/** 「2026年12月」 */
export function monthTitle(year: number, month: number): string {
  return `${year}年${month + 1}月`;
}

/** 「2026年12月8日」——单日任务视图标题 */
export function formatDayTitle(date: string): string {
  const d = parseDateString(date);
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
}

/** 当年 1 月 1 日（用于默认周数基准） */
export function yearFirstDay(): string {
  const d = parseDateString(todayString());
  return `${d.getFullYear()}-01-01`;
}

/** 取得当前周数：优先使用 settings 的 weekBaseDate；None 时当年 1 月 1 日 */
export function currentWeekNumber(
  baseDate: string | null | undefined,
): number {
  const cur = todayString();
  const base = baseDate || yearFirstDay();
  return weekNumberByBase(cur, base);
}
