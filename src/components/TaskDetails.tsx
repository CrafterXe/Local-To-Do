import { useEffect, useRef, useState } from "react";
import { api, type RecurrenceInput, type Task, type TaskInput, type TaskList } from "../api";
import {
  addDays,
  combineReminderOffset,
  formatDueFull,
  formatFullDateTime,
  formatTimestamp,
  monthGrid,
  recurrenceLabel,
  reminderLabel,
  splitReminderOffset,
  todayString,
  WEEKDAY_LABELS,
  weekdayIndex,
} from "../utils/date";
import { Popover } from "./Popover";
import {
  IconBell,
  IconCalendar,
  IconCheck,
  IconChevronDown,
  IconChevronLeft,
  IconChevronRight,
  IconList,
  IconPlus,
  IconRepeat,
  IconStar,
  IconSun,
  IconTrash,
  IconX,
} from "./Icons";

interface Draft {
  title: string;
  note: string;
  dueDate: string | null;
  dueTime: string | null;
  reminderOffset: number | null;
  reminderEnabled: boolean;
  listId: string;
  recurrence: RecurrenceInput | null;
}

function fromTask(t: Task): Draft {
  return {
    title: t.title,
    note: t.note,
    // 重复任务编辑的是「初时间点」，当前周期日期由底层根据次数计算
    dueDate: t.recurrence ? t.recurrence.baseDate : t.dueDate,
    dueTime: t.dueTime,
    reminderOffset: t.reminderOffset,
    reminderEnabled: t.reminderEnabled,
    listId: t.listId,
    recurrence: t.recurrence
      ? {
          kind: t.recurrence.kind,
          interval: t.recurrence.interval,
          weekdays: t.recurrence.weekdays,
          monthDay: t.recurrence.monthDay,
        }
      : null,
  };
}

function toInput(d: Draft): TaskInput {
  // 只提交本面板实际编辑的字段；重要 / 我的一天由独立入口维护，绝不在此覆盖
  return {
    title: d.title,
    listId: d.listId,
    note: d.note,
    dueDate: d.dueDate,
    dueTime: d.dueDate ? d.dueTime ?? "09:00" : null,
    reminderOffset: d.reminderOffset,
    reminderEnabled: d.reminderEnabled,
    recurrence: d.recurrence,
  };
}

interface Props {
  task: Task;
  lists: TaskList[];
  onClose: () => void;
  onSave: (id: string, input: TaskInput) => Promise<void>;
  onDelete: (task: Task, scope: "single" | "series") => void;
  onToggleComplete: (task: Task) => void;
  onToggleImportant: (task: Task) => void;
  onToggleMyDay: (task: Task) => void;
  onRefreshTask: (task: Task) => void;
  notify: (message: string, error?: boolean) => void;
}

type PopKind = "due" | "reminder" | "repeat" | "list" | null;

export function TaskDetails({
  task,
  lists,
  onClose,
  onSave,
  onDelete,
  onToggleComplete,
  onToggleImportant,
  onToggleMyDay,
  onRefreshTask,
  notify,
}: Props) {
  const [draft, setDraft] = useState<Draft>(() => fromTask(task));
  const [pop, setPop] = useState<{ kind: PopKind; rect: DOMRect } | null>(null);
  const savedRef = useRef(JSON.stringify(fromTask(task)));
  const timerRef = useRef<number | undefined>(undefined);
  const [stepTitle, setStepTitle] = useState("");
  const [editingStep, setEditingStep] = useState<string | null>(null);
  const [stepText, setStepText] = useState("");

  useEffect(() => {
    const d = fromTask(task);
    setDraft(d);
    savedRef.current = JSON.stringify(d);
    setPop(null);
    // 仅在切换任务时重置草稿
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [task.id]);

  useEffect(() => {
    const serialized = JSON.stringify(draft);
    if (serialized === savedRef.current) return;
    window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(async () => {
      if (!draft.title.trim()) return;
      try {
        await onSave(task.id, toInput(draft));
        savedRef.current = serialized;
      } catch (e) {
        notify(`保存失败：${String(e)}`, true);
      }
    }, 450);
    return () => window.clearTimeout(timerRef.current);
  }, [draft, task.id, onSave, notify]);

  const update = (patch: Partial<Draft>) => setDraft((d) => ({ ...d, ...patch }));

  const openAt = (kind: PopKind, el: HTMLElement) => {
    setPop(pop?.kind === kind ? null : { kind, rect: el.getBoundingClientRect() });
  };

  const open = (kind: PopKind) => (e: React.MouseEvent<HTMLElement>) => {
    openAt(kind, e.currentTarget);
  };

  const applyDue = (date: string, time: string) => {
    // 仅更新到期日期与时间，不擅自改动提醒设置
    update({ dueDate: date, dueTime: time });
    setPop(null);
  };

  const applyReminder = (offset: number) => {
    if (!draft.dueDate) {
      const today = todayString();
      update({ dueDate: today, dueTime: draft.dueTime ?? "09:00", reminderOffset: offset });
    } else {
      update({ reminderOffset: offset });
    }
    setPop(null);
  };

  const toggleReminder = () => {
    if (draft.reminderEnabled) {
      // 关闭提醒：保留已选方案，仅取消开关与调度
      update({ reminderEnabled: false });
    } else {
      // 开启提醒：没有历史方案时默认「准时提醒」
      const offset = draft.reminderOffset ?? 0;
      if (!draft.dueDate) {
        update({
          dueDate: todayString(),
          dueTime: draft.dueTime ?? "09:00",
          reminderEnabled: true,
          reminderOffset: offset,
        });
      } else {
        update({ reminderEnabled: true, reminderOffset: offset });
      }
    }
    setPop(null);
  };

  const setRecurrence = (rec: RecurrenceInput | null) => {
    if (rec && !draft.dueDate) {
      update({ dueDate: todayString(), dueTime: draft.dueTime ?? "09:00", recurrence: rec });
    } else {
      update({ recurrence: rec });
    }
    setPop(null);
  };

  const addStep = async () => {
    const title = stepTitle.trim();
    if (!title) return;
    setStepTitle("");
    try {
      const updated = await api.addStep(task.id, title);
      onRefreshTask(updated);
    } catch (e) {
      notify(String(e), true);
    }
  };

  const saveStep = async (stepId: string, text: string, completed: boolean) => {
    setEditingStep(null);
    if (!text.trim()) return;
    try {
      await api.updateStep(stepId, text.trim(), completed);
      onRefreshTask(await api.getTask(task.id));
    } catch (e) {
      notify(String(e), true);
    }
  };

  const deleteStep = async (stepId: string) => {
    try {
      await api.deleteStep(stepId);
      onRefreshTask(await api.getTask(task.id));
    } catch (e) {
      notify(String(e), true);
    }
  };

  const listColor = lists.find((l) => l.id === draft.listId)?.color ?? "var(--accent)";
  const doneSteps = task.steps.filter((s) => s.isCompleted).length;

  return (
    <aside className="details">
      <div className="details-header">
        <span className="dot" style={{ background: listColor, width: 9, height: 9 }} />
        <span style={{ fontSize: 12.5, color: "var(--muted)", flex: 1 }}>任务详情</span>
        <button className="icon-btn" onClick={onClose} title="关闭详情">
          <IconX size={16} />
        </button>
      </div>

      <div className="details-scroll">
        <div className="detail-title-row">
          <button
            className={`check ${task.isCompleted ? "checked" : ""}`}
            style={{ marginTop: 6 }}
            title={
              task.recurrence
                ? "完成当前周期或推进到下一次"
                : task.isCompleted
                  ? "恢复为未完成"
                  : "标记完成"
            }
            onClick={() => onToggleComplete(task)}
          >
            <IconCheck size={12} />
          </button>
          <textarea
            className="detail-title-input"
            rows={Math.max(1, Math.ceil(draft.title.length / 22))}
            value={draft.title}
            onChange={(e) => update({ title: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                (e.target as HTMLTextAreaElement).blur();
              }
            }}
          />
        </div>

        <div>
          <div className="section-label">
            步骤 {task.steps.length > 0 ? `${doneSteps}/${task.steps.length}` : ""}
          </div>
          <div className="step-list">
            {task.steps.map((s) => (
              <div key={s.id} className={`step-row ${s.isCompleted ? "done" : ""}`}>
                <button
                  className={`check ${s.isCompleted ? "checked" : ""}`}
                  onClick={() => void saveStep(s.id, s.title, !s.isCompleted)}
                >
                  <IconCheck size={10} />
                </button>
                {editingStep === s.id ? (
                  <input
                    autoFocus
                    value={stepText}
                    onChange={(e) => setStepText(e.target.value)}
                    onBlur={() => void saveStep(s.id, stepText, s.isCompleted)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void saveStep(s.id, stepText, s.isCompleted);
                      if (e.key === "Escape") setEditingStep(null);
                    }}
                  />
                ) : (
                  <input
                    readOnly
                    aria-label={`编辑步骤：${s.title}`}
                    value={s.title}
                    onDoubleClick={() => {
                      setEditingStep(s.id);
                      setStepText(s.title);
                    }}
                    onClick={() => {
                      if (window.matchMedia("(pointer: coarse)").matches) {
                        setEditingStep(s.id);
                        setStepText(s.title);
                      }
                    }}
                  />
                )}
                <button className="step-del" onClick={() => void deleteStep(s.id)} title="删除步骤">
                  <IconX size={13} />
                </button>
              </div>
            ))}
            <div className="step-add">
              <IconPlus size={14} />
              <input
                value={stepTitle}
                placeholder="添加步骤"
                onChange={(e) => setStepTitle(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void addStep();
                }}
              />
            </div>
          </div>
        </div>

        <div className="detail-rows">
          <button className="detail-row" onClick={open("due")}>
            <span className="row-icon on"><IconCalendar size={16} /></span>
            <span className="row-label">{task.recurrence ? "初时间点" : "到期日期"}</span>
            <span className="row-value">
              <span>{draft.dueDate ? formatDueFull(draft.dueDate, draft.dueTime) : "无"}</span>
              <IconChevronDown size={13} />
            </span>
          </button>

          <div
            className={`detail-row reminder-row ${draft.reminderEnabled ? "" : "dim"}`}
            role="button"
            tabIndex={0}
            onClick={draft.reminderEnabled ? open("reminder") : undefined}
            onKeyDown={(e) => {
              if (draft.reminderEnabled && (e.key === "Enter" || e.key === " ")) {
                e.preventDefault();
                openAt("reminder", e.currentTarget);
              }
            }}
          >
            <span className={`row-icon ${draft.reminderEnabled ? "on" : ""}`}>
              <IconBell size={16} />
            </span>
            <span className="row-label">提醒</span>
            <span className="row-value">
              <span>{reminderLabel(draft.reminderEnabled ? draft.reminderOffset ?? 0 : null)}</span>
              <button
                className={`switch small ${draft.reminderEnabled ? "on" : ""}`}
                title={draft.reminderEnabled ? "关闭该任务的提醒" : "开启该任务的提醒"}
                aria-pressed={draft.reminderEnabled}
                onClick={(e) => {
                  e.stopPropagation();
                  toggleReminder();
                }}
              />
              {draft.reminderEnabled && <IconChevronDown size={13} />}
            </span>
          </div>

          <button className="detail-row" onClick={open("repeat")}>
            <span className={`row-icon ${draft.recurrence ? "on" : ""}`}>
              <IconRepeat size={16} />
            </span>
            <span className="row-label">重复</span>
            <span className="row-value">
              <span>{draft.recurrence ? recurrenceLabel(draft.recurrence) : "不重复"}</span>
              <IconChevronDown size={13} />
            </span>
          </button>

          <button className="detail-row" onClick={open("list")}>
            <span className="row-icon on"><IconList size={16} /></span>
            <span className="row-label">所属清单</span>
            <span className="row-value">
              <span>{lists.find((l) => l.id === draft.listId)?.name ?? "任务"}</span>
              <IconChevronDown size={13} />
            </span>
          </button>

          <button
            className="detail-row"
            onClick={() => onToggleImportant(task)}
          >
            <span className={`row-icon ${task.isImportant ? "star-on" : ""}`}>
              <IconStar size={16} filled={task.isImportant} />
            </span>
            <span className="row-label">标记为重要</span>
            <span className="row-value">
              <span>{task.isImportant ? "已标记" : "未标记"}</span>
            </span>
          </button>

          <button className="detail-row" onClick={() => onToggleMyDay(task)}>
            <span className={`row-icon ${task.myDay ? "on" : ""}`}>
              <IconSun size={16} />
            </span>
            <span className="row-label">我的一天</span>
            <span className="row-value">
              <span>{task.myDay ? "已加入今天" : "添加到我的一天"}</span>
            </span>
          </button>
        </div>

        <div>
          <div className="section-label">备注</div>
          <textarea
            className="note-area"
            placeholder="添加备注…（自动保存）"
            value={draft.note}
            onChange={(e) => update({ note: e.target.value })}
          />
        </div>

        {task.recurrence && (
          <div className="detail-recur-info">
            <div>下次日期：{formatFullDateTime(task.dueDate, task.dueTime)}</div>
            <div>已重复：{task.recurrenceCount}</div>
          </div>
        )}

        <div className="detail-footer">
          <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
            <button className="text-btn danger" onClick={() => onDelete(task, "single")}>
              <IconTrash size={14} />
              删除任务
            </button>
          </div>
          <span style={{ fontSize: 11, color: "var(--muted-2)" }}>
            {task.createdAt ? `创建于 ${formatTimestamp(task.createdAt)}` : ""}
          </span>
        </div>
      </div>

      {pop?.kind === "due" && (
        <Popover anchor={pop.rect} onClose={() => setPop(null)} width={300}>
          <CalendarPanel
            date={draft.dueDate}
            time={draft.dueTime}
            onConfirm={applyDue}
            onCancel={() => setPop(null)}
          />
        </Popover>
      )}

      {pop?.kind === "reminder" && (
        <Popover anchor={pop.rect} onClose={() => setPop(null)} width={240}>
          <ReminderPanel
            current={draft.reminderOffset}
            hasDue={Boolean(draft.dueDate)}
            onApply={applyReminder}
            onCancel={() => setPop(null)}
          />
        </Popover>
      )}

      {pop?.kind === "repeat" && (
        <Popover anchor={pop.rect} onClose={() => setPop(null)} width={280}>
          <RepeatPanel
            current={draft.recurrence}
            dueDate={draft.dueDate}
            onApply={setRecurrence}
          />
        </Popover>
      )}

      {pop?.kind === "list" && (
        <Popover anchor={pop.rect} onClose={() => setPop(null)} width={230}>
          {lists.map((l) => (
            <button
              key={l.id}
              className={`menu-item ${draft.listId === l.id ? "selected" : ""}`}
              onClick={() => {
                update({ listId: l.id });
                setPop(null);
              }}
            >
              <span className="dot" style={{ background: l.color }} />
              {l.name}
            </button>
          ))}
        </Popover>
      )}
    </aside>
  );
}

function clampInt(value: string, min: number, max: number, fallback: number): number {
  const n = Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Math.min(max, Math.max(min, Math.trunc(n)));
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}

function CalendarPanel({
  date,
  time,
  onConfirm,
  onCancel,
}: {
  date: string | null;
  time: string | null;
  onConfirm: (date: string, time: string) => void;
  onCancel: () => void;
}) {
  const base = date ?? todayString();
  const [year, setYear] = useState(Number(base.slice(0, 4)));
  const [month, setMonth] = useState(Number(base.slice(5, 7)) - 1);
  const [selected, setSelected] = useState<string | null>(date);
  const initialTime = time ?? "09:00";
  const [hour, setHour] = useState(initialTime.slice(0, 2));
  const [minute, setMinute] = useState(initialTime.slice(3, 5));

  const cells = monthGrid(year, month);
  const today = todayString();

  const shift = (delta: number) => {
    const m = month + delta;
    const d = new Date(year, m, 1);
    setYear(d.getFullYear());
    setMonth(d.getMonth());
  };

  const normalizeHour = () => setHour(pad2(clampInt(hour, 0, 23, 9)));
  const normalizeMinute = () => setMinute(pad2(clampInt(minute, 0, 59, 0)));

  const confirm = () => {
    if (!selected) return;
    const h = clampInt(hour, 0, 23, 9);
    const m = clampInt(minute, 0, 59, 0);
    onConfirm(selected, `${pad2(h)}:${pad2(m)}`);
  };

  return (
    <div className="calendar">
      <div className="calendar-head">
        <button className="icon-btn" onClick={() => shift(-1)}>
          <IconChevronLeft size={16} />
        </button>
        <span className="calendar-title">
          {year} 年 {month + 1} 月
        </span>
        <button className="icon-btn" onClick={() => shift(1)}>
          <IconChevronRight size={16} />
        </button>
      </div>
      <div className="calendar-grid">
        {WEEKDAY_LABELS.map((w) => (
          <div key={w} className="calendar-weekday">
            {w.slice(1)}
          </div>
        ))}
        {cells.map((c) => {
          const outside = Number(c.slice(5, 7)) - 1 !== month;
          return (
            <button
              key={c}
              className={`calendar-cell ${outside ? "outside" : ""} ${
                c === today ? "today" : ""
              } ${c === selected ? "selected" : ""}`}
              onClick={() => setSelected(c)}
            >
              {Number(c.slice(8, 10))}
            </button>
          );
        })}
      </div>
      <div className="time-row">
        <IconCalendar size={14} />
        <span className="time-label">时间</span>
        <div className="time-fields">
          <input
            className="time-input"
            inputMode="numeric"
            maxLength={2}
            value={hour}
            aria-label="小时"
            onChange={(e) => setHour(e.target.value.replace(/\D/g, "").slice(0, 2))}
            onBlur={normalizeHour}
            onKeyDown={(e) => {
              if (e.key === "Enter") confirm();
            }}
          />
          <span className="time-sep">:</span>
          <input
            className="time-input"
            inputMode="numeric"
            maxLength={2}
            value={minute}
            aria-label="分钟"
            onChange={(e) => setMinute(e.target.value.replace(/\D/g, "").slice(0, 2))}
            onBlur={normalizeMinute}
            onKeyDown={(e) => {
              if (e.key === "Enter") confirm();
            }}
          />
        </div>
      </div>
      <div className="calendar-quick">
        <button className="chip" onClick={() => setSelected(todayString())}>今天</button>
        <button className="chip" onClick={() => setSelected(addDays(todayString(), 1))}>明天</button>
      </div>
      <div className="calendar-actions">
        <button className="btn small" onClick={onCancel}>
          取消
        </button>
        <button className="btn primary small" onClick={confirm} disabled={!selected}>
          确认
        </button>
      </div>
    </div>
  );
}

const REMINDER_PRESETS: { label: string; value: number }[] = [
  { label: "准时提醒", value: 0 },
  { label: "提前 20 分钟", value: 20 },
  { label: "提前 1 小时", value: 60 },
  { label: "提前 1 天", value: 1440 },
];

function isPresetReminder(value: number | null): boolean {
  return REMINDER_PRESETS.some((p) => p.value === value);
}

function ReminderPanel({
  current,
  hasDue,
  onApply,
  onCancel,
}: {
  current: number | null;
  hasDue: boolean;
  onApply: (offset: number) => void;
  onCancel: () => void;
}) {
  const isCustom = current !== null && !isPresetReminder(current);
  const [customOpen, setCustomOpen] = useState(isCustom);
  const [pending, setPending] = useState<number>(current ?? 0);
  const init = splitReminderOffset(current ?? 0);
  const [days, setDays] = useState(String(init.days));
  const [hours, setHours] = useState(String(init.hours));
  const [minutes, setMinutes] = useState(String(init.minutes));
  const [error, setError] = useState("");

  const confirm = () => {
    if (!customOpen) {
      onApply(pending);
      return;
    }
    const d = clampInt(days, 0, 3650, 0);
    const h = clampInt(hours, 0, 23, 0);
    const m = clampInt(minutes, 0, 59, 0);
    const total = combineReminderOffset(d, h, m);
    if (total <= 0) {
      setError("请输入有效的提前时间");
      return;
    }
    setError("");
    onApply(total);
  };

  return (
    <div>
      {REMINDER_PRESETS.map((o) => (
        <button
          key={String(o.value)}
          className={`menu-item ${!customOpen && pending === o.value ? "selected" : ""}`}
          onClick={() => {
            setCustomOpen(false);
            setPending(o.value);
          }}
        >
          <span className="menu-icon">
            <IconBell size={14} />
          </span>
          {o.label}
        </button>
      ))}
      <button
        className={`menu-item ${customOpen ? "selected" : ""}`}
        onClick={() => setCustomOpen(true)}
      >
        <span className="menu-icon">
          <IconBell size={14} />
        </span>
        {isCustom && customOpen ? reminderLabel(current) : "自定义"}
      </button>

      {customOpen && (
        <div className="reminder-custom">
          <div className="num-row">
            <label>
              <input
                className="num-input"
                inputMode="numeric"
                maxLength={4}
                value={days}
                aria-label="提前天数"
                onChange={(e) => setDays(e.target.value.replace(/\D/g, "").slice(0, 4))}
                onBlur={() => setDays(String(clampInt(days, 0, 3650, 0)))}
              />
              <span>天</span>
            </label>
            <label>
              <input
                className="num-input"
                inputMode="numeric"
                maxLength={2}
                value={hours}
                aria-label="提前小时"
                onChange={(e) => setHours(e.target.value.replace(/\D/g, "").slice(0, 2))}
                onBlur={() => setHours(String(clampInt(hours, 0, 23, 0)))}
              />
              <span>小时</span>
            </label>
            <label>
              <input
                className="num-input"
                inputMode="numeric"
                maxLength={2}
                value={minutes}
                aria-label="提前分钟"
                onChange={(e) => setMinutes(e.target.value.replace(/\D/g, "").slice(0, 2))}
                onBlur={() => setMinutes(String(clampInt(minutes, 0, 59, 0)))}
              />
              <span>分钟</span>
            </label>
          </div>
          {error && <div className="form-error">{error}</div>}
        </div>
      )}

      <div className="panel-actions">
        <button className="btn small" onClick={onCancel}>
          取消
        </button>
        <button className="btn primary small" onClick={confirm}>
          确认
        </button>
      </div>

      {!hasDue && (
        <div className="panel-hint">未设置到期日期时，选择提醒会将日期设为今天 09:00</div>
      )}
    </div>
  );
}

function RepeatPanel({
  current,
  dueDate,
  onApply,
}: {
  current: RecurrenceInput | null;
  dueDate: string | null;
  onApply: (rec: RecurrenceInput | null) => void;
}) {
  const base = dueDate ?? todayString();
  const baseWeekday = weekdayIndex(base) + 1;
  const baseDay = Number(base.slice(8, 10));
  const [custom, setCustom] = useState(current?.kind === "custom");
  const [unit, setUnit] = useState<"day" | "week" | "month">(
    current?.kind === "weekly" ? "week" : current?.kind === "monthly" ? "month" : "day",
  );
  const [interval, setInterval] = useState(current?.interval ?? 1);
  const [weekdays, setWeekdays] = useState<number[]>(
    current?.weekdays
      ? current.weekdays.split(",").filter(Boolean).map(Number)
      : [baseWeekday],
  );
  const [monthDay, setMonthDay] = useState(current?.monthDay ?? baseDay);

  const presets: { label: string; value: RecurrenceInput | null; key: string }[] = [
    { label: "不重复", value: null, key: "none" },
    { label: "每天", value: { kind: "daily", interval: 1, weekdays: "", monthDay: null }, key: "daily" },
    { label: "每个工作日（周一至周五）", value: { kind: "weekdays", interval: 1, weekdays: "", monthDay: null }, key: "weekdays" },
    {
      label: `每周（${WEEKDAY_LABELS[baseWeekday - 1]}）`,
      value: { kind: "weekly", interval: 1, weekdays: String(baseWeekday), monthDay: null },
      key: "weekly",
    },
    {
      label: `每月（${baseDay} 日）`,
      value: { kind: "monthly", interval: 1, weekdays: "", monthDay: baseDay },
      key: "monthly",
    },
    { label: "每年", value: { kind: "yearly", interval: 1, weekdays: "", monthDay: null }, key: "yearly" },
  ];

  const isActive = (key: string) => {
    if (!current) return key === "none";
    if (key === "none") return false;
    return current.kind === key;
  };

  const applyCustom = () => {
    if (unit === "week") {
      onApply({
        kind: "weekly",
        interval: Math.max(1, interval),
        weekdays: weekdays.slice().sort().join(","),
        monthDay: null,
      });
    } else if (unit === "month") {
      onApply({
        kind: "monthly",
        interval: Math.max(1, interval),
        weekdays: "",
        monthDay: Number(monthDay),
      });
    } else {
      onApply({ kind: "daily", interval: Math.max(1, interval), weekdays: "", monthDay: null });
    }
  };

  return (
    <div>
      {presets.map((p) => (
        <button
          key={p.key}
          className={`menu-item ${isActive(p.key) && !custom ? "selected" : ""}`}
          onClick={() => {
            setCustom(false);
            onApply(p.value);
          }}
        >
          <span className="menu-icon"><IconRepeat size={14} /></span>
          {p.label}
        </button>
      ))}
      <div className="menu-sep" />
      <button
        className={`menu-item ${custom ? "selected" : ""}`}
        onClick={() => setCustom((c) => !c)}
      >
        <span className="menu-icon"><IconRepeat size={14} /></span>
        自定义重复…
      </button>

      {custom && (
        <div style={{ padding: "8px 8px 4px" }}>
          <div className="field">
            <label>重复单位</label>
            <select
              className="select"
              value={unit}
              onChange={(e) => setUnit(e.target.value as "day" | "week" | "month")}
            >
              <option value="day">天</option>
              <option value="week">周</option>
              <option value="month">月</option>
            </select>
          </div>
          <div className="field">
            <label>间隔</label>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <span style={{ fontSize: 13, color: "var(--muted)" }}>每</span>
              <input
                className="input"
                type="number"
                min={1}
                max={365}
                value={interval}
                onChange={(e) => setInterval(Number(e.target.value) || 1)}
                style={{ width: 80 }}
              />
              <span style={{ fontSize: 13, color: "var(--muted)" }}>
                {unit === "day" ? "天" : unit === "week" ? "周" : "个月"}
              </span>
            </div>
          </div>

          {unit === "week" && (
            <div className="field">
              <label>重复的星期</label>
              <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
                {WEEKDAY_LABELS.map((label, i) => {
                  const n = i + 1;
                  const on = weekdays.includes(n);
                  return (
                    <button
                      key={n}
                      className={`chip ${on ? "selected" : ""}`}
                      style={
                        on
                          ? { background: "var(--accent)", borderColor: "var(--accent)", color: "#fff" }
                          : undefined
                      }
                      onClick={() =>
                        setWeekdays((w) =>
                          w.includes(n) ? w.filter((x) => x !== n) : [...w, n],
                        )
                      }
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          {unit === "month" && (
            <div className="field">
              <label>每月的日期</label>
              <select
                className="select"
                value={monthDay}
                onChange={(e) => setMonthDay(Number(e.target.value))}
              >
                {Array.from({ length: 31 }, (_, i) => i + 1).map((d) => (
                  <option key={d} value={d}>
                    {d} 日
                  </option>
                ))}
              </select>
              <span style={{ fontSize: 11, color: "var(--muted-2)", lineHeight: 1.5 }}>
                遇到没有该日期的月份时，将自动使用当月最后一天（如 31 日 → 2 月 28/29 日）。
              </span>
            </div>
          )}

          <button className="btn primary small" style={{ width: "100%", marginTop: 4 }} onClick={applyCustom}>
            应用自定义重复
          </button>
        </div>
      )}
    </div>
  );
}

export function addStepIcon() {
  return <IconPlus size={14} />;
}
