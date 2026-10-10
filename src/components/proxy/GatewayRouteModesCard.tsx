import {
  Button,
  Card,
  InputNumber,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import ExperimentOutlined from "@ant-design/icons/es/icons/ExperimentOutlined";
import { useTranslation } from "react-i18next";
import {
  catalogModelSelectProps,
  RouteModesHelpButton,
  RouteModesTutorialButton,
  type CatalogModelSelectOption,
  type RouteHelpTab,
} from "@/components/proxy";
import type {
  GatewayCatalogModelOption,
  RouteMode,
  RouteModePatch,
  RouteModeUsageStat,
} from "@/types/backend";

const { Text } = Typography;

export const MODE_ORDER = [
  "image_gen",
  "web_search",
  "vision",
  "long_context",
  "background",
  "plan",
  "think",
  "edit",
  "default",
] as const;

export function modeCapabilityWarning(
  row: RouteMode,
  catalog: GatewayCatalogModelOption[],
): { key: string; defaultValue: string; window?: number; threshold?: number } | null {
  if (!row.model) return null;
  const entry = catalog.find((item) => item.publicId === row.model);
  const lower = row.model.toLowerCase();
  if (row.id === "long_context" && entry?.contextWindow && row.threshold > entry.contextWindow) {
    return {
      key: "gateway.capabilityWarnWindow",
      defaultValue: "窗口 {{window}} 小于阈值 {{threshold}}",
      window: entry.contextWindow,
      threshold: row.threshold,
    };
  }
  if (row.id === "vision") {
    if (entry && entry.visionEnabled === false) {
      return { key: "gateway.capabilityWarnVision", defaultValue: "所选模型可能不支持视觉" };
    }
    if (!entry) {
      const maybeVision =
        lower.includes("gpt-4") ||
        lower.includes("gpt-5") ||
        lower.includes("gpt-6") ||
        lower.includes("gemini") ||
        lower.includes("claude") ||
        lower.includes("vl") ||
        lower.includes("vision");
      if (!maybeVision) {
        return { key: "gateway.capabilityWarnVision", defaultValue: "所选模型可能不支持视觉" };
      }
    }
  }
  if (row.id === "web_search" && entry && entry.webSearchEnabled === false) {
    return { key: "gateway.capabilityWarnSearch", defaultValue: "所选模型可能不支持联网" };
  }
  return null;
}

export function thinkingLevelsFor(model: string, catalog: GatewayCatalogModelOption[]): string[] {
  const entry = catalog.find((item) => item.publicId === model);
  const levels = (entry?.reasoningLevels ?? []).map((item) => item.trim()).filter(Boolean);
  return levels.length > 0 ? levels : ["off", "low", "medium", "high"];
}

export type GatewayModelSelectOption = CatalogModelSelectOption & {
  inputPrice?: number;
  contextWindow?: number;
  webSearchEnabled?: boolean;
};

export interface GatewayRouteModesCardProps {
  modes: RouteMode[];
  loading?: boolean;
  modelOptions: GatewayModelSelectOption[];
  catalog: GatewayCatalogModelOption[];
  usageStats: RouteModeUsageStat[];
  onPatchMode: (id: string, patch: RouteModePatch) => void;
  onSimulate: () => void;
  onOpenHelp: (tab: RouteHelpTab) => void;
}

export function GatewayRouteModesCard({
  modes,
  loading = false,
  modelOptions,
  catalog,
  usageStats,
  onPatchMode,
  onSimulate,
  onOpenHelp,
}: GatewayRouteModesCardProps) {
  const { t } = useTranslation();

  const sortedModes = [...modes].sort((a, b) => {
    const left = MODE_ORDER.indexOf(a.id as (typeof MODE_ORDER)[number]);
    const right = MODE_ORDER.indexOf(b.id as (typeof MODE_ORDER)[number]);
    return (left < 0 ? 99 : left) - (right < 0 ? 99 : right);
  });

  return (
    <Card
      size="small"
      title={t("gateway.routeModes", { defaultValue: "路由模式" })}
      extra={
        <Space size={8}>
          <Button
            size="small"
            icon={<ExperimentOutlined />}
            onClick={onSimulate}
          >
            {t("gateway.simulate", { defaultValue: "试跑" })}
          </Button>
          <RouteModesHelpButton onClick={() => onOpenHelp("guide")} />
          <RouteModesTutorialButton onClick={() => onOpenHelp("tutorial")} />
        </Space>
      }
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12 }}>
        {t("gateway.modesHelpIntro", {
          defaultValue:
            "Agent 请求 auto 时，网关按这次请求的特征选一行。你在 /model 里点了具体目录模型时，模式全部让路。",
        })}
      </Text>
      <Table
        size="small"
        rowKey="id"
        pagination={false}
        loading={loading}
        dataSource={sortedModes}
        columns={[
          {
            title: t("gateway.modeName", { defaultValue: "模式" }),
            dataIndex: "id",
            render: (id: string) => (
              <Tooltip title={t(`gateway.modeHint.${id}`, { defaultValue: id })}>
                <span style={{ cursor: "help", borderBottom: "1px dotted var(--color-text-tertiary)" }}>
                  {t(`gateway.modes.${id}`, { defaultValue: id })}
                </span>
              </Tooltip>
            ),
          },
          {
            title: t("gateway.enabled", { defaultValue: "启用" }),
            dataIndex: "enabled",
            render: (enabled: boolean, row: RouteMode) => (
              <Switch
                checked={enabled}
                onChange={(checked) => {
                  onPatchMode(row.id, { enabled: checked });
                }}
              />
            ),
          },
          {
            title: t("gateway.model", { defaultValue: "模型" }),
            dataIndex: "model",
            render: (model: string, row: RouteMode) => (
              <Select
                {...catalogModelSelectProps}
                allowClear
                style={{ minWidth: 200 }}
                value={model || undefined}
                options={modelOptions}
                filterSort={(a, b) => {
                  const left =
                    modelOptions.find((item) => item.value === a.value)?.inputPrice ??
                    Number.POSITIVE_INFINITY;
                  const right =
                    modelOptions.find((item) => item.value === b.value)?.inputPrice ??
                    Number.POSITIVE_INFINITY;
                  return left - right;
                }}
                onChange={(value) => {
                  onPatchMode(row.id, { model: value ?? "" });
                }}
              />
            ),
          },
          {
            title: t("gateway.thinking", { defaultValue: "挡位" }),
            render: (_: unknown, row: RouteMode) => {
              let effort = "off";
              try {
                const parsed = JSON.parse(row.thinkingConfigJson || "{}") as {
                  reasoningEffort?: string;
                  mode?: string;
                };
                effort = parsed.mode === "disabled" ? "off" : (parsed.reasoningEffort ?? "off");
              } catch {
                effort = "off";
              }
              const labels: Record<string, string> = {
                off: t("gateway.thinkingOff", { defaultValue: "关闭" }),
                low: t("gateway.thinkingLow", { defaultValue: "低" }),
                medium: t("gateway.thinkingMedium", { defaultValue: "中" }),
                high: t("gateway.thinkingHigh", { defaultValue: "高" }),
              };
              const levels = thinkingLevelsFor(row.model, catalog);
              const options = levels.map((value) => ({ value, label: labels[value] ?? value }));
              if (!options.some((item) => item.value === effort)) {
                options.unshift({ value: effort, label: labels[effort] ?? effort });
              }
              return (
                <Select
                  style={{ width: 100 }}
                  value={effort}
                  options={options}
                  onChange={(value) => {
                    const thinking =
                      value === "off"
                        ? { mode: "disabled" }
                        : { mode: "effort", reasoningEffort: value };
                    onPatchMode(row.id, { thinkingConfigJson: JSON.stringify(thinking) });
                  }}
                />
              );
            },
          },
          {
            title: t("gateway.fallback", { defaultValue: "备用" }),
            render: (_: unknown, row: RouteMode) => (
              <Space size={4} align="center">
                <Select
                  {...catalogModelSelectProps}
                  mode="multiple"
                  allowClear
                  maxTagCount={1}
                  style={{ minWidth: 160 }}
                  value={row.fallbackModels}
                  options={modelOptions}
                  onChange={(value) => {
                    onPatchMode(row.id, { fallbackModels: value.slice(0, 2) });
                  }}
                />
                <Tooltip
                  title={t("gateway.fallbackCountHint", {
                    count: row.fallbackModels?.length ?? 0,
                    defaultValue: `已配置 ${row.fallbackModels?.length ?? 0}/2 个备用模型`,
                  })}
                >
                  <Tag style={{ margin: 0 }}>
                    {`${row.fallbackModels?.length ?? 0}/2`}
                  </Tag>
                </Tooltip>
              </Space>
            ),
          },
          {
            title: t("gateway.threshold", { defaultValue: "阈值" }),
            render: (_: unknown, row: RouteMode) =>
              row.id === "long_context" ? (
                <Tooltip
                  title={t("gateway.thresholdHint", {
                    defaultValue:
                      "估算口径：CJK 约 1 token/字，ASCII 约 4 字符/token。新行默认 60000，不自动改写已有阈值。可用试跑器对照实时估算。",
                  })}
                >
                  <InputNumber
                    min={0}
                    placeholder="60000"
                    value={row.threshold}
                    addonAfter={t("gateway.thresholdUnit", { defaultValue: "token" })}
                    onChange={(value) => {
                      onPatchMode(row.id, { threshold: value ?? 0 });
                    }}
                  />
                </Tooltip>
              ) : (
                "—"
              ),
          },
          {
            title: t("gateway.weekUsage", { defaultValue: "近 7 天" }),
            render: (_: unknown, row: RouteMode) => {
              const stat = usageStats.find((item) => item.modeId === row.id);
              const warning = modeCapabilityWarning(row, catalog);
              return (
                <Space direction="vertical" size={0}>
                  <span>{stat ? `${stat.requestCount} / ${stat.estimatedCost.toFixed(4)}` : "0"}</span>
                  {warning ? (
                    <Tag color="warning">
                      {t(warning.key, {
                        defaultValue: warning.defaultValue,
                        window: warning.window,
                        threshold: warning.threshold,
                      })}
                    </Tag>
                  ) : null}
                </Space>
              );
            },
          },
        ]}
      />
    </Card>
  );
}
