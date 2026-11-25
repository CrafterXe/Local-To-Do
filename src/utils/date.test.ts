import { afterEach, describe, expect, it, vi } from "vitest";
import {
  addDays,
  combineReminderOffset,
  currentWeekNumber,
  defaultReminderOffset,
  diffDays,
  formatDueLabel,
  formatDueFull,
  formatFullDateTime,
  formatDayTitle,
  formatTaskListDue,
  formatTimestamp,
  monthGrid,
  monthTitle,
  parseDateString,
  recurrenceLabel,
  reminderLabel,
  setDisplayTimezone,
  splitReminderOffset,
  toDateString,
  todayString,
  weekdayIndex,
  weekNumberByBase,
} from "./date";

describe("日期基础函数", () => {
  it("日期字符串与 Date 互相转换", () => {
    const d = parseDateString("2026-03-05");
    expect(d.getFullYear()).toBe(2026);
    expect(d.getMonth()).toBe(2);
    expect(d.getDate()).toBe(5);
    expect(toDateString(d)).toBe("2026-03-05");
  });

  it("addDays 跨月与闰年", () => {
    expect(addDays("2026-02-28", 1)).toBe("2026-03-01");
    expect(addDays("2024-02-28", 1)).toBe("2024-02-29");
    expect(addDays("2024-03-01", -1)).toBe("2024-02-29");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
  });

  it("diffDays 返回两个日期相差天数", () => {
    expect(diffDays("2026-01-01", "2026-01-31")).toBe(30);
    expect(diffDays("2026-01-31", "2026-01-01")).toBe(-30);
    expect(diffDays("2026-03-01", "2026-03-01")).toBe(0);
  });

  it("weekdayIndex 以周一为 0", () => {
    expect(weekdayIndex("2026-11-23")).toBe(0); // 周一
    expect(weekdayIndex("2026-11-27")).toBe(4); // 周五
    expect(weekdayIndex("2026-11-29")).toBe(6); // 周日
  });

  it("formatDueFull 输出完整日期", () => {
    expect(formatDueFull("2026-11-23", "20:30")).toBe("2026年11月23日 周一 20:30");
    expect(formatDueFull("2026-11-23", null)).toBe("2026年11月23日 周一");
    expect(formatDueFull(null, null)).toBe("");
  });

  it("formatDueLabel 处理今天/明天/昨天", () => {
    const today = toDateString(new Date());
    expect(formatDueLabel(today, null)).toBe("今天");
    expect(formatDueLabel(addDays(today, 1), "09:00")).toBe("明天 09:00");
    expect(formatDueLabel(addDays(today, -1), null)).toBe("昨天");
    expect(formatDueLabel(null, "09:00")).toBe("");
  });

  it("重复任务详情时间格式为「YYYY年MM月DD日 HH:mm」", () => {
    expect(formatFullDateTime("2026-12-09", "09:00")).toBe("2026年12月09日 09:00");
    expect(formatFullDateTime("2026-01-05", "18:30")).toBe("2026年01月05日 18:30");
    expect(formatFullDateTime(null, null)).toBe("");
  });
});

describe("任务列表统一日期格式", () => {
  afterEach(() => {
    vi.useRealTimers();
    setDisplayTimezone(null);
  });

  it("以 2026-11-25 为基准：明天/后天/其他日期使用同一格式", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-25T02:00:00Z")); // 上海 11-25 10:00
    setDisplayTimezone("Asia/Shanghai");
    expect(formatTaskListDue("2026-11-26", "09:00")).toBe("明天 09:00");
    expect(formatTaskListDue("2026-11-27", "14:30")).toBe("后天 14:30");
    expect(formatTaskListDue("2026-11-28", "09:00")).toBe("11月28日 09:00");
    expect(formatTaskListDue("2026-12-02", "08:00")).toBe("12月2日 08:00");
    expect(formatTaskListDue("2027-01-05", "16:20")).toBe("1月5日 16:20");
    // 今天不属于“明天/后天”，统一显示月日
    expect(formatTaskListDue("2026-11-25", "09:00")).toBe("11月25日 09:00");
    expect(formatTaskListDue(null, "09:00")).toBe("");
  });

  it("跨月、跨年按自然日计算而不是固定小时差", () => {
    vi.useFakeTimers();
    setDisplayTimezone("Asia/Shanghai");
    vi.setSystemTime(new Date("2026-12-31T09:00:00+08:00"));
    expect(formatTaskListDue("2027-01-01", "08:00")).toBe("明天 08:00");
    vi.setSystemTime(new Date("2026-11-30T23:50:00+08:00"));
    expect(formatTaskListDue("2026-12-01", "00:10")).toBe("明天 00:10");
    expect(formatTaskListDue("2027-01-01", "00:00")).toBe("1月1日 00:00");
  });

  it("按用户当前时区判断今天，避免跨时区日期偏移", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-25T16:30:00Z")); // UTC 11-25 / 上海 11-26
    setDisplayTimezone("UTC");
    expect(todayString()).toBe("2026-11-25");
    expect(formatTaskListDue("2026-11-26", "09:00")).toBe("明天 09:00");
    setDisplayTimezone("Asia/Shanghai");
    expect(todayString()).toBe("2026-11-26");
    expect(formatTaskListDue("2026-11-27", "09:00")).toBe("明天 09:00");
    expect(formatTaskListDue("2026-11-26", "09:00")).toBe("11月26日 09:00");
  });
});

describe("日历网格", () => {
  it("返回 42 个单元格且包含当月首日", () => {
    const cells = monthGrid(2026, 1); // 2026 年 2 月
    expect(cells).toHaveLength(42);
    expect(cells).toContain("2026-02-01");
    // 2026-02-01 是周日，网格应从周一 2026-01-26 开始
    expect(cells[0]).toBe("2026-01-26");
    expect(cells[6]).toBe("2026-02-01");
  });
});

describe("提醒与重复描述", () => {
  it("reminderLabel 覆盖分钟/小时/天", () => {
    expect(reminderLabel(null)).toBe("不提醒");
    expect(reminderLabel(0)).toBe("准时提醒");
    expect(reminderLabel(30)).toBe("提前 30 分钟");
    expect(reminderLabel(120)).toBe("提前 2 小时");
    expect(reminderLabel(2880)).toBe("提前 2 天");
    expect(reminderLabel(90)).toBe("提前 1 小时 30 分钟");
    expect(reminderLabel(1635)).toBe("提前 1 天 3 小时 15 分钟");
  });

  it("自定义提醒的天/时/分拆分与合并", () => {
    expect(splitReminderOffset(1635)).toEqual({ days: 1, hours: 3, minutes: 15 });
    expect(splitReminderOffset(0)).toEqual({ days: 0, hours: 0, minutes: 0 });
    expect(combineReminderOffset(0, 0, 30)).toBe(30);
    expect(combineReminderOffset(0, 2, 0)).toBe(120);
    expect(combineReminderOffset(1, 3, 15)).toBe(1635);
    expect(combineReminderOffset(7, 0, 0)).toBe(10080);
    expect(combineReminderOffset(-1, -2, -3)).toBe(0);
  });

  it("跨月、跨年与闰年的日历单元格", () => {
    const dec = monthGrid(2026, 11);
    expect(dec).toContain("2026-12-31");
    expect(dec).toContain("2027-01-01");
    const feb = monthGrid(2024, 1);
    expect(feb).toContain("2024-02-29");
  });

  it("recurrenceLabel 输出中文规则", () => {
    expect(
      recurrenceLabel({ kind: "daily", interval: 1, weekdays: "", monthDay: null }),
    ).toBe("每天");
    expect(
      recurrenceLabel({ kind: "daily", interval: 2, weekdays: "", monthDay: null }),
    ).toBe("每 2 天");
    expect(
      recurrenceLabel({ kind: "weekdays", interval: 1, weekdays: "", monthDay: null }),
    ).toBe("每个工作日");
    expect(
      recurrenceLabel({ kind: "weekly", interval: 1, weekdays: "1,3,5", monthDay: null }),
    ).toBe("每周 周一、周三、周五");
    expect(
      recurrenceLabel({ kind: "monthly", interval: 1, weekdays: "", monthDay: 31 }),
    ).toBe("每月 31 日");
    expect(
      recurrenceLabel({ kind: "yearly", interval: 1, weekdays: "", monthDay: null }),
    ).toBe("每年");
  });

  it("默认提醒偏移为准时提醒", () => {
    expect(defaultReminderOffset()).toBe(0);
  });
});

describe("显示时区", () => {
  afterEach(() => {
    vi.useRealTimers();
    setDisplayTimezone(null);
  });

  it("手动时区影响 todayString", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-24T20:30:00Z"));
    setDisplayTimezone("UTC");
    expect(todayString()).toBe("2026-11-24");
    setDisplayTimezone("Asia/Shanghai");
    expect(todayString()).toBe("2026-11-25");
  });

  it("手动时区影响时间戳显示", () => {
    setDisplayTimezone("Asia/Shanghai");
    expect(formatTimestamp(0)).toBe("1970-01-01 08:00");
    setDisplayTimezone("UTC");
    expect(formatTimestamp(0)).toBe("1970-01-01 00:00");
  });

  it("无效时区回退到系统时区", () => {
    setDisplayTimezone("Invalid/Zone");
    expect(() => todayString()).not.toThrow();
    expect(() => formatTimestamp(0)).not.toThrow();
  });
});

describe("周数计算（周一为一周开始）", () => {
  it("同自然周算第 1 周", () => {
    // 2026-03-16 是周一，同周的周一到周日（3-16 至 3-22）都应是第 1 周
    expect(weekNumberByBase("2026-03-16", "2026-03-16")).toBe(1); // 周一
    expect(weekNumberByBase("2026-03-18", "2026-03-16")).toBe(1); // 周三
    expect(weekNumberByBase("2026-03-22", "2026-03-16")).toBe(1); // 周日
    // 基准日为同周周三，同一周周一也应是第 1 周
    expect(weekNumberByBase("2026-03-16", "2026-03-18")).toBe(1);
    expect(weekNumberByBase("2026-03-22", "2026-03-22")).toBe(1);
  });

  it("下一周算第 2 周", () => {
    expect(weekNumberByBase("2026-03-23", "2026-03-16")).toBe(2); // 下周一
    expect(weekNumberByBase("2026-03-29", "2026-03-16")).toBe(2); // 下周日
  });

  it("跨月跨年正确：函数严格按给定基准计算，跨年不自动重置", () => {
    // 2026-01-01(周四) 作为基准，该周算第 1 周
    expect(weekNumberByBase("2026-01-01", "2026-01-01")).toBe(1);
    // 2027-01-01(周五) 距基准 52 周
    expect(weekNumberByBase("2027-01-01", "2026-01-01")).toBe(53);
    // 自定义基准跨年仍按原基准
    expect(weekNumberByBase("2027-01-01", "2026-06-15")).toBeGreaterThan(25);
  });

  it("2026 年 1 月 1 日是周四，所在周(12-29至1-4)算第 1 周", () => {
    expect(weekNumberByBase("2025-12-29", "2026-01-01")).toBe(1); // 周一
    expect(weekNumberByBase("2026-01-01", "2026-01-01")).toBe(1);
    expect(weekNumberByBase("2026-01-04", "2026-01-01")).toBe(1); // 周日
    expect(weekNumberByBase("2026-01-05", "2026-01-01")).toBe(2); // 下周一
  });

  it("自定义基准：2026-03-18(周三)作为第一周", () => {
    // 该周的周一 3-16 至 周日 3-22 都是第 1 周
    expect(weekNumberByBase("2026-03-16", "2026-03-18")).toBe(1);
    expect(weekNumberByBase("2026-03-22", "2026-03-18")).toBe(1);
    expect(weekNumberByBase("2026-03-23", "2026-03-18")).toBe(2);
    expect(weekNumberByBase("2026-03-30", "2026-03-18")).toBe(3);
  });

  it("当前日期早于基准时返回非正数", () => {
    expect(weekNumberByBase("2026-03-09", "2026-03-18")).toBe(0); // 基准前一周
    expect(weekNumberByBase("2026-03-02", "2026-03-18")).toBe(-1); // 基准前两周
  });

  it("currentWeekNumber：基准为 null 时用当年 1 月 1 日", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-25T10:00:00"));
    // 2026-11-25 周三；以 2026-01-01 为基准计算
    const w = currentWeekNumber(null);
    expect(w).toBeGreaterThan(0);
    // 显式传 baseDate 应与 null 等价（同一天）
    expect(currentWeekNumber(null)).toBe(weekNumberByBase(todayString(), "2026-01-01"));
    vi.useRealTimers();
  });
});

describe("月历与单日标题", () => {
  it("月份标题与日期标题", () => {
    expect(monthTitle(2026, 11)).toBe("2026年12月");
    expect(monthTitle(2027, 0)).toBe("2027年1月");
    expect(formatDayTitle("2026-12-08")).toBe("2026年12月8日");
    expect(formatDayTitle("2026-01-01")).toBe("2026年1月1日");
  });

  it("月份网格跨年时保持连续 42 天", () => {
    const cells = monthGrid(2026, 11); // 2026 年 12 月
    expect(cells).toHaveLength(42);
    expect(cells[0]).toBe("2026-11-30");
    expect(cells[41]).toBe("2027-01-10");
  });
});
