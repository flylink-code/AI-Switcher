import React from "react";
import { Select } from "antd";
import { useTranslation } from "react-i18next";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { usageSourceSegmentLabel } from "@/components/UsageSourceIcons";
import type { ProviderTarget } from "@/types/backend";

const TARGET_OPTIONS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode"];

const LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "workspace.claude_code",
  claude_desktop: "workspace.claude_desktop",
  codex: "workspace.codex",
  opencode: "workspace.opencode",
};

export interface ClientSwitcherProps {
  size?: "small" | "middle";
  className?: string;
}

/**
 * Global Current Client context selector (single source of truth:
 * pagePreferencesStore.workspaceTarget). Rendered once, in the ContextHeader.
 * Page-local analytics must use a Data Source filter instead of this control.
 */
export const ClientSwitcher: React.FC<ClientSwitcherProps> = ({
  size = "small",
  className,
}) => {
  const { t } = useTranslation();
  const workspaceTarget = usePagePreferencesStore((s) => s.workspaceTarget);
  const setWorkspaceTarget = usePagePreferencesStore((s) => s.setWorkspaceTarget);

  return (
    <Select<ProviderTarget>
      className={className}
      size={size}
      value={workspaceTarget}
      onChange={setWorkspaceTarget}
      aria-label={t("workspace.target")}
      style={{ minWidth: size === "small" ? 132 : 156 }}
      variant="outlined"
      labelRender={(props) =>
        usageSourceSegmentLabel(
          props.value as ProviderTarget,
          t(LABEL_KEYS[props.value as ProviderTarget]),
        )
      }
      options={TARGET_OPTIONS.map((option) => ({
        value: option,
        label: usageSourceSegmentLabel(option, t(LABEL_KEYS[option])),
      }))}
    />
  );
};
