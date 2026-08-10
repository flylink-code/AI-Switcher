import React from "react";

export interface SurfaceProps {
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
  variant?: "default" | "subtle" | "elevated";
  padding?: "none" | "sm" | "md" | "lg";
}

const paddingMap: Record<NonNullable<SurfaceProps["padding"]>, string> = {
  none: "0px",
  sm: "var(--space-2)",
  md: "var(--space-4)",
  lg: "var(--space-6)",
};

export const Surface: React.FC<SurfaceProps> = ({
  children,
  className = "",
  style,
  variant = "default",
  padding = "md",
}) => {
  const variantClass =
    variant === "subtle"
      ? "ui-surface--subtle"
      : variant === "elevated"
        ? "ui-surface--elevated"
        : "";

  const combinedStyle: React.CSSProperties = {
    padding: paddingMap[padding],
    ...style,
  };

  return (
    <div className={`ui-surface ${variantClass} ${className}`.trim()} style={combinedStyle}>
      {children}
    </div>
  );
};
