// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  api,
  type AppSettings,
  type Note,
  type ReminderStatus,
  type Task,
  type TaskList,
} from "../api";
import { MobileOfflineApp } from "./MobileOfflineApp";

const list: TaskList = {
  id: "list-1",
  name: "任务",
  color: "#2f6fed",
  icon: "",
  sortOrder: 0,
  createdAt: 1,
  openCount: 1,
  totalCount: 1,
};

const task: Task = {
  id: "task-1",
  listId: list.id,
  title: "写周报",
  note: " Friday 前完成",
  isImportant: false,
  isCompleted: false,
  completedAt: null,
  dueDate: "2026-11-27",
  dueTime: "18:00",
  reminderOffset: 0,
  reminderEnabled: true,
  reminderAt: 1_800_000_000_000,
  recurrenceId: null,
  seriesId: null,
  parentTaskId: null,
  recurrenceCount: 0,
  myDay: "2026-11-26",
  sortOrder: 0,
  createdAt: 1,
  updatedAt: 1,
  deletedAt: null,
  steps: [],
  recurrence: null,
  listName: list.name,
  listColor: list.color,
};

const note: Note = {
  id: "note-1",
  title: "购物清单",
  body: "牛奶",
  createdAt: 1,
  updatedAt: 1,
};

const settings: AppSettings = {
  theme: "dark",
  notificationsEnabled: true,
  soundEnabled: true,
  soundPath: null,
  closeToTray: false,
  autostart: false,
  startMinimized: false,
  autoBackupEnabled: true,
  autoBackupIntervalHours: 24,
  lastBackupAt: null,
  defaultListId: list.id,
  timezoneAuto: true,
  timezone: null,
  weekBaseDate: null,
  weekCustomized: false,
};

const reminder: ReminderStatus = {
  enabled: true,
  leadMinutes: 0,
  scheduled: 1,
  exactAlarmAllowed: true,
  notificationsAllowed: true,
  ignoringBatteryOptimizations: true,
  skippedPast: 0,
  skippedFar: 0,
  error: null,
};

beforeEach(() => {
  window.localStorage.clear();
  window.matchMedia = vi.fn().mockReturnValue({ matches: true, addListener: vi.fn(), removeListener: vi.fn() });
  vi.spyOn(api, "listLists").mockResolvedValue([list]);
  vi.spyOn(api, "sidebarCounts").mockResolvedValue({
    myDay: 1,
    important: 0,
    planned: 1,
    allOpen: 1,
    completed: 0,
    deleted: 0,
    notes: 1,
  });
  vi.spyOn(api, "getSettings").mockResolvedValue(settings);
  vi.spyOn(api, "mobileRemindersResync").mockResolvedValue(reminder);
  vi.spyOn(api, "mobileReminderRequestNotifications").mockResolvedValue(true);
  vi.spyOn(api, "mobileReminderStatus").mockResolvedValue(reminder);
  vi.spyOn(api, "getAppInfo").mockResolvedValue({
    version: "1.2.1",
    dataDir: "/data",
    dbPath: "/data/local-to-do.db",
    backupDir: "/data/backups",
    logDir: "/data/logs",
    today: "2026-11-26",
    now: 1,
  });
  vi.spyOn(api, "getDbStats").mockResolvedValue({
    tasks: 1,
    lists: 1,
    notes: 1,
    pendingReminders: 1,
    databaseBytes: 4096,
  });
  vi.spyOn(api, "listBackups").mockResolvedValue([]);
  vi.spyOn(api, "listTasks").mockResolvedValue([task]);
  vi.spyOn(api, "listTasksForDay").mockResolvedValue([task]);
  vi.spyOn(api, "getTask").mockResolvedValue(task);
  vi.spyOn(api, "listNotes").mockResolvedValue([note]);
  vi.spyOn(api, "listHistory").mockResolvedValue([]);
  vi.spyOn(api, "listNotifiedTasks").mockResolvedValue([]);
  vi.spyOn(api, "calendarMonth").mockResolvedValue([]);
  vi.spyOn(api, "listDeletedLists").mockResolvedValue([]);
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

async function renderBooted() {
  const result = render(<MobileOfflineApp />);
  await screen.findByRole("heading", { name: "我的一天" });
  await screen.findAllByText("写周报");
  return result;
}

describe("独立 Android 主业务链路", () => {
  it("离线启动后快速创建、编辑详情并完成任务", async () => {
    vi.spyOn(api, "createTask").mockResolvedValue(task);
    vi.spyOn(api, "updateTask").mockImplementation(async (_id, input) => ({
      ...task,
      title: input.title,
      note: input.note ?? task.note,
    }));
    vi.spyOn(api, "completeTask").mockResolvedValue(null);
    await renderBooted();

    fireEvent.change(screen.getByPlaceholderText("添加任务"), { target: { value: "新任务" } });
    fireEvent.keyDown(screen.getByPlaceholderText("添加任务"), { key: "Enter" });
    await waitFor(() => expect(api.createTask).toHaveBeenCalled());
    expect(vi.mocked(api.createTask).mock.calls[0][0]).toMatchObject({
      title: "新任务",
      listId: list.id,
      reminderEnabled: false,
    });
    expect(vi.mocked(api.createTask).mock.calls[0][0].myDay).toMatch(/^\d{4}-\d{2}-\d{2}$/);

    fireEvent.click(screen.getByText("写周报"));
    const title = await screen.findByDisplayValue("写周报");
    fireEvent.change(title, { target: { value: "写本周周报" } });
    await waitFor(() => expect(api.updateTask).toHaveBeenCalled(), { timeout: 1800 });
    expect(vi.mocked(api.updateTask).mock.calls[0][1].title).toBe("写本周周报");

    fireEvent.click(screen.getAllByTitle("标记完成")[0]);
    await waitFor(() => expect(api.completeTask).toHaveBeenCalledWith(task.id, true));
  });

  it("面板互斥、系统返回关闭详情，并保留每个视图的搜索状态", async () => {
    await renderBooted();
    fireEvent.click(screen.getByText("写周报"));
    await screen.findByDisplayValue("写周报");
    expect(document.querySelector(".offline-detail-panel")).not.toBeNull();

    fireEvent(window, new PopStateEvent("popstate"));
    await waitFor(() => expect(document.querySelector(".offline-detail-panel")).toBeNull());

    fireEvent.change(screen.getByPlaceholderText("搜索任务和备注"), { target: { value: "周报" } });
    await waitFor(() => expect(api.listTasks).toHaveBeenCalledWith({ view: "all", search: "周报" }));

    fireEvent.click(screen.getByRole("button", { name: "展开导航" }));
    expect(document.querySelector(".offline-nav-drawer")).not.toBeNull();
    expect(document.querySelector(".offline-detail-panel")).toBeNull();
    fireEvent.click(screen.getAllByRole("button", { name: "全部任务" }).at(-1)!);
    await screen.findByRole("heading", { name: "全部任务" });

    fireEvent.click(screen.getByRole("button", { name: "我的一天" }));
    await screen.findByRole("heading", { name: "搜索结果" });
    expect(screen.getByPlaceholderText("搜索任务和备注")).toHaveProperty("value", "周报");
  });

  it("创建清单，并通过右侧详情自动保存和删除备忘录", async () => {
    vi.spyOn(api, "createList").mockResolvedValue({ ...list, id: "list-2", name: "工作" });
    vi.spyOn(api, "updateNote").mockImplementation(async (id, title, body) => ({ ...note, id, title, body }));
    vi.spyOn(api, "deleteNote").mockResolvedValue();
    await renderBooted();

    fireEvent.click(screen.getByRole("button", { name: "我的清单" }));
    await screen.findByRole("heading", { name: "我的清单" });
    fireEvent.click(screen.getAllByRole("button", { name: "新建清单" })[0]);
    fireEvent.change(screen.getByPlaceholderText("例如：工作、学习、购物"), { target: { value: "工作" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await waitFor(() => expect(api.createList).toHaveBeenCalledWith("工作", expect.any(String)));

    fireEvent.click(screen.getByRole("button", { name: "备忘录" }));
    fireEvent.click(await screen.findByText("购物清单"));
    const title = await screen.findByLabelText("备忘录标题");
    fireEvent.change(title, { target: { value: "周末购物" } });
    await waitFor(() => expect(api.updateNote).toHaveBeenCalledWith(note.id, "周末购物", "牛奶"), { timeout: 1800 });

    fireEvent.click(screen.getByRole("button", { name: "删除" }));
    const deleteButtons = await screen.findAllByRole("button", { name: "删除" });
    fireEvent.click(deleteButtons.at(-1)!);
    await waitFor(() => expect(api.deleteNote).toHaveBeenCalledWith(note.id));
  });

  it("保存设置、创建备份并在确认后恢复导入文件", async () => {
    vi.spyOn(api, "mobileReminderRequestNotifications").mockResolvedValue(true);
    vi.spyOn(api, "updateSettings").mockImplementation(async (next) => next);
    vi.spyOn(api, "createBackup").mockResolvedValue({
      path: "/data/backups/a.json",
      fileName: "a.json",
      size: 100,
      createdAt: 1,
      format: "json",
    });
    vi.spyOn(api, "restoreBackupText").mockResolvedValue({ lists: 1, tasks: 2, notes: 1, recurrences: 0 });
    const { container } = await renderBooted();

    fireEvent.click(screen.getByRole("button", { name: "设置" }));
    await screen.findByRole("heading", { name: "设置" });
    fireEvent.change(screen.getByLabelText("主题"), { target: { value: "light" } });
    await waitFor(() => expect(api.updateSettings).toHaveBeenCalledWith({ ...settings, theme: "light" }));

    fireEvent.click(screen.getByRole("button", { name: "立即备份到本机" }));
    await waitFor(() => expect(api.createBackup).toHaveBeenCalled());

    const input = container.querySelector<HTMLInputElement>("input[type='file']")!;
    const bundle = JSON.stringify({
      format: "local-to-do-backup",
      version: 1,
      data: { lists: [{}], tasks: [{}, {}], notes: [{}], recurrences: [] },
    });
    fireEvent.change(input, { target: { files: [new File([bundle], "backup.json", { type: "application/json" })] } });
    fireEvent.click(await screen.findByRole("button", { name: "覆盖并恢复" }));
    await waitFor(() => expect(api.restoreBackupText).toHaveBeenCalledWith(bundle));
  });

  it("在任务详情中编辑日期时间、提醒、重复规则和步骤", async () => {
    vi.spyOn(api, "updateTask").mockImplementation(async (_id, input) => ({
      ...task,
      title: input.title,
      dueDate: input.dueDate,
      dueTime: input.dueTime,
      reminderOffset: input.reminderOffset,
      recurrence: input.recurrence
        ? { id: "r1", baseDate: input.dueDate ?? "2026-11-26", createdAt: 1, ...input.recurrence }
        : null,
    }));
    vi.spyOn(api, "addStep").mockResolvedValue({
      ...task,
      steps: [{ id: "s1", taskId: task.id, title: "收集数据", isCompleted: false, sortOrder: 0 }],
    });
    await renderBooted();
    fireEvent.click(screen.getByText("写周报"));
    await screen.findByDisplayValue("写周报");

    fireEvent.click(screen.getByText("到期日期").closest("button")!);
    fireEvent.click(screen.getByRole("button", { name: "明天" }));
    fireEvent.change(screen.getByLabelText("小时"), { target: { value: "20" } });
    fireEvent.change(screen.getByLabelText("分钟"), { target: { value: "35" } });
    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    await waitFor(() => expect(api.updateTask).toHaveBeenCalled(), { timeout: 1800 });
    expect(vi.mocked(api.updateTask).mock.calls.at(-1)?.[1]).toMatchObject({ dueTime: "20:35" });

    fireEvent.click(document.querySelector(".reminder-row")!);
    fireEvent.click(screen.getByRole("button", { name: "提前 20 分钟" }));
    fireEvent.click(screen.getByRole("button", { name: "确认" }));
    await waitFor(() => expect(vi.mocked(api.updateTask).mock.calls.at(-1)?.[1].reminderOffset).toBe(20), { timeout: 1800 });

    fireEvent.click(screen.getByText("不重复").closest("button")!);
    fireEvent.click(screen.getByRole("button", { name: "每年" }));
    await waitFor(() => expect(vi.mocked(api.updateTask).mock.calls.at(-1)?.[1].recurrence?.kind).toBe("yearly"), { timeout: 1800 });

    fireEvent.change(screen.getByPlaceholderText("添加步骤"), { target: { value: "收集数据" } });
    fireEvent.keyDown(screen.getByPlaceholderText("添加步骤"), { key: "Enter" });
    await waitFor(() => expect(api.addStep).toHaveBeenCalledWith(task.id, "收集数据"));
  });

  it("展示已触发提醒并可打开对应任务", async () => {
    vi.mocked(api.listNotifiedTasks).mockResolvedValue([task]);
    await renderBooted();
    fireEvent.click(await screen.findByText("提醒已触发"));
    await screen.findByDisplayValue("写周报");
    expect(document.querySelector(".offline-detail-panel")).not.toBeNull();
  });

  it("滑出导航在 Logo 右侧显示周数，并将全部任务置于首位", async () => {
    await renderBooted();
    fireEvent.click(screen.getByRole("button", { name: "展开导航" }));

    const brand = document.querySelector(".offline-drawer-brand");
    expect(document.querySelector(".offline-drawer-logo")).not.toBeNull();
    expect(brand?.lastElementChild?.classList.contains("sidebar-week-wrap")).toBe(true);
    expect(screen.getByRole("button", { name: /^第\d+周$/ })).not.toBeNull();
    expect(document.querySelectorAll(".offline-drawer-item")[0].textContent).toContain("全部任务");
  });

  it("保存清单手动排序", async () => {
    const second = { ...list, id: "list-2", name: "工作" };
    vi.mocked(api.listLists).mockResolvedValue([list, second]);
    vi.spyOn(api, "reorderLists").mockResolvedValue();
    await renderBooted();
    fireEvent.click(screen.getByRole("button", { name: "我的清单" }));
    fireEvent.click(await screen.findByRole("button", { name: "下移清单任务" }));
    await waitFor(() => expect(api.reorderLists).toHaveBeenCalledWith([second.id, list.id]));
  });

  it("切换月历并进入有任务的单日视图", async () => {
    const { monthGrid, todayString } = await import("../utils/date");
    const today = todayString();
    const date = parseDate(today);
    const cells = monthGrid(date.year, date.month);
    vi.mocked(api.calendarMonth).mockResolvedValue([{ date: cells[0], total: 1, nonRepeat: 1, repeat: 0, hasImportant: true }]);
    await renderBooted();
    fireEvent.click(screen.getByRole("button", { name: "日历" }));
    const cell = await screen.findByTitle("1个任务");
    fireEvent.click(cell);
    await waitFor(() => expect(api.listTasksForDay).toHaveBeenCalledWith(cells[0]));
    expect(screen.getByRole("button", { name: "返回日历" })).not.toBeNull();
  });
});

function parseDate(value: string): { year: number; month: number } {
  const [year, month] = value.split("-").map(Number);
  return { year, month: month - 1 };
}
