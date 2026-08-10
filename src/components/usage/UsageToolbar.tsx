import React from "react";
import { Button, Select, Space, Typography } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import DollarOutlined from "@ant-design/icons/es/icons/DollarOutlined";
import { useTranslation } from "react-i18next";
import type { SessionProvider } from "@/types/backend";
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
            {t("usage.statsSource", { defaultValue: "统计来源" })}:
          </Text>
          <UsageSourceFilterSegmented
            value={logTargetApp}
            onChange={onTargetAppChange}
            t={t}
          />
        </Inline>

        <Button
          size="small"
          icon={<ReloadOutlined spin={refreshing} />}
          loading={refreshing}
          onClick={onRefresh}
        >
          {t("common.refresh", { defaultValue: "刷新" })}
        </Button>
      </Inline>

      {/* Right Action Tools */}
      <Inline gap="xs" align="center" wrap>
        {includesCodex && onSyncCodex && (
          <Button size="small" loading={refreshing} onClick={onSyncCodex}>
            {t("usage.syncCodexSessions")}
          </Button>
        )}
        {includesClaudeCode && onSyncClaudeCode && (
          <Button size="small" loading={refreshing} onClick={onSyncClaudeCode}>
            {t("usage.syncClaudeCodeSessions")}
          </Button>
        )}
        {includesOpenCode && onSyncOpenCode && (
          <Button size="small" loading={refreshing} onClick={onSyncOpenCode}>
            {t("usage.syncOpenCodeSessions")}
          </Button>
        )}

        <Button size="small" icon={<DollarOutlined />} onClick={onOpenPricing}>
          {t("usage.configurePricing")}
        </Button>

        <Button size="small" loading={maintaining} onClick={onOpenMaintenance}>
          {t("usage.maintainLogs")}
        </Button>
      </Inline>
    </Inline>
  );
};
