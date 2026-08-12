import React from "react";
import { CliStatusCard } from "@/components/pi/CliStatusCard";
import { ProviderCard } from "@/components/pi/ProviderCard";
import { WorkspaceEditor } from "@/components/pi/WorkspaceEditor";
import { SessionManager } from "@/components/pi/SessionManager";

export default function PiSettingsPage() {
  return (
    <div
      style={{
        width: "calc(100% - 64px)",
        maxWidth: 1040,
        marginInline: "auto",
        display: "flex",
        flexDirection: "column",
        gap: "24px",
        paddingBottom: "40px",
        paddingTop: "16px",
      }}
    >
      <CliStatusCard />
      <ProviderCard />
      <WorkspaceEditor />
      <SessionManager />
    </div>
  );
}
