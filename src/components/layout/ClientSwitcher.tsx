import React from "react";
import { useTranslation } from "react-i18next";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import type { ProviderTarget } from "@/types/backend";

export interface ClientSwitcherProps {
  size?: "small" | "middle";
  className?: string;
}

export const ClientSwitcher: React.FC<ClientSwitcherProps> = ({
  size = "small",
  className,
}) => {
  const { t } = useTranslation();
  const workspaceTarget = usePagePreferencesStore((s) => s.workspaceTarget);
  const setWorkspaceTarget = usePagePreferencesStore((s) => s.setWorkspaceTarget);

  return (
    <WorkspaceTargetSegmented
      value={workspaceTarget}
      onChange={(value: ProviderTarget) => setWorkspaceTarget(value)}
      t={t}
      size={size}
      className={className}
      ariaLabel={t("workspace.target")}
    />
  );
};
