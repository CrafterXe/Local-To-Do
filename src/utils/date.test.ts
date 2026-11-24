import { afterEach, describe, expect, it, vi } from "vitest";
import {
  addDays,
  combineReminderOffset,
  defaultReminderOffset,
  diffDays,
  formatDueLabel,
  formatDueFull,
  formatTimestamp,
  monthGrid,
  parseDateString,
  recurrenceLabel,
  reminderLabel,
  setDisplayTimezone,
  splitReminderOffset,
  toDateString,
  todayString,
  weekdayIndex,
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
