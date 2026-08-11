import React from "react";

export interface ContextHeaderProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  extra?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

export const ContextHeader: React.FC<ContextHeaderProps> = ({
  title,
  description,
  extra,
  className = "",
  style,
}) => {
  return (
    <header
      className={`app-context-header ${className}`.trim()}
      style={{
        padding: "var(--space-4) var(--page-padding-x)",
        backgroundColor: "var(--color-bg-surface)",
        borderBottom: "1px solid var(--color-border)",
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-4)",
        minHeight: "var(--app-header-height)",
        flexShrink: 0,
        boxSizing: "border-box",
        ...style,
      }}
    >
      <div style={{ display: "flex", flexDirection: "column", gap: "2px", minWidth: 0, flex: 1 }}>
        <div style={{ fontSize: "var(--font-size-xl)", fontWeight: "var(--font-weight-bold)", color: "var(--color-text-primary)", lineHeight: 1.2 }}>
          {title}
        </div>
        {description && (
          <div style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
            {description}
          </div>
        )}
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-3)", flexShrink: 0 }}>
        {extra}
      </div>
    </header>
  );
};
