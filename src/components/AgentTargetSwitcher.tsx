import React from "react";
import { ConfigProvider, Segmented, Tooltip, theme } from "antd";
import { useTranslation } from "react-i18next";
import { usageSourceIcon, usageSourceSegmentLabel } from "@/components/UsageSourceIcons";
import type { ProviderTarget } from "@/types/backend";

export const TARGET_OPTIONS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode"];

export const LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "workspace.claude_code",
  claude_desktop: "workspace.claude_desktop",
  codex: "workspace.codex",
  opencode: "workspace.opencode",
};

const SHORT_LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "agentSwitcher.claudeCode",
  claude_desktop: "agentSwitcher.desktop",
  codex: "agentSwitcher.codex",
  opencode: "agentSwitcher.opencode",
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
  className,
}) => {
  const { t } = useTranslation();
  const { token } = theme.useToken();

  return (
    <ConfigProvider
      theme={{
        components: {
          Segmented: {
            trackBg: token.colorBgContainer,
            itemSelectedBg: token.colorFillSecondary,
            itemHoverBg: token.colorFillTertiary,
            trackPadding: 2,
          },
        },
      }}
    >
      <Segmented<ProviderTarget>
        className={className}
        size="small"
        block={block}
        value={value}
        onChange={onChange}
        aria-label={t("workspace.target")}
        style={{
          border: `1px solid ${token.colorBorder}`,
          borderRadius: token.borderRadius,
          boxSizing: "border-box",
        }}
        options={TARGET_OPTIONS.map((option) => {
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
    </ConfigProvider>
  );
};
