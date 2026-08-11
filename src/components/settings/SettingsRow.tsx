import React from "react";
import RightOutlined from "@ant-design/icons/es/icons/RightOutlined";

export interface SettingsRowProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  /** Right-side control (Select, Switch, ...) or value text. */
  control?: React.ReactNode;
  /** When set, the row behaves as a navigation link to a detail view. */
  onClick?: () => void;
  className?: string;
  style?: React.CSSProperties;
}

/** Single settings row: label + description on the left, control/chevron right. */
export const SettingsRow: React.FC<SettingsRowProps> = ({
  title,
  description,
  control,
  onClick,
  className = "",
  style,
}) => {
  const interactive = Boolean(onClick);

  const inner = (
    <>
      <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 2 }}>
        <span style={{ fontSize: "var(--font-size-md)", color: "var(--color-text-primary)" }}>
          {title}
        </span>
        {description && (
          <span style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-tertiary)" }}>
            {description}
          </span>
        )}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)", flexShrink: 0 }}>
        {control}
        {interactive && (
          <RightOutlined style={{ fontSize: 12, color: "var(--color-text-tertiary)" }} />
        )}
      </div>
    </>
  );

  const baseStyle: React.CSSProperties = {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "var(--space-4)",
    width: "100%",
    padding: "12px 4px",
    borderBottom: "1px solid var(--color-border-subtle)",
    background: "none",
    textAlign: "left",
    ...style,
  };

  if (interactive) {
    return (
      <button
        type="button"
        className={className}
        onClick={onClick}
        style={{ ...baseStyle, border: "none", borderBottom: "1px solid var(--color-border-subtle)", cursor: "pointer" }}
        onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = "var(--color-bg-surface)"; }}
        onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = "transparent"; }}
      >
        {inner}
      </button>
    );
  }

  return (
    <div className={className} style={baseStyle}>
      {inner}
    </div>
  );
};
