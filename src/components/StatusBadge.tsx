import { useT } from "../i18n";
import type { ClaudeStatus } from "../types";

export function StatusDot({
  status,
  unread,
}: {
  status: ClaudeStatus | undefined;
  unread?: boolean;
}) {
  const t = useT();
  const s = status ?? "none";
  return (
    <span
      className={`dot dot--${s} ${unread ? "dot--unread" : ""}`}
      title={status ? t(`status.${status}`) : t("status.notStarted")}
    />
  );
}

export function StatusBadge({ status }: { status: ClaudeStatus | undefined }) {
  const t = useT();
  if (!status) return null;
  return <span className={`badge badge--${status}`}>{t(`status.${status}`)}</span>;
}
