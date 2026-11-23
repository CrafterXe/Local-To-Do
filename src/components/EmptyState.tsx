import { IconCheckCircle } from "./Icons";

export function EmptyState({ text }: { text: string }) {
  return (
    <div className="empty-state">
      <span className="big-icon">
        <IconCheckCircle size={46} />
      </span>
      <span style={{ maxWidth: 320, lineHeight: 1.7, fontSize: 13 }}>{text}</span>
    </div>
  );
}
