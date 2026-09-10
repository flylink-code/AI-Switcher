import { useMemo, useState } from "react";
import {
  Alert,
  Badge,
  Button,
  Card,
  Input,
  InputNumber,
  Segmented,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import LinkOutlined from "@ant-design/icons/es/icons/LinkOutlined";
import CheckOutlined from "@ant-design/icons/es/icons/CheckOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  GatewayUpstreamPanel,
  RouteModesHelpButton,
  RouteModesHelpDrawer,
  RouteModesTutorialButton,
  type RouteHelpTab,
} from "@/components/proxy";
import AntigravityPage from "@/pages/AntigravityPage";
import { BIND_TARGETS } from "@/components/antigravity";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { listModelPricing } from "@/services/usage";
import {
  bindSmartGateway,
  deleteRouteRule,
  getSmartGatewayStatus,
  listGatewayCatalogEntries,
  listGatewayRouteLogs,
  listRouteModeUsageStats,
  listRouteModes,
  listRouteRules,
  listSmartGatewayBindings,
  setSmartGatewayPort,
  setSmartGatewayApiKey,
  rotateSmartGatewayApiKey,
  startSmartGateway,
  stopSmartGateway,
  unbindSmartGateway,
  updateRouteMode,
  upsertRouteRule,
} from "@/services/providers";
import type { ProviderTarget, RouteMode, RouteRule, GatewayCatalogModelOption } from "@/types/backend";

const { Text, Paragraph } = Typography;

type SnippetKind = "sdk" | "openai" | "anthropic" | "responses";

const MODE_ORDER = [
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

function modeCapabilityWarning(
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
      const maybeVision = lower.includes("gpt-4") || lower.includes("gpt-5") || lower.includes("gpt-6")
        || lower.includes("gemini") || lower.includes("claude") || lower.includes("vl") || lower.includes("vision");
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

function thinkingLevelsFor(model: string, catalog: GatewayCatalogModelOption[]): string[] {
  const entry = catalog.find((item) => item.publicId === model);
  const levels = (entry?.reasoningLevels ?? []).map((item) => item.trim()).filter(Boolean);
  return levels.length > 0 ? levels : ["off", "low", "medium", "high"];
}

function errMsg(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error ?? "未知错误");
}

export default function GatewayPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const tab = usePagePreferencesStore((state) => state.gatewayTab);
  const setTab = usePagePreferencesStore((state) => state.setGatewayTab);
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);
  const [port, setPort] = useState(15828);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [curlVisible, setCurlVisible] = useState(true);
  const [snippetKind, setSnippetKind] = useState<SnippetKind>("sdk");
  const [modesHelpOpen, setModesHelpOpen] = useState(false);
  const [modesHelpTab, setModesHelpTab] = useState<RouteHelpTab>("guide");

  const statusQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
  });
  const bindingsQuery = useQuery({
    queryKey: ["smart-gateway-bindings"],
    queryFn: listSmartGatewayBindings,
  });
  const modesQuery = useQuery({ queryKey: ["route-modes"], queryFn: listRouteModes });
  const rulesQuery = useQuery({ queryKey: ["route-rules"], queryFn: listRouteRules });
  const routesQuery = useQuery({
    queryKey: ["gateway-route-logs-all"],
    queryFn: () => listGatewayRouteLogs(null, 20),
  });
  const statsQuery = useQuery({
    queryKey: ["route-mode-usage"],
    queryFn: listRouteModeUsageStats,
    staleTime: 60_000,
  });
  const catalogQuery = useQuery({
    queryKey: ["gateway-catalog-entries", "claude_code"],
    queryFn: () => listGatewayCatalogEntries("claude_code"),
  });
  const pricingQuery = useQuery({ queryKey: ["model-pricing"], queryFn: listModelPricing });

  const status = statusQuery.data;
  const apiKey = apiKeyDraft ?? status?.apiKey ?? "";
  const listenUrl = (status?.baseUrl ?? `http://127.0.0.1:${status?.port ?? port}`).replace(/\/$/, "");
  const openaiUrl = `${listenUrl}/v1`;
  const curlSnippet = useMemo(() => {
    const key = apiKey || "sk-aisw-your-key";
    switch (snippetKind) {
      case "sdk":
        return t("gateway.sdkSnippet", {
          openaiUrl,
          anthropicUrl: listenUrl,
          apiKey: key,
          defaultValue: [
            `OpenAI / Cursor / Continue / 其他兼容客户端`,
            `  Base URL: ${openaiUrl}`,
            `  API Key:  ${key}`,
            `  Model:    auto`,
            ``,
            `Anthropic SDK`,
            `  Base URL: ${listenUrl}`,
            `  API Key:  ${key}`,
            `  Model:    auto`,
            ``,
            `Codex 风格目录加请求头: x-ai-switcher-target: codex`,
          ].join("\n"),
        });
      case "anthropic":
        return `curl -s ${listenUrl}/v1/messages \\\n  -H "x-api-key: ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"auto","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'`;
      case "responses":
        return `curl -s ${listenUrl}/v1/responses \\\n  -H "Authorization: Bearer ${key}" \\\n  -H "x-ai-switcher-target: codex" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"auto","input":"hi"}'`;
      case "openai":
        return `curl -s ${listenUrl}/v1/chat/completions \\\n  -H "Authorization: Bearer ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"auto","messages":[{"role":"user","content":"hi"}]}'`;
      default: {
        const _exhaustive: never = snippetKind;
        return _exhaustive;
      }
    }
  }, [apiKey, listenUrl, openaiUrl, snippetKind, t]);
  const copyText = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value);
      void message.success(t("antigravity.copied", { defaultValue: "已复制" }));
    } catch (error) {
      void message.error(errMsg(error));
    }
  };
  const bound = new Set((bindingsQuery.data ?? []).map((item) => item.targetApp));
  const modelOptions = useMemo(() => {
    const pricing = new Map(
      (pricingQuery.data ?? []).map((row) => [row.model.toLowerCase(), row]),
    );
    return (catalogQuery.data ?? []).map((entry) => {
      const price = pricing.get(entry.publicId.toLowerCase())
        ?? pricing.get(entry.publicId.split(".").pop()?.toLowerCase() ?? "");
      const extra = price
        ? `  ${price.inputPricePerMillion}/${price.outputPricePerMillion}`
        : "";
      return {
        value: entry.publicId,
        label: `${entry.displayName || entry.publicId}${extra}`,
        inputPrice: price?.inputPricePerMillion ?? Number.POSITIVE_INFINITY,
        contextWindow: entry.contextWindow ?? 0,
        webSearchEnabled: entry.webSearchEnabled ?? false,
      };
    });
  }, [catalogQuery.data, pricingQuery.data]);

  const handleStart = async () => {
    setBusy(true);
    try {
      await setSmartGatewayPort(port);
      const next = await startSmartGateway(port);
      queryClient.setQueryData(["smart-gateway-status"], next);
      void message.success(t("gateway.started", { port: next.port, defaultValue: "智能网关已启动 :{{port}}" }));
    } catch (error) {
      void message.error(errMsg(error));
      await statusQuery.refetch();
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    try {
      const next = await stopSmartGateway();
      queryClient.setQueryData(["smart-gateway-status"], next);
      void message.success(t("gateway.stopped", { defaultValue: "智能网关已停止" }));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusy(false);
    }
  };

  const handleSaveApiKey = async () => {
    setBusy(true);
    try {
      const next = await setSmartGatewayApiKey(apiKey);
      queryClient.setQueryData(["smart-gateway-status"], next);
      setApiKeyDraft(null);
      void message.success(t("gateway.apiKeySaved", { defaultValue: "对外 API Key 已保存" }));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusy(false);
    }
  };

  const handleRotateApiKey = async () => {
    setBusy(true);
    try {
      const next = await rotateSmartGatewayApiKey();
      queryClient.setQueryData(["smart-gateway-status"], next);
      setApiKeyDraft(null);
      void message.success(t("gateway.apiKeyRotated", { defaultValue: "已生成新的对外 API Key，请更新自定义 Agent" }));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusy(false);
    }
  };

  const bindMutation = useMutation({
    mutationFn: async (target: ProviderTarget) => {
      if (bound.has(target)) {
        await unbindSmartGateway(target);
      } else {
        await bindSmartGateway(target);
      }
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["smart-gateway-bindings"] });
      await queryClient.invalidateQueries({ queryKey: ["smart-gateway-status"] });
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error) => {
      void message.error(errMsg(error));
    },
  });

  if (tab === "antigravity") {
    return (
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Segmented
          size="small"
          value={tab}
          onChange={(value) => setTab(value as "smart" | "antigravity")}
          options={[
            { value: "smart", label: t("gateway.tabSmart", { defaultValue: "智能网关" }) },
            { value: "antigravity", label: t("gateway.tabAntigravity", { defaultValue: "反代网关" }) },
          ]}
        />
        <AntigravityPage embedded />
      </Space>
    );
  }

  const phase = status?.phase ?? "stopped";
  const running = status?.running ?? false;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%", minWidth: 0 }}>
      <Segmented
        size="small"
        value={tab}
        onChange={(value) => setTab(value as "smart" | "antigravity")}
        options={[
          { value: "smart", label: t("gateway.tabSmart", { defaultValue: "智能网关" }) },
          { value: "antigravity", label: t("gateway.tabAntigravity", { defaultValue: "反代网关" }) },
        ]}
      />

      <Alert
        type="info"
        showIcon
        message={t("gateway.catalogRefreshHint", {
          defaultValue: "Claude Code / Desktop / Codex 通过 /v1/models 拉目录，变更后需重启 Agent 才刷新。OpenCode / Pi / DSH / Cline 会立即重写配置。",
        })}
      />

      <Card
        size="small"
        title={t("gateway.service", { defaultValue: "智能网关服务" })}
        extra={
          <Space>
            <Badge
              status={running ? "success" : phase === "error" ? "error" : "default"}
              text={running ? t("proxy.running") : phase === "error" ? t("proxy.failed") : t("proxy.stopped")}
            />
            {running ? (
              <Button danger icon={<StopOutlined />} loading={busy} onClick={() => void handleStop()}>
                {t("proxy.stop")}
              </Button>
            ) : (
              <Button type="primary" icon={<PlayCircleOutlined />} loading={busy} onClick={() => void handleStart()}>
                {t("proxy.start")}
              </Button>
            )}
          </Space>
        }
      >
        <Space direction="vertical" size={12} style={{ width: "100%" }}>
          <Space wrap>
            <Tag color={running ? "success" : phase === "error" ? "error" : "default"}>
              {running ? t("proxy.running") : phase === "error" ? t("proxy.failed") : t("proxy.stopped")}
            </Tag>
            <Text type="secondary">
              {t("gateway.bindingsCount", {
                count: status?.bindingCount ?? bound.size,
                defaultValue: "{{count}} 个绑定",
              })}
            </Text>
          </Space>

          <Text type="secondary">
            {t("gateway.externalHint", {
              defaultValue:
                "对外 API Key 已自动生成，自定义 Agent 直接复制即可，不必绑定。把 Base URL 指到访问地址，模型用 auto。已绑定应用仍用各自入口令牌。",
            })}
          </Text>

          <Input
            readOnly
            value={listenUrl}
            addonBefore={t("gateway.accessUrl", { defaultValue: "访问地址" })}
            addonAfter={
              <Button type="text" size="small" icon={<CopyOutlined />} onClick={() => void copyText(listenUrl)}>
                {t("gateway.copyBaseUrl", { defaultValue: "复制地址" })}
              </Button>
            }
          />
          <Input
            readOnly
            value={openaiUrl}
            addonBefore={t("gateway.openaiBaseUrl", { defaultValue: "OpenAI Base URL" })}
            addonAfter={
              <Button type="text" size="small" icon={<CopyOutlined />} onClick={() => void copyText(openaiUrl)}>
                {t("gateway.copyBaseUrl", { defaultValue: "复制地址" })}
              </Button>
            }
          />

          <Space wrap>
            <InputNumber
              min={1024}
              max={65535}
              value={status?.port ?? port}
              onChange={(value) => setPort(value ?? 15828)}
              disabled={running}
              addonBefore={t("gateway.port", { defaultValue: "端口" })}
            />
            <Input.Password
              style={{ width: 280 }}
              value={apiKey}
              onChange={(event) => setApiKeyDraft(event.target.value)}
              placeholder="sk-aisw-..."
              addonBefore="API Key"
            />
            <Button
              type="primary"
              size="small"
              icon={<CopyOutlined />}
              disabled={!apiKey}
              onClick={() => void copyText(apiKey)}
            >
              {t("gateway.copyApiKey", { defaultValue: "复制 Key" })}
            </Button>
            <Button size="small" loading={busy} onClick={() => void handleSaveApiKey()}>
              {t("gateway.saveApiKey", { defaultValue: "保存 Key" })}
            </Button>
            <Button size="small" icon={<ReloadOutlined />} loading={busy} onClick={() => void handleRotateApiKey()}>
              {t("gateway.rotateApiKey", { defaultValue: "换新 Key" })}
            </Button>
          </Space>

          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("gateway.externalEndpoints", {
              defaultValue:
                "协议：Anthropic POST /v1/messages；OpenAI Chat POST /v1/chat/completions；Responses POST /v1/responses；目录 GET /v1/models；绘图 POST /v1/images/generations。OpenAI 风格模型名可加请求头 x-ai-switcher-target: codex。",
            })}
          </Text>

          <Space wrap>
            <Segmented
              size="small"
              value={snippetKind}
              onChange={(value) => setSnippetKind(value as SnippetKind)}
              options={[
                { value: "sdk", label: t("gateway.snippetSdk", { defaultValue: "接入配置" }) },
                { value: "openai", label: "OpenAI Chat" },
                { value: "anthropic", label: "Anthropic" },
                { value: "responses", label: "Responses" },
              ]}
            />
            <Button
              icon={<CopyOutlined />}
              onClick={() => void copyText(curlSnippet)}
            >
              {snippetKind === "sdk"
                ? t("gateway.copySdk", { defaultValue: "复制配置" })
                : t("antigravity.copyCurl", { defaultValue: "复制测试命令" })}
            </Button>
          </Space>
          {curlVisible ? (
            <Paragraph style={{ marginBottom: 0 }}>
              <pre style={{ margin: 0, padding: 8, borderRadius: 6, background: "var(--ant-color-bg-layout, #f5f5f5)", whiteSpace: "pre-wrap", fontSize: 12 }}>
                {curlSnippet}
              </pre>
              <Button type="link" size="small" style={{ padding: 0, marginTop: 4 }} onClick={() => setCurlVisible(false)}>
                {t("antigravity.hideTestCommand", { defaultValue: "收起测试命令" })}
              </Button>
            </Paragraph>
          ) : (
            <Button type="link" size="small" style={{ padding: 0 }} onClick={() => setCurlVisible(true)}>
              {t("antigravity.viewTestCommand", { defaultValue: "查看测试命令" })}
            </Button>
          )}
        </Space>
        {status?.lastError ? (
          <Alert style={{ marginTop: 12 }} type="error" showIcon message={status.lastError} />
        ) : null}
      </Card>

      <Card size="small" title={t("gateway.bindApps", { defaultValue: "绑定应用" })}>
        <Text type="secondary">{t("gateway.bindAppsHint", { defaultValue: "绑定后写入指向 127.0.0.1:15828 的供应商卡，并设为当前。自定义 Agent 用上方 API Key 即可，不必绑定。" })}</Text>
        <div style={{ display: "flex", gap: 12, flexWrap: "wrap", marginTop: 12 }}>
          {BIND_TARGETS.filter((target) => visibleAgents.includes(target)).map((target) => {
            const isBound = bound.has(target);
            return (
              <Button
                key={target}
                size="small"
                icon={isBound ? <CheckOutlined /> : <LinkOutlined />}
                loading={bindMutation.isPending && bindMutation.variables === target}
                onClick={() => bindMutation.mutate(target)}
              >
                {t(`workspace.${target}`)}
                {isBound ? <Tag color="green" style={{ marginLeft: 4, marginRight: 0 }}>{t("antigravity.bound")}</Tag> : null}
              </Button>
            );
          })}
        </div>
      </Card>

      <GatewayUpstreamPanel allowlistTarget="claude_code" />

      <Card
        size="small"
        title={t("gateway.routeModes", { defaultValue: "路由模式" })}
        extra={
          <Space size={8}>
            <RouteModesHelpButton
              onClick={() => {
                setModesHelpTab("guide");
                setModesHelpOpen(true);
              }}
            />
            <RouteModesTutorialButton
              onClick={() => {
                setModesHelpTab("tutorial");
                setModesHelpOpen(true);
              }}
            />
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
          dataSource={[...(modesQuery.data ?? [])].sort((a, b) => {
            const left = MODE_ORDER.indexOf(a.id as (typeof MODE_ORDER)[number]);
            const right = MODE_ORDER.indexOf(b.id as (typeof MODE_ORDER)[number]);
            return (left < 0 ? 99 : left) - (right < 0 ? 99 : right);
          })}
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
                    void updateRouteMode(row.id, { enabled: checked }).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-modes"] });
                    });
                  }}
                />
              ),
            },
            {
              title: t("gateway.model", { defaultValue: "模型" }),
              dataIndex: "model",
              render: (model: string, row: RouteMode) => (
                <Select
                  showSearch
                  allowClear
                  style={{ minWidth: 260 }}
                  value={model || undefined}
                  options={modelOptions}
                  filterSort={(a, b) => {
                    const left = modelOptions.find((item) => item.value === a.value)?.inputPrice ?? Number.POSITIVE_INFINITY;
                    const right = modelOptions.find((item) => item.value === b.value)?.inputPrice ?? Number.POSITIVE_INFINITY;
                    return left - right;
                  }}
                  onChange={(value) => {
                    void updateRouteMode(row.id, { model: value ?? "" }).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-modes"] });
                    });
                  }}
                />
              ),
            },
            {
              title: t("gateway.thinking", { defaultValue: "挡位" }),
              render: (_: unknown, row: RouteMode) => {
                let effort = "off";
                try {
                  const parsed = JSON.parse(row.thinkingConfigJson || "{}") as { reasoningEffort?: string; mode?: string };
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
                const levels = thinkingLevelsFor(row.model, catalogQuery.data ?? []);
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
                      const thinking = value === "off"
                        ? { mode: "disabled" }
                        : { mode: "effort", reasoningEffort: value };
                      void updateRouteMode(row.id, { thinkingConfigJson: JSON.stringify(thinking) }).then(() => {
                        void queryClient.invalidateQueries({ queryKey: ["route-modes"] });
                      });
                    }}
                  />
                );
              },
            },
            {
              title: t("gateway.fallback", { defaultValue: "备用" }),
              render: (_: unknown, row: RouteMode) => (
                <Select
                  mode="multiple"
                  allowClear
                  maxTagCount={1}
                  style={{ minWidth: 160 }}
                  value={row.fallbackModels}
                  options={modelOptions}
                  onChange={(value) => {
                    void updateRouteMode(row.id, { fallbackModels: value.slice(0, 3) }).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-modes"] });
                    });
                  }}
                />
              ),
            },
            {
              title: t("gateway.threshold", { defaultValue: "阈值" }),
              render: (_: unknown, row: RouteMode) =>
                row.id === "long_context" ? (
                  <InputNumber
                    min={0}
                    value={row.threshold}
                    addonAfter={t("gateway.thresholdUnit", { defaultValue: "token" })}
                    onChange={(value) => {
                      void updateRouteMode(row.id, { threshold: value ?? 0 }).then(() => {
                        void queryClient.invalidateQueries({ queryKey: ["route-modes"] });
                      });
                    }}
                  />
                ) : (
                  "—"
                ),
            },
            {
              title: t("gateway.weekUsage", { defaultValue: "近 7 天" }),
              render: (_: unknown, row: RouteMode) => {
                const stat = (statsQuery.data ?? []).find((item) => item.modeId === row.id);
                const warning = modeCapabilityWarning(row, catalogQuery.data ?? []);
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

      <Card
        size="small"
        title={t("gateway.routeRules", { defaultValue: "条件规则" })}
        extra={
          <Button
            size="small"
            icon={<PlusOutlined />}
            onClick={() => {
              const rule: RouteRule = {
                id: `rule_${crypto.randomUUID().replace(/-/g, "").slice(0, 12)}`,
                profileId: "gprof_shared",
                enabled: true,
                sortIndex: (rulesQuery.data?.length ?? 0),
                ruleType: "model-prefix",
                conditionJson: "{}",
                pattern: "",
                targetModel: "",
                thinkingConfigJson: "{}",
                rewritesJson: "[]",
              };
              void upsertRouteRule(rule).then(() => {
                void queryClient.invalidateQueries({ queryKey: ["route-rules"] });
              });
            }}
          >
            {t("gateway.addRule", { defaultValue: "添加规则" })}
          </Button>
        }
      >
        <Text type="secondary" style={{ display: "block", marginBottom: 12 }}>
          {t("gateway.rulesHelpIntro", {
            defaultValue:
              "规则优先于模式、排在显式模型之后。model-prefix 按客户端模型名前缀匹配；condition 用 token/工具/是否含图等左值。不做 JS 脚本。",
          })}
        </Text>
        <Table
          columns={[
            {
              title: t("gateway.ruleType", { defaultValue: "类型" }),
              dataIndex: "ruleType",
              render: (value: string, row: RouteRule) => (
                <Select
                  value={value}
                  options={[
                    { value: "model-prefix", label: "model-prefix" },
                    { value: "condition", label: "condition" },
                  ]}
                  onChange={(ruleType) => {
                    void upsertRouteRule({ ...row, ruleType }).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-rules"] });
                    });
                  }}
                />
              ),
            },
            {
              title: t("gateway.pattern", { defaultValue: "匹配" }),
              render: (_: unknown, row: RouteRule) => (
                <Input
                  defaultValue={row.ruleType === "condition" ? row.conditionJson : row.pattern}
                  onBlur={(event) => {
                    const value = event.target.value;
                    const next = row.ruleType === "condition"
                      ? { ...row, conditionJson: value }
                      : { ...row, pattern: value };
                    void upsertRouteRule(next).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-rules"] });
                    });
                  }}
                />
              ),
            },
            {
              title: t("gateway.targetModel", { defaultValue: "目标模型" }),
              render: (_: unknown, row: RouteRule) => (
                <Select
                  showSearch
                  allowClear
                  style={{ minWidth: 220 }}
                  value={row.targetModel || undefined}
                  options={modelOptions}
                  onChange={(value) => {
                    void upsertRouteRule({ ...row, targetModel: value ?? "" }).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-rules"] });
                    });
                  }}
                />
              ),
            },
            {
              title: t("gateway.enabled", { defaultValue: "启用" }),
              render: (_: unknown, row: RouteRule) => (
                <Switch
                  checked={row.enabled}
                  onChange={(enabled) => {
                    void upsertRouteRule({ ...row, enabled }).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-rules"] });
                    });
                  }}
                />
              ),
            },
            {
              title: "",
              render: (_: unknown, row: RouteRule) => (
                <Button
                  size="small"
                  danger
                  onClick={() => {
                    void deleteRouteRule(row.id).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["route-rules"] });
                    });
                  }}
                >
                  {t("common.delete", { defaultValue: "删除" })}
                </Button>
              ),
            },
          ]}
        />
      </Card>

      <RouteModesHelpDrawer
        open={modesHelpOpen}
        onClose={() => setModesHelpOpen(false)}
        tab={modesHelpTab}
        onTabChange={setModesHelpTab}
      />

      <Card size="small" title={t("proxy.recentRoutes")}>
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          dataSource={routesQuery.data ?? []}
          columns={[
            { title: t("gateway.reason", { defaultValue: "依据" }), dataIndex: "routeReason" },
            { title: t("gateway.requested", { defaultValue: "请求" }), dataIndex: "requestedModel" },
            { title: t("gateway.model", { defaultValue: "模型" }), dataIndex: "model" },
            { title: t("gateway.upstream", { defaultValue: "上游" }), dataIndex: "providerName" },
            { title: "HTTP", dataIndex: "statusCode" },
          ]}
        />
      </Card>
    </Space>
  );
}
