import React from "react";
import { Button, ConfigProvider, Dropdown, Select, Segmented, Tooltip, theme, type MenuProps } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import DownOutlined from "@ant-design/icons/es/icons/DownOutlined";
import { useTranslation } from "react-i18next";
import {
  USAGE_SOURCE_FILTER_OPTIONS,
  usageSourceSegmentLabel,
  type UsageSourceFilter,
} from "@/components/UsageSourceIcons";
import { USAGE_PERIOD_VALUES, usagePeriodLabelKey, type UsagePeriod } from "@/utils/usagePeriod";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

const SOURCE_SHORT_LABEL: Record<UsageSourceFilter, string> = {
  all: "usage.sourceAll",
  claude_code: "agentSwitcher.claudeCode",
  claude_desktop: "agentSwitcher.desktop",
  codex: "agentSwitcher.codex",
  opencode: "agentSwitcher.opencode",
  pi: "agentSwitcher.pi",
  antigravity: "usage.sourceAntigravity",
};

export interface UsageToolbarProps {
  period: UsagePeriod;
  onPeriodChange: (period: UsagePeriod) => void;
  logTargetApp: UsageSourceFilter;
  onTargetAppChange: (target: UsageSourceFilter) => void;
  refreshing: boolean;
  onRefresh: () => void;
  onSyncCodex?: () => void;
  onRebuildCodex?: () => void;
  onSyncClaudeCode?: () => void;
  onRebuildClaudeCode?: () => void;
  onSyncOpenCode?: () => void;
  onRebuildOpenCode?: () => void;
  onSyncPi?: () => void;
  onRebuildPi?: () => void;
  onOpenPricing: () => void;
  onOpenMaintenance: () => void;
  maintaining: boolean;
  className?: string;
  style?: React.CSSProperties;
}

export const UsageToolbar: React.FC<UsageToolbarProps> = ({
  period,
  onPeriodChange,
  logTargetApp,
  onTargetAppChange,
  refreshing,
  onRefresh,
  onSyncCodex,
  onRebuildCodex,
  onSyncClaudeCode,
  onRebuildClaudeCode,
  onSyncOpenCode,
  onRebuildOpenCode,
  onSyncPi,
  onRebuildPi,
  onOpenPricing,
  onOpenMaintenance,
  maintaining,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const availableOptions = USAGE_SOURCE_FILTER_OPTIONS.filter((option) => {
    if (option.value === "all" || option.value === "antigravity") return true;
    return visibleAgents.includes(option.value);
  });

  const includesCodex = visibleAgents.includes("codex") && (logTargetApp === "all" || logTargetApp === "codex");
  const includesOpenCode = visibleAgents.includes("opencode") && (logTargetApp === "all" || logTargetApp === "opencode");
  const includesClaudeCode = visibleAgents.includes("claude_code") && (logTargetApp === "all" || logTargetApp === "claude_code");
  const includesPi = visibleAgents.includes("pi") && (logTargetApp === "all" || logTargetApp === "pi");

  const moreItems: MenuProps["items"] = [
    ...(includesClaudeCode && onSyncClaudeCode
      ? [{ key: "syncClaudeCode", label: t("usage.syncClaudeCodeSessions"), disabled: refreshing, onClick: onSyncClaudeCode }]
      : []),
    ...(includesClaudeCode && onRebuildClaudeCode
      ? [{ key: "rebuildClaudeCode", label: t("usage.rebuildClaudeCodeSessions"), disabled: refreshing, onClick: onRebuildClaudeCode }]
      : []),
    ...(includesCodex && onSyncCodex
      ? [{ key: "syncCodex", label: t("usage.syncCodexSessions"), disabled: refreshing, onClick: onSyncCodex }]
      : []),
    ...(includesCodex && onRebuildCodex
      ? [{ key: "rebuildCodex", label: t("usage.rebuildCodexSessions"), disabled: refreshing, onClick: onRebuildCodex }]
      : []),
    ...(includesOpenCode && onSyncOpenCode
      ? [{ key: "syncOpenCode", label: t("usage.syncOpenCodeSessions"), disabled: refreshing, onClick: onSyncOpenCode }]
      : []),
    ...(includesOpenCode && onRebuildOpenCode
      ? [{ key: "rebuildOpenCode", label: t("usage.rebuildOpenCodeSessions"), disabled: refreshing, onClick: onRebuildOpenCode }]
      : []),
    ...(includesPi && onSyncPi
      ? [{ key: "syncPi", label: t("usage.syncPiSessions"), disabled: refreshing, onClick: onSyncPi }]
      : []),
    ...(includesPi && onRebuildPi
      ? [{ key: "rebuildPi", label: t("usage.rebuildPiSessions"), disabled: refreshing, onClick: onRebuildPi }]
      : []),
    { type: "divider" },
    { key: "pricing", icon: <DollarOutlined />, label: t("usage.configurePricing"), onClick: onOpenPricing },
    { key: "maintenance", label: t("usage.maintainLogs"), disabled: maintaining, onClick: onOpenMaintenance },
  ];

  return (
    <div className={`cc-workbench-header ${className}`.trim()} style={style}>
      <div className="cc-header-left">
        <Select<UsagePeriod>
          value={period}
          style={{ width: 148 }}
          aria-label={t("usage.period", { defaultValue: "统计时间" })}
          options={USAGE_PERIOD_VALUES.map((value) => ({
            value,
            label:
              typeof value === "number"
                ? t("usage.lastDays", { days: value })
                : t(usagePeriodLabelKey(value)),
          }))}
          onChange={onPeriodChange}
        />
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
          <Segmented<UsageSourceFilter>
            size="small"
            value={logTargetApp}
            onChange={onTargetAppChange}
            aria-label={t("usage.dataSource", { defaultValue: "数据来源" })}
            style={{
              border: `1px solid ${token.colorBorder}`,
              borderRadius: token.borderRadius,
              boxSizing: "border-box",
            }}
            options={availableOptions.map((option) => {
              const fullLabel = t(option.labelKey);
              const shortLabel = t(SOURCE_SHORT_LABEL[option.value], { defaultValue: fullLabel });
              return {
                value: option.value,
                label: (
                  <Tooltip title={fullLabel}>
                    {usageSourceSegmentLabel(option.value, shortLabel)}
                  </Tooltip>
                ),
              };
            })}
          />
        </ConfigProvider>
      </div>
      <div className="cc-header-right">
        <Button
          icon={<ReloadOutlined spin={refreshing} />}
          loading={refreshing}
          onClick={onRefresh}
        >
          {t("common.refresh", { defaultValue: "刷新" })}
        </Button>
        <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
          <Button loading={maintaining}>
            {t("common.moreActions", { defaultValue: "更多操作" })}
            <DownOutlined style={{ marginLeft: 6, fontSize: 11 }} />
          </Button>
        </Dropdown>
      </div>
    </div>
  );
};
