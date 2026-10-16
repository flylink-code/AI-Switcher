import React from "react";
import { Button, Dropdown, Select, Segmented, Tooltip, type MenuProps } from "antd";
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

const SOURCE_SHORT_LABEL: Record<UsageSourceFilter, { key: string; fallback: string }> = {
  all: { key: "usage.sourceAll", fallback: "全部" },
  claude_code: { key: "usage.shortClaudeCode", fallback: "Code" },
  claude_desktop: { key: "agentSwitcher.desktop", fallback: "桌面版" },
  codex: { key: "agentSwitcher.codex", fallback: "Codex" },
  opencode: { key: "agentSwitcher.opencode", fallback: "OpenCode" },
  pi: { key: "usage.shortPi", fallback: "Pi" },
  dsh: { key: "agentSwitcher.dsh", fallback: "DSH" },
  cline: { key: "agentSwitcher.cline", fallback: "Cline" },
  vscode_copilot: { key: "usage.shortVscodeCopilot", fallback: "Copilot" },
  antigravity: { key: "usage.sourceAntigravity", fallback: "Antigravity" },
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
  onSyncDsh?: () => void;
  onRebuildDsh?: () => void;
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
  onSyncDsh,
  onRebuildDsh,
  onOpenPricing,
  onOpenMaintenance,
  maintaining,
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const availableOptions = USAGE_SOURCE_FILTER_OPTIONS.filter((option) => {
    if (
      option.value === "all" ||
      option.value === "antigravity" ||
      option.value === "vscode_copilot"
    ) {
      return true;
    }
    return visibleAgents.includes(option.value);
  });

  const includesCodex = visibleAgents.includes("codex") && (logTargetApp === "all" || logTargetApp === "codex");
  const includesOpenCode = visibleAgents.includes("opencode") && (logTargetApp === "all" || logTargetApp === "opencode");
  const includesClaudeCode = visibleAgents.includes("claude_code") && (logTargetApp === "all" || logTargetApp === "claude_code");
  const includesPi = visibleAgents.includes("pi") && (logTargetApp === "all" || logTargetApp === "pi");
  const includesDsh = visibleAgents.includes("dsh") && (logTargetApp === "all" || logTargetApp === "dsh");

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
    ...(includesDsh && onSyncDsh
      ? [{ key: "syncDsh", label: t("usage.syncDshSessions"), disabled: refreshing, onClick: onSyncDsh }]
      : []),
    ...(includesDsh && onRebuildDsh
      ? [{ key: "rebuildDsh", label: t("usage.rebuildDshSessions"), disabled: refreshing, onClick: onRebuildDsh }]
      : []),
    { type: "divider" },
    { key: "pricing", icon: <DollarOutlined />, label: t("usage.configurePricing"), onClick: onOpenPricing },
    { key: "maintenance", label: t("usage.maintainLogs"), disabled: maintaining, onClick: onOpenMaintenance },
  ];

  return (
    <div className={`page-strip cc-workbench-header usage-toolbar-strip ${className}`.trim()} style={style}>
      <div className="page-strip-main cc-header-left">
        <Segmented<UsageSourceFilter>
          className="app-segmented-switcher"
          size="small"
          value={logTargetApp}
          onChange={onTargetAppChange}
          aria-label={t("usage.dataSource", { defaultValue: "数据来源" })}
          options={availableOptions.map((option) => {
            const fullLabel = t(option.labelKey);
            const shortDef = SOURCE_SHORT_LABEL[option.value];
            const shortLabel = t(shortDef.key, { defaultValue: shortDef.fallback || fullLabel });
            return {
              value: option.value,
              label: (
                <Tooltip title={fullLabel}>
                  {usageSourceSegmentLabel(option.value, shortLabel, 14)}
                </Tooltip>
              ),
            };
          })}
        />
      </div>
      <div className="cc-header-right">
        <Select<UsagePeriod>
          size="small"
          value={period}
          style={{ width: 132 }}
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
        <Button
          size="small"
          type="text"
          className="page-strip-quiet"
          icon={<ReloadOutlined spin={refreshing} />}
          loading={refreshing}
          onClick={onRefresh}
        >
          {t("common.refresh", { defaultValue: "刷新" })}
        </Button>
        <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
          <Button size="small" type="text" className="page-strip-quiet" loading={maintaining}>
            {t("common.moreActions", { defaultValue: "更多操作" })}
            <DownOutlined style={{ marginLeft: 6, fontSize: 11 }} />
          </Button>
        </Dropdown>
      </div>
    </div>
  );
};
