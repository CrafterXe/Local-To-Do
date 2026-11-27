import { useCallback, useEffect, useRef, useState } from "react";

import { api, type Note } from "../api";
import { IconCheck, IconTrash, IconX } from "../components/Icons";
import { formatTimestamp } from "../utils/date";

interface Props {
  note: Note;
  onClose: () => void;
  onSaved: (note: Note) => void;
  onDelete: (note: Note) => void;
  notify: (message: string, error?: boolean) => void;
}

export function MobileNoteDetail({ note, onClose, onSaved, onDelete, notify }: Props) {
  const [title, setTitle] = useState(note.title);
  const [body, setBody] = useState(note.body);
  const [saving, setSaving] = useState(false);
  const savedRef = useRef(JSON.stringify({ title: note.title, body: note.body }));
  const latestRef = useRef({ title, body });
  latestRef.current = { title, body };

  useEffect(() => {
    setTitle(note.title);
    setBody(note.body);
    savedRef.current = JSON.stringify({ title: note.title, body: note.body });
  }, [note.id, note.title, note.body]);

  const isDirty = useCallback(() => {
    return JSON.stringify(latestRef.current) !== savedRef.current;
  }, []);

  const save = useCallback(async () => {
    const draft = latestRef.current;
    const serialized = JSON.stringify(draft);
    if (serialized === savedRef.current) return;
    setSaving(true);
    try {
      const updated = await api.updateNote(note.id, draft.title, draft.body);
      savedRef.current = serialized;
      onSaved(updated);
      notify("已保存");
    } catch (error) {
      notify(`保存备忘录失败：${String(error)}`, true);
    } finally {
      setSaving(false);
    }
  }, [note.id, notify, onSaved]);

  // 自动保存（debounce 500ms）
  useEffect(() => {
    const timer = window.setTimeout(() => void save(), 500);
    return () => window.clearTimeout(timer);
  }, [title, body, save]);

  const handleClose = useCallback(() => {
    if (isDirty()) {
      void save().then(onClose);
    } else {
      onClose();
    }
  }, [isDirty, save, onClose]);

  return (
    <section className="offline-note-detail">
      <header className="offline-note-header">
        <span>{saving ? "正在保存…" : isDirty() ? "备忘录 · 未保存" : "备忘录 · 已保存"}</span>
        <div style={{ display: "flex", gap: 4 }}>
          {isDirty() && !saving ? (
            <button className="offline-icon" onClick={() => void save()} aria-label="保存备忘录" title="保存">
              <IconCheck size={19} />
            </button>
          ) : null}
          <button className="offline-icon" onClick={handleClose} aria-label="关闭备忘录">
            <IconX size={19} />
          </button>
        </div>
      </header>
      <div className="offline-note-editor">
        <input
          className="offline-note-title-input"
          value={title}
          placeholder="标题"
          aria-label="备忘录标题"
          onChange={(event) => setTitle(event.target.value)}
        />
        <textarea
          className="offline-note-body-input"
          value={body}
          placeholder="记录一些内容…"
          aria-label="备忘录内容"
          onChange={(event) => setBody(event.target.value)}
        />
        <div className="offline-note-footer">
          <span>更新于 {formatTimestamp(note.updatedAt)}</span>
          <button
            className="text-btn danger"
            onClick={() => onDelete({ ...note, title, body })}
          >
            <IconTrash size={15} /> 删除
          </button>
        </div>
      </div>
    </section>
  );
}
