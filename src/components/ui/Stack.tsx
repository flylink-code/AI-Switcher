import React from "react";

export interface StackProps {
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
  gap?: "none" | "xs" | "sm" | "md" | "lg" | "xl";
  align?: React.CSSProperties["alignItems"];
  justify?: React.CSSProperties["justifyContent"];
}

const gapMap: Record<NonNullable<StackProps["gap"]>, string> = {
  none: "0px",
  xs: "var(--space-1)",
  sm: "var(--space-2)",
  md: "var(--space-4)",
  lg: "var(--space-5)",
  xl: "var(--space-6)",
};

export const Stack: React.FC<StackProps> = ({
  children,
  className = "",
  style,
  gap = "md",
  align = "stretch",
  justify = "flex-start",
}) => {
  const combinedStyle: React.CSSProperties = {
    display: "flex",
    flexDirection: "column",
    gap: gapMap[gap],
    alignItems: align,
    justifyContent: justify,
    ...style,
  };

  return (
    <div className={className} style={combinedStyle}>
      {children}
    </div>
  );
};
