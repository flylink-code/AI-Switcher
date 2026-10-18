import React from "react";
import { Button, Dropdown, Select, Tooltip, type MenuProps } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import DownOutlined from "@ant-design/icons/es/icons/DownOutlined";
import EllipsisOutlined from "@ant-design/icons/es/icons/EllipsisOutlined";
import { useTranslation } from "react-i18next";
import {
  USAGE_SOURCE_FILTER_OPTIONS,
  usageSourceSegmentLabel,
  type UsageSourceFilter,
  type UsageSourceFilterOption,
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
  kiro: { key: "usage.sourceKiro", fallback: "Kiro" },
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
      option.value === "kiro" ||
      option.value === "vscode_copilot"
    ) {
      return true;
    }
    return visibleAgents.includes(option.value);
  });

  const allOption = availableOptions.find((option) => option.group === "all");
  const agentOptions = availableOptions.filter((option) => option.group === "agent");
  const gatewayOptions = availableOptions.filter((option) => option.group === "gateway");

  const renderOptionPill = (option: UsageSourceFilterOption) => {
    const fullLabel = t(option.labelKey);
    const shortDef = SOURCE_SHORT_LABEL[option.value];
    const shortLabel = t(shortDef.key, { defaultValue: shortDef.fallback || fullLabel });
    const active = logTargetApp === option.value;
    const isAll = option.value === "all";
    return (
      <Tooltip key={option.value} title={fullLabel} mouseEnterDelay={0.25}>
        <button
          type="button"
          role="tab"
          aria-selected={active}
          className={`usage-source-pill${isAll ? " usage-source-pill--all" : ""}${active ? " is-active" : ""}`}
          onClick={() => onTargetAppChange(option.value)}
        >
          {usageSourceSegmentLabel(option.value, shortLabel, 14)}
        </button>
      </Tooltip>
    );
  };

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

  const refreshLabel = t("common.refresh", { defaultValue: "刷新" });
  const moreActionsLabel = t("common.moreActions", { defaultValue: "更多操作" });
  const periodTagLabel = t("usage.period", { defaultValue: "统计时间" });

  const getPeriodLabel = (value: UsagePeriod) =>
    typeof value === "number"
      ? t("usage.lastDays", { days: value })
      : t(usagePeriodLabelKey(value));

  return (
    <div className="usage-toolbar-container">
      <div className={`page-strip cc-workbench-header usage-toolbar-strip ${className}`.trim()} style={style}>
        <div className="usage-toolbar-row usage-toolbar-row--sources">
          <div
            className="usage-source-switcher"
            role="tablist"
            aria-label={t("usage.dataSource", { defaultValue: "数据来源" })}
          >
            {allOption && (
              <div className="usage-source-group usage-source-group--all">
                {renderOptionPill(allOption)}
              </div>
            )}
            {agentOptions.length > 0 && (
              <div className="usage-source-group" role="group" aria-label={t("usage.groupAgent", { defaultValue: "Agent" })}>
                <span className="usage-source-group-tag">
                  {t("usage.groupAgent", { defaultValue: "Agent" })}
                </span>
                <div className="usage-source-group-items">
                  {agentOptions.map(renderOptionPill)}
                </div>
              </div>
            )}
            {gatewayOptions.length > 0 && (
              <div className="usage-source-group" role="group" aria-label={t("usage.groupGateway", { defaultValue: "网关" })}>
                <span className="usage-source-group-tag">
                  {t("usage.groupGateway", { defaultValue: "网关" })}
                </span>
                <div className="usage-source-group-items">
                  {gatewayOptions.map(renderOptionPill)}
                </div>
              </div>
            )}
          </div>
        </div>
        <div className="usage-toolbar-row usage-toolbar-row--controls">
          <div className="usage-toolbar-controls-left">
            <div
              className="usage-source-group usage-period-group"
              role="group"
              aria-label={periodTagLabel}
            >
              <span className="usage-source-group-tag usage-period-group-tag">
                {periodTagLabel}
              </span>
              <div className="usage-source-group-items">
                {USAGE_PERIOD_VALUES.map((value) => {
                  const active = period === value;
                  const label = getPeriodLabel(value);
                  return (
                    <button
                      key={String(value)}
                      type="button"
                      aria-pressed={active}
                      className={`usage-source-pill usage-period-pill${active ? " is-active" : ""}`}
                      onClick={() => onPeriodChange(value)}
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            </div>
            <Select<UsagePeriod>
              size="small"
              className="usage-period-select usage-period-select--compact"
              value={period}
              style={{ width: 112 }}
              aria-label={periodTagLabel}
              options={USAGE_PERIOD_VALUES.map((value) => ({
                value,
                label: getPeriodLabel(value),
              }))}
              onChange={onPeriodChange}
            />
          </div>
          <div className="cc-header-right usage-toolbar-controls-right">
            <Tooltip title={refreshLabel} mouseEnterDelay={0.3}>
              <Button
                size="small"
                type="text"
                className="page-strip-quiet usage-toolbar-action-btn"
                icon={<ReloadOutlined spin={refreshing} />}
                loading={refreshing}
                aria-label={refreshLabel}
                onClick={onRefresh}
              >
                <span className="usage-toolbar-btn-text">{refreshLabel}</span>
              </Button>
            </Tooltip>
            <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
              <Tooltip title={moreActionsLabel} mouseEnterDelay={0.3}>
                <Button
                  size="small"
                  type="text"
                  className="page-strip-quiet usage-toolbar-action-btn"
                  icon={<EllipsisOutlined className="usage-toolbar-more-icon" />}
                  loading={maintaining}
                  aria-label={moreActionsLabel}
                >
                  <span className="usage-toolbar-btn-text">
                    {moreActionsLabel}
                    <DownOutlined style={{ marginLeft: 5, fontSize: 10 }} />
                  </span>
                </Button>
              </Tooltip>
            </Dropdown>
          </div>
        </div>
      </div>
    </div>
  );
};
