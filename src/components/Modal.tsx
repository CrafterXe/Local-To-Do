import { useEffect, useState, type ReactNode } from "react";

export function Modal({
  title,
  children,
  onClose,
  width,
}: {
  title: string;
  children: ReactNode;
  onClose?: () => void;
  width?: number;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="modal-overlay" onMouseDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div className="modal" style={width ? { width } : undefined}>
        <h3>{title}</h3>
        {children}
      </div>
    </div>
  );
}

export interface ConfirmAction {
  label: string;
  kind?: "primary" | "danger" | "default";
  onClick: () => void | Promise<void>;
}

export function ConfirmModal({
  title,
  message,
  actions,
  onClose,
}: {
  title: string;
  message: string;
  actions: ConfirmAction[];
  onClose: () => void;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={title} onClose={onClose}>
      <p>{message}</p>
      <div className="modal-actions">
        <button className="btn" onClick={onClose} disabled={busy}>
          取消
        </button>
        {actions.map((a) => (
          <button
            key={a.label}
            className={`btn ${a.kind === "danger" ? "danger" : a.kind === "primary" ? "primary" : ""}`}
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await a.onClick();
              } finally {
                setBusy(false);
              }
            }}
          >
            {a.label}
          </button>
        ))}
      </div>
    </Modal>
  );
}

export function PromptModal({
  title,
  initialValue = "",
  placeholder,
  confirmLabel = "确定",
  onSubmit,
  onClose,
  extra,
}: {
  title: string;
  initialValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  onSubmit: (value: string) => void | Promise<void>;
  onClose: () => void;
  extra?: ReactNode;
}) {
  const [value, setValue] = useState(initialValue);
  const [busy, setBusy] = useState(false);
  return (
    <Modal title={title} onClose={onClose}>
      <div className="field">
        <input
          className="input"
          autoFocus
          value={value}
          placeholder={placeholder}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && value.trim()) {
              void onSubmit(value.trim());
            }
          }}
        />
      </div>
      {extra}
      <div className="modal-actions">
        <button className="btn" onClick={onClose} disabled={busy}>
          取消
        </button>
        <button
          className="btn primary"
          disabled={busy || !value.trim()}
          onClick={async () => {
            setBusy(true);
            try {
              await onSubmit(value.trim());
            } finally {
              setBusy(false);
            }
          }}
        >
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
