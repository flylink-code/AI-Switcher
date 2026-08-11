import React from "react";

export interface SettingsSectionProps {
  title: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

/** A titled group of settings rows wrapped in a rounded Desktop Group Surface. */
export const SettingsSection: React.FC<SettingsSectionProps> = ({
  title,
  children,
  className = "",
  style,
}) => {
  return (
    <section className={className} style={{ marginBottom: "24px", ...style }}>
      <h3
        style={{
          margin: "0 0 10px 4px",
          fontSize: "15px",
          fontWeight: 600,
          color: "var(--color-text-primary)",
          letterSpacing: "-0.1px",
        }}
      >
        {title}
      </h3>
      <div
        className="settings-group-surface"
        style={{
          backgroundColor: "var(--color-bg-surface, var(--color-bg-container))",
          border: "1px solid var(--color-border-subtle, var(--color-border))",
          borderRadius: "8px",
          overflow: "hidden",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {children}
      </div>
    </section>
  );
};
