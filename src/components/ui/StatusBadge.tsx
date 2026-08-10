import React from "react";

export type StatusType =
  | "running"
  | "stopped"
  | "healthy"
  | "slow"
  | "warning"
  | "error"
  | "active"
  | "current"
  | "default";

export interface StatusBadgeProps {
  status: StatusType;
  label?: string;
  className?: string;
  style?: React.CSSProperties;
  showDot?: boolean;
}

const statusConfig: Record<
  StatusType,
  { bg: string; color: string; dot: string; defaultLabel: string }
> = {
  running: {
    bg: "var(--color-success-subtle)",
    color: "var(--color-success)",
    dot: "var(--color-success)",
    defaultLabel: "Running",
  },
  healthy: {
    bg: "var(--color-success-subtle)",
    color: "var(--color-success)",
    dot: "var(--color-success)",
    defaultLabel: "Healthy",
  },
  active: {
    bg: "var(--color-brand-subtle)",
    color: "var(--color-brand)",
    dot: "var(--color-brand)",
    defaultLabel: "Active",
  },
  current: {
    bg: "var(--color-brand-subtle)",
    color: "var(--color-brand)",
    dot: "var(--color-brand)",
    defaultLabel: "Current",
  },
  slow: {
    bg: "var(--color-warning-subtle)",
    color: "var(--color-warning)",
    dot: "var(--color-warning)",
    defaultLabel: "Slow",
  },
  warning: {
    bg: "var(--color-warning-subtle)",
    color: "var(--color-warning)",
    dot: "var(--color-warning)",
    defaultLabel: "Warning",
  },
  error: {
    bg: "var(--color-danger-subtle)",
    color: "var(--color-danger)",
    dot: "var(--color-danger)",
    defaultLabel: "Error",
  },
  stopped: {
    bg: "var(--color-bg-subtle)",
    color: "var(--color-text-secondary)",
    dot: "var(--color-text-tertiary)",
    defaultLabel: "Stopped",
  },
  default: {
    bg: "var(--color-bg-subtle)",
    color: "var(--color-text-secondary)",
    dot: "var(--color-text-tertiary)",
    defaultLabel: "Default",
  },
};

export const StatusBadge: React.FC<StatusBadgeProps> = ({
  status,
  label,
  className = "",
  style,
  showDot = true,
}) => {
  const config = statusConfig[status] || statusConfig.default;
  const displayLabel = label ?? config.defaultLabel;

  const combinedStyle: React.CSSProperties = {
    backgroundColor: config.bg,
    color: config.color,
    ...style,
  };

  return (
    <span
      className={`ui-badge ${className}`.trim()}
      style={combinedStyle}
      aria-label={`Status: ${displayLabel}`}
    >
      {showDot && (
        <span className="ui-status-dot" style={{ backgroundColor: config.dot }} aria-hidden="true" />
      )}
      <span>{displayLabel}</span>
    </span>
  );
};
