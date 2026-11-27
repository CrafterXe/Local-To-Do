import { useCallback, useEffect, useRef, useState } from "react";

import { api, type Note } from "../api";
import { IconTrash, IconX } from "../components/Icons";
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

  const save = useCallback(async () => {
    const draft = latestRef.current;
    const serialized = JSON.stringify(draft);
    if (serialized === savedRef.current) return;
    setSaving(true);
    try {
      const updated = await api.updateNote(note.id, draft.title, draft.body);
      savedRef.current = serialized;
      onSaved(updated);
    } catch (error) {
      notify(`保存备忘录失败：${String(error)}`, true);
    } finally {
      setSaving(false);
    }
  }, [note.id, notify, onSaved]);

  useEffect(() => {
    const timer = window.setTimeout(() => void save(), 500);
    return () => window.clearTimeout(timer);
  }, [title, body, save]);

  return (
    <section className="offline-note-detail">
      <header className="offline-note-header">
        <span>{saving ? "正在保存…" : "备忘录 · 自动保存"}</span>
        <button className="offline-icon" onClick={() => void save().then(onClose)} aria-label="关闭备忘录">
          <IconX size={19} />
        </button>
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
