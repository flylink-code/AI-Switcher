import React from "react";
import { Segmented, Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import { usageSourceIcon, usageSourceSegmentLabel } from "@/components/UsageSourceIcons";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ProviderTarget } from "@/types/backend";

export const TARGET_OPTIONS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode", "pi", "dsh"];

export const LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "workspace.claude_code",
  claude_desktop: "workspace.claude_desktop",
  codex: "workspace.codex",
  opencode: "workspace.opencode",
  pi: "workspace.pi",
  dsh: "workspace.dsh",
};

const SHORT_LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "agentSwitcher.claudeCode",
  claude_desktop: "agentSwitcher.desktop",
  codex: "agentSwitcher.codex",
  opencode: "agentSwitcher.opencode",
  pi: "agentSwitcher.pi",
  dsh: "agentSwitcher.dsh",
};

export interface AgentTargetSwitcherProps {
  value: ProviderTarget;
  onChange: (target: ProviderTarget) => void;
  /** Stretch the segmented control to the container width (dashboard cards). */
  block?: boolean;
  /** Icons only (full name in tooltip) — for tight spaces like dashboard cards. */
  iconOnly?: boolean;
  className?: string;
}

/**
 * Page-level Agent target switcher (controlled, store-agnostic).
 * Each page binds it to its own persisted slice of pagePreferencesStore
 * (providersTarget / proxyTarget) — there is no longer a global Agent context.
 */
export const AgentTargetSwitcher: React.FC<AgentTargetSwitcherProps> = ({
  value,
  onChange,
  block = false,
  iconOnly = false,
  className = "",
}) => {
  const { t } = useTranslation();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const availableOptions = TARGET_OPTIONS.filter((opt) => visibleAgents.includes(opt));
  const activeValue = availableOptions.includes(value) ? value : (availableOptions[0] ?? value);

  return (
    <Segmented<ProviderTarget>
      className={["app-segmented-switcher", className].filter(Boolean).join(" ")}
      size="small"
      block={block}
      value={activeValue}
      onChange={onChange}
      aria-label={t("workspace.target")}
      options={availableOptions.map((option) => {
        const fullLabel = t(LABEL_KEYS[option]);
        return {
          value: option,
          label: (
            <Tooltip title={fullLabel}>
              {iconOnly
                ? usageSourceIcon(option, { size: 15 })
                : usageSourceSegmentLabel(
                    option,
                    t(SHORT_LABEL_KEYS[option], { defaultValue: fullLabel }),
                  )}
            </Tooltip>
          ),
        };
      })}
    />
  );
};
