import { useEffect, useRef, type ReactNode } from "react";

export function Popover({
  anchor,
  onClose,
  width = 240,
  align = "left",
  children,
}: {
  anchor: DOMRect;
  onClose: () => void;
  width?: number;
  align?: "left" | "right";
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const timer = window.setTimeout(() => {
      document.addEventListener("mousedown", onDown);
    }, 0);
    document.addEventListener("keydown", onKey);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const left =
    align === "right"
      ? Math.max(12, anchor.right - width)
      : Math.min(anchor.left, window.innerWidth - width - 12);
  const top = Math.min(anchor.bottom + 6, Math.max(12, window.innerHeight - 72));

  return (
    <div
      ref={ref}
      className="popover"
      style={{ position: "fixed", top, left, width, maxHeight: "72vh", overflowY: "auto" }}
    >
      {children}
    </div>
  );
}
