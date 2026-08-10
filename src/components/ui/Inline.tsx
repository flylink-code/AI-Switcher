import React from "react";

export interface InlineProps {
  children: React.ReactNode;
  className?: string;
  style?: React.CSSProperties;
  gap?: "none" | "xs" | "sm" | "md" | "lg" | "xl";
  align?: React.CSSProperties["alignItems"];
  justify?: React.CSSProperties["justifyContent"];
  wrap?: boolean;
}

const gapMap: Record<NonNullable<InlineProps["gap"]>, string> = {
  none: "0px",
  xs: "var(--space-1)",
  sm: "var(--space-2)",
  md: "var(--space-4)",
  lg: "var(--space-5)",
  xl: "var(--space-6)",
};

export const Inline: React.FC<InlineProps> = ({
  children,
  className = "",
  style,
  gap = "sm",
  align = "center",
  justify = "flex-start",
  wrap = false,
}) => {
  const combinedStyle: React.CSSProperties = {
    display: "flex",
    flexDirection: "row",
    gap: gapMap[gap],
    alignItems: align,
    justifyContent: justify,
    flexWrap: wrap ? "wrap" : "nowrap",
    ...style,
  };

  return (
    <div className={className} style={combinedStyle}>
      {children}
    </div>
  );
};
