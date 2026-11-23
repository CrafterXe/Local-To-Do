import { useEffect, useRef, useState } from "react";
import type { Note } from "../api";
import { formatRelative, formatTimestamp } from "../utils/date";
import { EmptyState } from "./EmptyState";
import { IconNote, IconPlus, IconTrash } from "./Icons";

interface Props {
  notes: Note[];
  selectedId: string | null;
  searchActive: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void | Promise<void>;
  onSave: (id: string, title: string, body: string) => Promise<void>;
  onDelete: (note: Note) => void;
  notify: (msg: string, error?: boolean) => void;
}

export function NotesView({
  notes,
  selectedId,
  searchActive,
  onSelect,
  onCreate,
  onSave,
  onDelete,
  notify,
}: Props) {
  const note = notes.find((n) => n.id === selectedId) ?? null;
  const [title, setTitle] = useState(note?.title ?? "");
  const [body, setBody] = useState(note?.body ?? "");
  const savedRef = useRef("");
  const timerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    setTitle(note?.title ?? "");
    setBody(note?.body ?? "");
    savedRef.current = JSON.stringify([note?.title ?? "", note?.body ?? ""]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [note?.id]);

  useEffect(() => {
    if (!note) return;
    const serialized = JSON.stringify([title, body]);
    if (serialized === savedRef.current) return;
    window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(async () => {
      try {
        await onSave(note.id, title, body);
        savedRef.current = serialized;
      } catch (e) {
        notify(`笔记保存失败：${String(e)}`, true);
      }
    }, 500);
    return () => window.clearTimeout(timerRef.current);
  }, [title, body, note, onSave, notify]);

  return (
    <div className="notes-wrap">
      <div className="notes-list">
        <button className="quick-add" style={{ margin: "4px 4px 10px" }} onClick={() => void onCreate()}>
          <span className="plus-wrap"><IconPlus size={16} /></span>
          <span style={{ flex: 1, textAlign: "left", color: "var(--muted)", fontSize: 13 }}>
            新建笔记
          </span>
        </button>
        {notes.length === 0 ? (
          <EmptyState text={searchActive ? "没有找到匹配的笔记" : "还没有笔记，点击上方新建"} />
        ) : (
          notes.map((n) => (
            <button
              key={n.id}
              className={`note-item ${n.id === selectedId ? "active" : ""}`}
              onClick={() => onSelect(n.id)}
            >
              <span className="note-title">{n.title.trim() || "无标题笔记"}</span>
              <span className="note-preview">{n.body.replace(/\s+/g, " ").slice(0, 60) || "空白内容"}</span>
              <span className="note-time">{formatRelative(n.updatedAt)}</span>
            </button>
          ))
        )}
      </div>

      {note ? (
        <div className="note-editor">
          <input
            className="note-title-input"
            placeholder="笔记标题"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
          />
          <div className="note-meta">
            <span>创建：{formatTimestamp(note.createdAt)}</span>
            <span>修改：{formatTimestamp(note.updatedAt)}</span>
            <span style={{ marginLeft: "auto", color: "var(--muted-2)" }}>自动保存</span>
          </div>
          <textarea
            className="note-body-input"
            placeholder="开始输入…支持多行纯文本 / Markdown"
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
          <div style={{ display: "flex", justifyContent: "flex-end" }}>
            <button className="text-btn danger" onClick={() => onDelete(note)}>
              <IconTrash size={14} />
              删除笔记
            </button>
          </div>
        </div>
      ) : (
        <div className="note-editor" style={{ alignItems: "center", justifyContent: "center" }}>
          <span style={{ color: "var(--muted-2)", display: "flex", flexDirection: "column", alignItems: "center", gap: 10 }}>
            <IconNote size={42} />
            选择左侧笔记，或新建一条
          </span>
        </div>
      )}
    </div>
  );
}
