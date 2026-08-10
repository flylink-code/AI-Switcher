import React from "react";

export interface MetricProps {
  label: React.ReactNode;
  value: React.ReactNode;
  supporting?: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
}

export const Metric: React.FC<MetricProps> = ({
  label,
  value,
  supporting,
  className = "",
  style,
}) => {
  return (
    <div className={className} style={{ display: "flex", flexDirection: "column", gap: "var(--space-1)", ...style }}>
      <div style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-secondary)", fontWeight: "var(--font-weight-medium)" }}>
        {label}
      </div>
      <div style={{ fontSize: "var(--font-size-2xl)", color: "var(--color-text-primary)", fontWeight: "var(--font-weight-semibold)", lineHeight: 1.2 }}>
        {value}
      </div>
      {supporting && (
        <div style={{ fontSize: "var(--font-size-xs)", color: "var(--color-text-tertiary)", marginTop: "2px" }}>
          {supporting}
        </div>
      )}
    </div>
  );
};
