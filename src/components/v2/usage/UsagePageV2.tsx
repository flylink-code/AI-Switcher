import React from "react";
import UsagePage from "@/pages/UsagePage";

/** V2 usage shell — embeds the existing UsagePage analytics engine. */
export const UsagePageV2: React.FC = () => {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px", minHeight: "500px" }}>
      <UsagePage />
    </div>
  );
};
