import React from "react";
import { Button, Dropdown, Select, Typography, type MenuProps } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import EllipsisOutlined from "@ant-design/icons/es/icons/EllipsisOutlined";
import { useTranslation } from "react-i18next";
import type { UsageSourceFilter } from "@/components/UsageSourceIcons";
import { UsageSourceFilterSegmented } from "@/components/UsageSourceFilterSegmented";
import { USAGE_PERIOD_VALUES, usagePeriodLabelKey, type UsagePeriod } from "@/utils/usagePeriod";
import { Inline } from "@/components/ui";

const { Text } = Typography;

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
  onOpenPricing,
  onOpenMaintenance,
  maintaining,
  className = "",
  style,
}) => {
  const { t } = useTranslation();

  const includesCodex = logTargetApp === "all" || logTargetApp === "codex";
  const includesOpenCode = logTargetApp === "all" || logTargetApp === "opencode";
  const includesClaudeCode = logTargetApp === "all" || logTargetApp === "claude_code";

  // Low-frequency maintenance operations live in the overflow menu.
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
    { type: "divider" },
    { key: "pricing", icon: <DollarOutlined />, label: t("usage.configurePricing"), onClick: onOpenPricing },
    { key: "maintenance", label: t("usage.maintainLogs"), disabled: maintaining, onClick: onOpenMaintenance },
  ];

  return (
    <Inline
      justify="space-between"
      align="center"
      wrap
      gap="md"
      className={`usage-toolbar ${className}`.trim()}
      style={style}
    >
      {/* Left Filter Controls */}
      <Inline gap="md" align="center" wrap>
        <Inline gap="xs" align="center">
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {t("usage.period", { defaultValue: "统计时间" })}:
          </Text>
          <Select
            size="small"
            value={period}
            style={{ width: 140 }}
            options={USAGE_PERIOD_VALUES.map((value) => ({
              value,
              label:
                typeof value === "number"
                  ? t("usage.lastDays", { days: value })
                  : t(usagePeriodLabelKey(value)),
            }))}
            onChange={onPeriodChange}
          />
        </Inline>

        <Inline gap="xs" align="center">
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {t("usage.dataSource", { defaultValue: "数据来源" })}:
          </Text>
          <UsageSourceFilterSegmented
            value={logTargetApp}
            onChange={onTargetAppChange}
            t={t}
          />
        </Inline>
      </Inline>

      {/* Right: refresh + overflow */}
      <Inline gap="xs" align="center" wrap>
        <Button
          size="small"
          icon={<ReloadOutlined spin={refreshing} />}
          loading={refreshing}
          onClick={onRefresh}
        >
          {t("common.refresh", { defaultValue: "刷新" })}
        </Button>
        <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
          <Button
            size="small"
            icon={<EllipsisOutlined />}
            loading={maintaining}
            aria-label={t("common.moreActions", { defaultValue: "更多操作" })}
          />
        </Dropdown>
      </Inline>
    </Inline>
  );
};
