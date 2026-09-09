import React from "react";

export interface UsagePageV2Props {
  /**
   * The lazily loaded Usage page. Passing it in keeps `UsagePage` (and its
   * recharts dependency) inside its own bundle chunk instead of the shell.
   */
  children: React.ReactNode;
}

/** V2 usage shell — embeds the existing UsagePage analytics engine. */
export const UsagePageV2: React.FC<UsagePageV2Props> = ({ children }) => {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px", minHeight: "500px" }}>
      {children}
    </div>
  );
};
