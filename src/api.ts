import { invoke } from "@tauri-apps/api/core";

export interface TaskList {
  id: string;
  name: string;
  color: string;
  icon: string;
  sortOrder: number;
  createdAt: number;
  openCount: number;
  totalCount: number;
}

export interface Recurrence {
  id: string;
  kind: string;
  interval: number;
  weekdays: string;
  monthDay: number | null;
  baseDate: string;
  createdAt: number;
}

export interface TaskStep {
  id: string;
  taskId: string;
  title: string;
  isCompleted: boolean;
  sortOrder: number;
}

export interface Task {
  id: string;
  listId: string;
  title: string;
  note: string;
  isImportant: boolean;
  isCompleted: boolean;
  completedAt: number | null;
  dueDate: string | null;
  dueTime: string | null;
  reminderOffset: number | null;
  reminderEnabled: boolean;
  reminderAt: number | null;
  recurrenceId: string | null;
  seriesId: string | null;
  parentTaskId: string | null;
  recurrenceCount: number;
  myDay: string | null;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
  steps: TaskStep[];
  recurrence: Recurrence | null;
  listName: string;
  listColor: string;
}

export interface Note {
  id: string;
  title: string;
  body: string;
  createdAt: number;
  updatedAt: number;
}

export interface DeletedList {
  id: string;
  name: string;
  color: string;
  sortOrder: number;
  deletedAt: number | null;
  taskCount: number;
}

export interface AppSettings {
  theme: string;
  notificationsEnabled: boolean;
  soundEnabled: boolean;
  soundPath: string | null;
  closeToTray: boolean;
  autostart: boolean;
  startMinimized: boolean;
  autoBackupEnabled: boolean;
  autoBackupIntervalHours: number;
  lastBackupAt: number | null;
  defaultListId: string | null;
  timezoneAuto: boolean;
  timezone: string | null;
  weekBaseDate: string | null;
  weekCustomized: boolean;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  dbPath: string;
  backupDir: string;
  logDir: string;
  today: string;
  now: number;
}

export interface BackupEntry {
  path: string;
  fileName: string;
  size: number;
  createdAt: number;
  format: string;
}

export interface HistoryEntry {
  id: number;
  taskId: string;
  taskTitle: string;
  action: string;
  detail: string;
  at: number;
}

/** 月历中某一天的任务统计（仅用于展示） */
export interface CalendarDay {
  date: string;
  total: number;
  nonRepeat: number;
  repeat: number;
  hasImportant: boolean;
}

export interface SidebarCounts {
  myDay: number;
  important: number;
  planned: number;
  allOpen: number;
  completed: number;
  deleted: number;
  notes: number;
}

export interface DbStats {
  tasks: number;
  lists: number;
  notes: number;
  pendingReminders: number;
  databaseBytes: number;
}

export interface RestoreReport {
  lists: number;
  tasks: number;
  notes: number;
  recurrences: number;
}

export interface SyncStatus {
  running: boolean;
  protocolVersion: string;
  host: string | null;
  port: number | null;
  fingerprint: string | null;
  startedAt: number | null;
  deviceCount: number;
  pairingActive: boolean;
  pairingExpiresAt: number | null;
  firewallHint: string;
  error: string | null;
}

export interface PairingInfo {
  code: string;
  expiresAt: number;
  host: string;
  port: number;
  fingerprint: string;
  protocolVersion: string;
  payload: string;
  qrSvg: string;
}

export interface SyncDevice {
  id: string;
  name: string;
  createdAt: number;
  lastSeenAt: number | null;
}

export interface RecurrenceInput {
  kind: string;
  interval: number;
  weekdays: string;
  monthDay: number | null;
}

export interface TaskInput {
  title: string;
  listId: string | null;
  note: string | null;
  isImportant?: boolean | null;
  dueDate: string | null;
  dueTime: string | null;
  reminderOffset: number | null;
  reminderEnabled?: boolean | null;
  myDay?: string | null;
  recurrence: RecurrenceInput | null;
}

export type ViewKind =
  | "myDay"
  | "important"
  | "planned"
  | "all"
  | "completed"
  | "deleted"
  | "notes"
  | "settings"
  | "history"
  | "calendar"
  | "day"
  | "lists"
  | "list";

export interface View {
  kind: ViewKind;
  listId?: string;
  /** 单日任务视图对应的日期 YYYY-MM-DD */
  date?: string;
}

/** Android 只读同步客户端的连接信息（不含令牌） */
export interface MobileConnectionInfo {
  host: string;
  port: number;
  fingerprint: string;
  protocolVersion: string;
  deviceId: string;
  deviceName: string;
  pairedAt: number;
}

export type MobileSyncPhase =
  | "unpaired"
  | "pairing"
  | "paired"
  | "firstSync"
  | "syncing"
  | "synced"
  | "offline"
  | "failed"
  | "needsPairing";

export interface MobileSyncCounts {
  lists: number;
  tasks: number;
  recurrences: number;
  steps: number;
}

export interface MobileSyncState {
  paired: boolean;
  connection: MobileConnectionInfo | null;
  phase: MobileSyncPhase;
  lastSyncAt: number | null;
  lastRev: number;
  needsSnapshot: boolean;
  counts: MobileSyncCounts;
  error: string | null;
}

export interface MobilePairingPreview {
  host: string;
  port: number;
  fingerprint: string;
  expiresAt: number;
  protocolVersion: string;
  expired: boolean;
}

/** Android 只读查询条件（仅影响展示结果） */
export interface MobileTaskQuery {
  scope?: "all" | "today" | "completed" | "list";
  listId?: string | null;
  search?: string | null;
  includeCompleted?: boolean | null;
}

/** Android 本地设置，不参与 Windows 同步 */
export interface MobileSettings {
  notificationsEnabled: boolean;
  reminderLeadMinutes: number;
}

/** Android 后台提醒状态（由原生层与本地设置共同决定） */
export interface ReminderStatus {
  enabled: boolean;
  leadMinutes: number;
  scheduled: number;
  exactAlarmAllowed: boolean;
  notificationsAllowed: boolean;
  ignoringBatteryOptimizations: boolean;
  skippedPast: number;
  skippedFar: number;
  /** 超出系统一次性注册上限的后续提醒数量 */
  truncated?: number;
  error: string | null;
}

export const api = {
  listLists: () => invoke<TaskList[]>("list_lists"),
  createList: (name: string, color: string, icon = "") =>
    invoke<TaskList>("create_list", { name, color, icon }),
  updateList: (id: string, name: string, color: string, icon: string) =>
    invoke<TaskList>("update_list", { id, name, color, icon }),
  deleteList: (id: string, mode: "move" | "delete", targetListId?: string | null) =>
    invoke<void>("delete_list", { id, mode, targetListId: targetListId ?? null }),
  reorderLists: (ids: string[]) => invoke<void>("reorder_lists", { ids }),
  listDeletedLists: () => invoke<DeletedList[]>("list_deleted_lists"),
  restoreList: (id: string) => invoke<void>("restore_list", { id }),
  purgeList: (id: string) => invoke<void>("purge_list", { id }),

  listTasks: (query: {
    view: string;
    listId?: string | null;
    search?: string | null;
    includeCompleted?: boolean | null;
  }) => invoke<Task[]>("list_tasks", { query }),
  calendarMonth: (start: string, end: string) =>
    invoke<CalendarDay[]>("calendar_month", { start, end }),
  listTasksForDay: (date: string) => invoke<Task[]>("list_tasks_for_day", { date }),
  listNotifiedTasks: () => invoke<Task[]>("list_notified_tasks"),
  getTask: (id: string) => invoke<Task>("get_task", { id }),
  createTask: (input: TaskInput) => invoke<Task>("create_task", { input }),
  updateTask: (id: string, input: TaskInput) => invoke<Task>("update_task", { id, input }),
  setImportant: (id: string, important: boolean) =>
    invoke<Task>("set_important", { id, important }),
  setMyDay: (id: string, add: boolean) => invoke<Task>("set_my_day", { id, add }),
  moveTask: (id: string, listId: string) => invoke<Task>("move_task", { id, listId }),
  completeTask: (id: string, completed: boolean) =>
    invoke<Task | null>("complete_task", { id, completed }),
  deleteTask: (id: string, scope: "single" | "series") =>
    invoke<void>("delete_task", { id, scope }),
  restoreTask: (id: string) => invoke<Task>("restore_task", { id }),
  purgeTask: (id: string) => invoke<void>("purge_task", { id }),
  emptyDeleted: () => invoke<void>("empty_deleted"),
  reorderTasks: (ids: string[]) => invoke<void>("reorder_tasks", { ids }),
  sidebarCounts: () => invoke<SidebarCounts>("sidebar_counts"),

  addStep: (taskId: string, title: string) => invoke<Task>("add_step", { taskId, title }),
  updateStep: (stepId: string, title: string, completed: boolean) =>
    invoke<void>("update_step", { stepId, title, completed }),
  deleteStep: (stepId: string) => invoke<void>("delete_step", { stepId }),

  listHistory: (limit = 200) => invoke<HistoryEntry[]>("list_history", { limit }),
  listTaskHistory: (taskId: string) =>
    invoke<HistoryEntry[]>("list_task_history", { taskId }),

  listNotes: (search?: string) => invoke<Note[]>("list_notes", { search: search ?? null }),
  getNote: (id: string) => invoke<Note>("get_note", { id }),
  createNote: (title: string, body: string) => invoke<Note>("create_note", { title, body }),
  updateNote: (id: string, title: string, body: string) =>
    invoke<Note>("update_note", { id, title, body }),
  deleteNote: (id: string) => invoke<void>("delete_note", { id }),

  getSettings: () => invoke<AppSettings>("get_settings"),
  updateSettings: (settings: AppSettings) =>
    invoke<AppSettings>("update_settings", { settings }),
  resetSettings: () => invoke<AppSettings>("reset_settings"),
  resyncReminders: () => invoke<void>("resync_reminders"),
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getDbStats: () => invoke<DbStats>("get_db_stats"),
  sendTestNotification: () => invoke<boolean>("send_test_notification"),
  previewSound: (path: string | null) => invoke<void>("preview_sound", { path }),
  checkSoundFile: (path: string | null) => invoke<boolean>("check_sound_file", { path }),
  getNextReminder: () => invoke<number | null>("get_next_reminder"),

  createBackup: () => invoke<BackupEntry>("create_backup"),
  listBackups: () => invoke<BackupEntry[]>("list_backups"),
  exportJsonFile: (path: string) => invoke<string>("export_json_file", { path }),
  exportCsvFile: (path: string) => invoke<string>("export_csv_file", { path }),
  restoreBackup: (path: string) => invoke<RestoreReport>("restore_backup", { path }),
  restoreBackupText: (text: string) => invoke<RestoreReport>("restore_backup_text", { text }),
  validateBackup: (path: string) => invoke<RestoreReport>("validate_backup", { path }),
  openDataDir: () => invoke<void>("open_data_dir"),
  openBackupDir: () => invoke<void>("open_backup_dir"),

  syncServerStatus: () => invoke<SyncStatus>("sync_server_status"),
  syncServerStart: () => invoke<SyncStatus>("sync_server_start"),
  syncServerStop: () => invoke<SyncStatus>("sync_server_stop"),
  syncCreatePairing: () => invoke<PairingInfo>("sync_create_pairing"),
  syncCancelPairing: () => invoke<void>("sync_cancel_pairing"),
  syncListDevices: () => invoke<SyncDevice[]>("sync_list_devices"),
  syncRevokeDevice: (id: string) => invoke<SyncDevice[]>("sync_revoke_device", { id }),
  syncResetPairing: () => invoke<SyncStatus>("sync_reset_pairing"),

  mobileSyncState: () => invoke<MobileSyncState>("mobile_sync_state"),
  mobileInspectPairing: (payload: string) =>
    invoke<MobilePairingPreview>("mobile_inspect_pairing", { payload }),
  mobilePair: (payload: string, deviceName: string) =>
    invoke<MobileSyncState>("mobile_pair", { payload, deviceName }),
  mobileSyncNow: () => invoke<MobileSyncState>("mobile_sync_now"),
  mobileUnpair: () => invoke<MobileSyncState>("mobile_unpair"),

  mobileLists: () => invoke<TaskList[]>("mobile_lists"),
  mobileTasks: (query: MobileTaskQuery) => invoke<Task[]>("mobile_tasks", { query }),
  mobileTask: (id: string) => invoke<Task>("mobile_task", { id }),
  mobileTasksForDay: (date: string) => invoke<Task[]>("mobile_tasks_for_day", { date }),
  mobileCalendar: (start: string, end: string) =>
    invoke<CalendarDay[]>("mobile_calendar", { start, end }),
  mobileSettingsGet: () => invoke<MobileSettings>("mobile_settings_get"),
  mobileSettingsUpdate: (settings: MobileSettings) =>
    invoke<MobileSettings>("mobile_settings_update", { settings }),
  mobileRemindersResync: () => invoke<ReminderStatus>("mobile_reminders_resync"),
  mobileReminderStatus: () => invoke<ReminderStatus>("mobile_reminder_status"),
  mobileReminderOpenExactSettings: () =>
    invoke<void>("mobile_reminder_open_exact_alarm_settings"),
  mobileReminderOpenBatterySettings: () =>
    invoke<void>("mobile_reminder_open_battery_settings"),
  mobileReminderRequestNotifications: () =>
    invoke<boolean>("mobile_reminder_request_notifications"),
  mobileBackupShare: (path: string) => invoke<void>("mobile_backup_share", { path }),
};
