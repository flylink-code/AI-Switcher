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
        <span style={{ fontSize: "14px", fontWeight: 500, color: "var(--color-text-primary)" }}>
          {title}
        </span>
        {description && (
          <span style={{ fontSize: "12.5px", color: "var(--color-text-secondary)", lineHeight: 1.4 }}>
            {description}
          </span>
        )}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: "12px", flexShrink: 0 }}>
        {control}
        {interactive && (
          <RightOutlined style={{ fontSize: 13, color: "var(--color-text-tertiary)" }} />
        )}
      </div>
    </>
  );

  const baseStyle: React.CSSProperties = {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: "16px",
    width: "100%",
    minHeight: "64px",
    padding: "14px 16px",
    borderBottom: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
    background: "none",
    textAlign: "left",
    boxSizing: "border-box",
    transition: "background-color 0.15s ease",
    ...style,
  };

  if (interactive) {
    return (
      <button
        type="button"
        className={`settings-row-interactive ${className}`.trim()}
        onClick={onClick}
        style={{
          ...baseStyle,
          border: "none",
          borderBottom: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
          cursor: "pointer",
        }}
        onMouseEnter={(e) => {
          e.currentTarget.style.backgroundColor = "var(--color-bg-subtle, rgba(0,0,0,0.025))";
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.backgroundColor = "transparent";
        }}
      >
        {inner}
      </button>
    );
  }

  return (
    <div className={`settings-row ${className}`.trim()} style={baseStyle}>
      {inner}
    </div>
  );
};
