import React from "react";
import { Button } from "antd";
import ArrowLeftOutlined from "@ant-design/icons/es/icons/ArrowLeftOutlined";

export interface ContextHeaderProps {
  title: React.ReactNode;
  description?: React.ReactNode;
  showBack?: boolean;
  onBack?: () => void;
  backText?: React.ReactNode;
  extra?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

export const ContextHeader: React.FC<ContextHeaderProps> = ({
  title,
  description,
  showBack,
  onBack,
  backText,
  extra,
  className = "",
  style,
}) => {
  return (
    <header
      className={`app-context-header ${className}`.trim()}
      style={{
        padding: "8px var(--page-padding-x, 16px)",
        backgroundColor: "transparent",
        borderBottom: "1px solid var(--color-border-subtle, rgba(0,0,0,0.06))",
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: "var(--space-4)",
        minHeight: "40px",
        flexShrink: 0,
        boxSizing: "border-box",
        ...style,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: "12px", minWidth: 0, flex: 1 }}>
        {showBack && (
          <Button
            type="text"
            size="small"
            icon={<ArrowLeftOutlined />}
            onClick={onBack}
            style={{
              padding: "0 8px",
              height: "28px",
              borderRadius: "6px",
              color: "var(--color-text-secondary)",
              fontWeight: 500,
            }}
          >
            {backText}
          </Button>
        )}
        <div style={{ display: "flex", flexDirection: "column", gap: "1px", minWidth: 0, flex: 1 }}>
          <div style={{ fontSize: "var(--font-size-md, 15px)", fontWeight: "var(--font-weight-semibold, 600)", color: "var(--color-text-primary)", lineHeight: 1.3 }}>
            {title}
          </div>
          {description && (
            <div style={{ fontSize: "var(--font-size-xs, 12px)", color: "var(--color-text-secondary)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
              {description}
            </div>
          )}
        </div>
      </div>

      <div style={{ display: "flex", alignItems: "center", gap: "var(--space-3)", flexShrink: 0 }}>
        {extra}
      </div>
    </header>
  );
};
