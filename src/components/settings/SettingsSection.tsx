import React from "react";

export interface SettingsSectionProps {
  title: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

/** A titled group of settings rows separated by a heading + divider. */
export const SettingsSection: React.FC<SettingsSectionProps> = ({
  title,
  children,
  className = "",
  style,
}) => {
  return (
    <section className={className} style={style}>
      <h3
        style={{
          margin: "0 0 var(--space-2)",
          fontSize: "var(--font-size-sm)",
          fontWeight: "var(--font-weight-semibold)",
          color: "var(--color-text-secondary)",
        }}
      >
        {title}
      </h3>
      <div
        style={{
          borderTop: "1px solid var(--color-border-subtle)",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {children}
      </div>
    </section>
  );
};
