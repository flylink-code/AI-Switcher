import React from "react";
import { Empty } from "antd";

export interface ResourceEmptyStateProps {
  /** What the resource is (short title). */
  title: React.ReactNode;
  /** Why the user may want it. */
  description?: React.ReactNode;
  /** Primary next action, e.g. an "Add" button. */
  action?: React.ReactNode;
  /** Secondary action(s), e.g. import. */
  extra?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

/** Intentional empty state for workspace resource pages. */
export const ResourceEmptyState: React.FC<ResourceEmptyStateProps> = ({
  title,
  description,
  action,
  extra,
  className = "",
  style,
}) => {
  return (
    <div
      className={className}
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: "var(--space-3)",
        padding: "var(--space-8) var(--space-4)",
        textAlign: "center",
        ...style,
      }}
    >
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description={
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <span style={{ fontSize: "var(--font-size-md)", color: "var(--color-text-primary)" }}>
              {title}
            </span>
            {description && (
              <span style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)", maxWidth: 420 }}>
                {description}
              </span>
            )}
          </div>
        }
      />
      {(action || extra) && (
        <div style={{ display: "flex", alignItems: "center", gap: "var(--space-3)", flexWrap: "wrap", justifyContent: "center" }}>
          {action}
          {extra}
        </div>
      )}
    </div>
  );
};
