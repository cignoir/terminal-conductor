import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes } from "react";

type Props = Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title"> & {
  icon: LucideIcon;
  /** Shown as the tooltip and read by screen readers. */
  label: string;
  /** Shortcut shown after the label in the tooltip, e.g. "Ctrl+K". */
  shortcut?: string;
  variant?: "default" | "primary" | "danger";
  /** Rendered as pressed (for toggles). */
  active?: boolean;
  size?: number;
  spin?: boolean;
};

/** An icon-only button; its label appears as a tooltip (see Tooltip). */
export function IconButton({
  icon: Icon,
  label,
  shortcut,
  variant = "default",
  active,
  size = 16,
  spin,
  className,
  ...rest
}: Props) {
  return (
    <button
      type="button"
      {...rest}
      aria-label={label}
      aria-pressed={active === undefined ? undefined : active}
      data-tip={shortcut ? `${label}  ${shortcut}` : label}
      className={[
        "icon-btn",
        variant !== "default" ? `icon-btn--${variant}` : "",
        active ? "icon-btn--active" : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <Icon size={size} strokeWidth={1.8} className={spin ? "spin" : undefined} aria-hidden />
    </button>
  );
}
