import { useState } from "react";
import { api, type TaskList } from "../api";
import { Modal, ConfirmModal } from "./Modal";
import { IconTrash } from "./Icons";

const COLORS = ["#2f6fed", "#00a884", "#e0a03a", "#e05252", "#8a5cf6", "#0ea5e9", "#ec4899", "#6b7280"];

interface Props {
  list?: TaskList;
  lists: TaskList[];
  onClose: () => void;
  onDone: (message: string) => void;
  notify: (msg: string, error?: boolean) => void;
}

export function ListModal({ list, lists, onClose, onDone, notify }: Props) {
  const [name, setName] = useState(list?.name ?? "");
  const [color, setColor] = useState(list?.color ?? COLORS[0]);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    if (!name.trim()) return;
    setBusy(true);
    try {
      if (list) {
        await api.updateList(list.id, name.trim(), color, list.icon);
        onDone("清单已更新");
      } else {
        await api.createList(name.trim(), color);
        onDone("清单已创建");
      }
      onClose();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  const remove = async (mode: "move" | "delete") => {
    if (!list) return;
    setBusy(true);
    try {
      const others = lists.filter((l) => l.id !== list.id);
      await api.deleteList(list.id, mode, others[0]?.id ?? null);
      onDone(mode === "move" ? "清单已删除，任务已移动" : "清单及其任务已删除");
      onClose();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Modal title={list ? "管理清单" : "新建清单"} onClose={onClose}>
        <div className="field">
          <label>清单名称</label>
          <input
            className="input"
            autoFocus
            value={name}
            placeholder="例如：工作、学习、购物"
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void save();
            }}
          />
        </div>
        <div className="field">
          <label>颜色</label>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            {COLORS.map((c) => (
              <button
                key={c}
                onClick={() => setColor(c)}
                style={{
                  width: 26,
                  height: 26,
                  borderRadius: "50%",
                  background: c,
                  outline: color === c ? "2px solid var(--text)" : "none",
                  outlineOffset: 2,
                }}
                title={c}
              />
            ))}
          </div>
        </div>
        <div className="modal-actions">
          {list && (
            <button
              className="btn danger"
              style={{ marginRight: "auto" }}
              onClick={() => setConfirmDelete(true)}
              disabled={busy || lists.length <= 1}
            >
              <IconTrash size={14} />
              删除清单
            </button>
          )}
          <button className="btn" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="btn primary" onClick={() => void save()} disabled={busy || !name.trim()}>
            保存
          </button>
        </div>
      </Modal>

      {confirmDelete && list && (
        <ConfirmModal
          title="删除清单"
          message={`即将删除清单「${list.name}」。请选择其中任务的去向：\n\n· 移动任务：把任务移到其他清单，任务不会丢失\n· 删除任务：任务进入「最近删除」，可稍后恢复`}
          onClose={() => setConfirmDelete(false)}
          actions={[
            { label: "取消", kind: "default", onClick: () => setConfirmDelete(false) },
            { label: "移动任务到其他清单", kind: "primary", onClick: () => remove("move") },
            { label: "删除清单及任务", kind: "danger", onClick: () => remove("delete") },
          ]}
        />
      )}
    </>
  );
}
