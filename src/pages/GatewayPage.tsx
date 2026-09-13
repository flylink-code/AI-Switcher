import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Badge,
  Button,
  Card,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
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
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ExperimentOutlined from "@ant-design/icons/es/icons/ExperimentOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  GatewayUpstreamPanel,
  GatewayLimitsCard,
  RouteModesHelpButton,
  RouteModesHelpDrawer,
  RouteModesTutorialButton,
  RouteRulesCard,
  RouteSimulatorDrawer,
  type RouteHelpTab,
} from "@/components/proxy";
import AntigravityPage from "@/pages/AntigravityPage";
import { BIND_TARGETS } from "@/components/antigravity";
import { OnboardingTip } from "@/components/OnboardingTip";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { listModelPricing } from "@/services/usage";
import {
  bindSmartGateway,
  createGatewayProfile,
  deleteGatewayProfile,
  getSmartGatewayStatus,
  listGatewayCatalogEntries,
  listGatewayProfiles,
  listGatewayRouteLogs,
  listRouteModeUsageStats,
  listRouteModes,
  listRouteRules,
  listSmartGatewayBindings,
  renameGatewayProfile,
  setGatewayBindingProfile,
  setSmartGatewayPort,
  setSmartGatewayApiKey,
  rotateSmartGatewayApiKey,
  startSmartGateway,
  stopSmartGateway,
  unbindSmartGateway,
  updateRouteMode,
} from "@/services/providers";
import type { ProviderTarget, RouteMode, GatewayCatalogModelOption, GatewayRouteLog } from "@/types/backend";
import { formatCompactNumber } from "@/utils/formatCompact";

const SHARED_PROFILE_ID = "gprof_shared";

const { Text, Paragraph } = Typography;

type SnippetKind = "sdk" | "openai" | "anthropic" | "responses";

const ROUTE_LOG_PAGE_SIZE = 20;

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

function routeModeColor(mode?: string | null): string {
  switch (mode) {
    case "long_context":
      return "purple";
    case "think":
      return "blue";
    case "plan":
      return "cyan";
    case "edit":
      return "orange";
    case "background":
      return "geekblue";
    case "web_search":
      return "green";
    case "vision":
      return "magenta";
    case "image_gen":
      return "gold";
    case "explicit_model":
      return "processing";
    case "rule":
      return "lime";
    case "default":
      return "default";
    default:
      return "default";
  }
}

function classifyRouteReasonMode(reason: string): string | null {
  if (reason === "explicit_model") return "explicit_model";
  if (reason === "rule" || reason.startsWith("rule:")) return "rule";
  if (reason.includes("规划") || reason === "plan") return "plan";
  if (reason.includes("改内容") || reason === "edit") return "edit";
  if (reason.includes("后台") || reason === "background" || reason === "role_subagent") {
    return "background";
  }
  if (reason.includes("思考") || reason === "think") return "think";
  if (reason.includes("长上下文") || reason === "long_context") return "long_context";
  if (reason.includes("联网") || reason === "web_search") return "web_search";
  if (reason.includes("视觉") || reason === "vision") return "vision";
  if (reason.includes("图像") || reason === "image_gen") return "image_gen";
  if (reason.includes("默认") || reason === "auto" || reason === "profile_default") {
    return "default";
  }
  return null;
}

function routeLogModeKey(row: GatewayRouteLog): string {
  const mode = row.routeMode?.trim();
  if (mode) return mode;
  return classifyRouteReasonMode(row.routeReason?.trim() ?? "") ?? "";
}

function EllipsisText({ value }: { value?: string | null }) {
  const text = value?.trim() || "—";
  return (
    <Text ellipsis={{ tooltip: text }} style={{ maxWidth: "100%", margin: 0 }}>
      {text}
    </Text>
  );
}

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
  const section = usePagePreferencesStore((state) => state.gatewaySection);
  const setSection = usePagePreferencesStore((state) => state.setGatewaySection);
  const profileId = usePagePreferencesStore((state) => state.gatewayProfileId);
  const setProfileId = usePagePreferencesStore((state) => state.setGatewayProfileId);
  const snippetVisible = usePagePreferencesStore((state) => state.gatewaySnippetVisible);
  const setSnippetVisible = usePagePreferencesStore((state) => state.setGatewaySnippetVisible);
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);
  const [port, setPort] = useState(15828);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [routeLogPage, setRouteLogPage] = useState(0);
  const [snippetKind, setSnippetKind] = useState<SnippetKind>("sdk");
  const [modesHelpOpen, setModesHelpOpen] = useState(false);
  const [modesHelpTab, setModesHelpTab] = useState<RouteHelpTab>("guide");
  const [simulateOpen, setSimulateOpen] = useState(false);
  const [profileModal, setProfileModal] = useState<{
    mode: "create" | "clone" | "rename";
    name: string;
  } | null>(null);
  const [profileBusy, setProfileBusy] = useState(false);

  const smartTab = tab === "smart";
  const serviceSection = smartTab && section === "service";
  const routingSection = smartTab && section === "routing";
  const upstreamsSection = smartTab && section === "upstreams";
  const logsSection = smartTab && section === "logs";

  const statusQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    enabled: serviceSection,
  });
  const bindingsQuery = useQuery({
    queryKey: ["smart-gateway-bindings"],
    queryFn: listSmartGatewayBindings,
    enabled: serviceSection,
  });
  const profilesQuery = useQuery({
    queryKey: ["gateway-profiles"],
    queryFn: listGatewayProfiles,
    enabled: serviceSection || routingSection,
  });
  const modesQuery = useQuery({
    queryKey: ["route-modes", profileId],
    queryFn: () => listRouteModes(profileId),
    enabled: routingSection,
  });
  const rulesQuery = useQuery({
    queryKey: ["route-rules", profileId],
    queryFn: () => listRouteRules(profileId),
    enabled: routingSection,
  });
  const routesQuery = useQuery({
    queryKey: ["gateway-route-logs-all", routeLogPage],
    queryFn: () => listGatewayRouteLogs(null, ROUTE_LOG_PAGE_SIZE, routeLogPage * ROUTE_LOG_PAGE_SIZE),
    enabled: logsSection,
  });
  const statsQuery = useQuery({
    queryKey: ["route-mode-usage"],
    queryFn: listRouteModeUsageStats,
    staleTime: 60_000,
    enabled: routingSection,
  });
  const catalogQuery = useQuery({
    queryKey: ["gateway-catalog-entries", "claude_code"],
    queryFn: () => listGatewayCatalogEntries("claude_code"),
    enabled: routingSection || upstreamsSection,
  });
  const pricingQuery = useQuery({
    queryKey: ["model-pricing"],
    queryFn: listModelPricing,
    enabled: routingSection || upstreamsSection,
  });

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
  const profiles = profilesQuery.data ?? [];
  const profileOptions = profiles.map((profile) => ({
    value: profile.id,
    label: profile.id === SHARED_PROFILE_ID
      ? t("gateway.profileDefault", { defaultValue: profile.name || "默认" })
      : profile.name,
  }));
  const editingProfile = profiles.find((profile) => profile.id === profileId)
    ?? profiles.find((profile) => profile.id === SHARED_PROFILE_ID);
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

  useEffect(() => {
    if (profiles.length === 0) return;
    if (!profiles.some((profile) => profile.id === profileId)) {
      setProfileId(SHARED_PROFILE_ID);
    }
  }, [profiles, profileId, setProfileId]);

  const refreshProfiles = async () => {
    await queryClient.invalidateQueries({ queryKey: ["gateway-profiles"] });
    await queryClient.invalidateQueries({ queryKey: ["route-modes"] });
    await queryClient.invalidateQueries({ queryKey: ["route-rules"] });
    await queryClient.invalidateQueries({ queryKey: ["smart-gateway-bindings"] });
  };

  const handleProfileModalOk = async () => {
    if (!profileModal) return;
    const name = profileModal.name.trim();
    if (!name) {
      void message.error(t("gateway.profileNameRequired", { defaultValue: "请填写档案名称" }));
      return;
    }
    setProfileBusy(true);
    try {
      if (profileModal.mode === "rename") {
        await renameGatewayProfile(profileId, name);
      } else {
        const created = await createGatewayProfile(
          name,
          profileModal.mode === "clone" ? profileId : SHARED_PROFILE_ID,
        );
        setProfileId(created.id);
      }
      setProfileModal(null);
      await refreshProfiles();
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setProfileBusy(false);
    }
  };

  const handleDeleteProfile = async () => {
    if (profileId === SHARED_PROFILE_ID) return;
    setProfileBusy(true);
    try {
      await deleteGatewayProfile(profileId);
      setProfileId(SHARED_PROFILE_ID);
      await refreshProfiles();
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setProfileBusy(false);
    }
  };

  const patchMode = (id: string, patch: Parameters<typeof updateRouteMode>[1]) => {
    void updateRouteMode(id, patch, profileId).then(() => {
      void queryClient.invalidateQueries({ queryKey: ["route-modes", profileId] });
    });
  };

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
      <Segmented
        size="small"
        value={section}
        onChange={(value) => {
          switch (value) {
            case "service":
            case "routing":
            case "upstreams":
            case "logs":
              setSection(value);
              break;
            default:
              break;
          }
        }}
        options={[
          { value: "service", label: t("gateway.sectionService", { defaultValue: "服务与绑定" }) },
          { value: "routing", label: t("gateway.sectionRouting", { defaultValue: "路由与规则" }) },
          { value: "upstreams", label: t("gateway.sectionUpstreams", { defaultValue: "上游与限额" }) },
          { value: "logs", label: t("gateway.sectionLogs", { defaultValue: "最近路由" }) },
        ]}
      />

      <OnboardingTip
        tipKey="gateway_catalog_refresh"
        message={t("gateway.catalogRefreshHint", {
          defaultValue: "Claude Code / Desktop / Codex 通过 /v1/models 拉目录，变更后需重启 Agent 才刷新。OpenCode / Pi / DSH / Cline 会立即重写配置。",
        })}
      />

      {section === "service" ? (
      <>
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

          <OnboardingTip
            tipKey="gateway_external"
            message={t("gateway.externalHint", {
              defaultValue:
                "对外 API Key 已自动生成，自定义 Agent 直接复制即可，不必绑定。把 Base URL 指到访问地址，模型用 auto。已绑定应用仍用各自入口令牌。",
            })}
          />

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

          <OnboardingTip
            tipKey="gateway_endpoints"
            message={t("gateway.externalEndpoints", {
              defaultValue:
                "协议：Anthropic POST /v1/messages；OpenAI Chat POST /v1/chat/completions；Responses POST /v1/responses；目录 GET /v1/models；绘图 POST /v1/images/generations。OpenAI 风格模型名可加请求头 x-ai-switcher-target: codex。",
            })}
          />

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
          {snippetVisible ? (
            <Paragraph style={{ marginBottom: 0 }}>
              <pre style={{ margin: 0, padding: 8, borderRadius: 6, background: "var(--ant-color-bg-layout, #f5f5f5)", whiteSpace: "pre-wrap", fontSize: 12 }}>
                {curlSnippet}
              </pre>
              <Button type="link" size="small" style={{ padding: 0, marginTop: 4 }} onClick={() => setSnippetVisible(false)}>
                {t("antigravity.hideTestCommand", { defaultValue: "收起测试命令" })}
              </Button>
            </Paragraph>
          ) : (
            <Button type="link" size="small" style={{ padding: 0 }} onClick={() => setSnippetVisible(true)}>
              {t("antigravity.viewTestCommand", { defaultValue: "查看测试命令" })}
            </Button>
          )}
        </Space>
        {status?.lastError ? (
          <Alert style={{ marginTop: 12 }} type="error" showIcon message={status.lastError} />
        ) : null}
      </Card>

      <Card size="small" title={t("gateway.bindApps", { defaultValue: "绑定应用" })}>
        <OnboardingTip
          tipKey="gateway_bind"
          message={t("gateway.bindAppsHint", { defaultValue: "绑定后写入指向 127.0.0.1:15828 的供应商卡，并设为当前。自定义 Agent 用上方 API Key 即可，不必绑定。" })}
          style={{ marginBottom: 12 }}
        />
        <Table
          size="small"
          pagination={false}
          rowKey="target"
          dataSource={BIND_TARGETS.filter((target) => visibleAgents.includes(target)).map((target) => ({
            target,
            bound: bound.has(target),
            profileId: (bindingsQuery.data ?? []).find((item) => item.targetApp === target)?.profileId
              ?? SHARED_PROFILE_ID,
          }))}
          columns={[
            {
              title: t("gateway.bindAgent", { defaultValue: "应用" }),
              dataIndex: "target",
              render: (target: ProviderTarget) => t(`workspace.${target}`),
            },
            {
              title: t("gateway.bound", { defaultValue: "绑定" }),
              dataIndex: "bound",
              width: 140,
              render: (isBound: boolean, row: { target: ProviderTarget }) => (
                <Button
                  size="small"
                  icon={isBound ? <CheckOutlined /> : <LinkOutlined />}
                  loading={bindMutation.isPending && bindMutation.variables === row.target}
                  onClick={() => bindMutation.mutate(row.target)}
                >
                  {isBound
                    ? t("antigravity.bound", { defaultValue: "已绑定" })
                    : t("gateway.bind", { defaultValue: "绑定" })}
                </Button>
              ),
            },
            {
              title: t("gateway.currentProfile", { defaultValue: "当前档案" }),
              dataIndex: "profileId",
              render: (value: string, row: { target: ProviderTarget; bound: boolean }) => (
                <Select
                  size="small"
                  style={{ minWidth: 160 }}
                  disabled={!row.bound}
                  value={row.bound ? value : undefined}
                  placeholder={t("gateway.profileUnbound", { defaultValue: "未绑定" })}
                  options={profileOptions}
                  onChange={(next) => {
                    void setGatewayBindingProfile(row.target, String(next)).then(() => {
                      void queryClient.invalidateQueries({ queryKey: ["smart-gateway-bindings"] });
                      void queryClient.invalidateQueries({ queryKey: ["providers"] });
                    }).catch((error) => {
                      void message.error(errMsg(error));
                    });
                  }}
                />
              ),
            },
          ]}
        />
      </Card>
      </>
      ) : null}

      {section === "upstreams" ? (
      <>
      <GatewayUpstreamPanel allowlistTarget="claude_code" />

      <GatewayLimitsCard modelOptions={modelOptions} />
      </>
      ) : null}

      {section === "routing" ? (
      <>
      <Card size="small">
        <Space wrap style={{ width: "100%", justifyContent: "space-between" }}>
          <Space wrap>
            <Text type="secondary">{t("gateway.profileToolbar", { defaultValue: "正在编辑" })}</Text>
            <Select
              size="small"
              style={{ minWidth: 180 }}
              value={editingProfile?.id ?? profileId}
              options={profileOptions}
              onChange={(value) => setProfileId(String(value))}
            />
            <Button
              size="small"
              icon={<PlusOutlined />}
              onClick={() => setProfileModal({ mode: "create", name: "" })}
            >
              {t("gateway.profileCreate", { defaultValue: "新建" })}
            </Button>
            <Button
              size="small"
              icon={<CopyOutlined />}
              onClick={() => setProfileModal({
                mode: "clone",
                name: `${editingProfile?.name || t("gateway.profileDefault", { defaultValue: "默认" })} 副本`,
              })}
            >
              {t("gateway.profileClone", { defaultValue: "复制" })}
            </Button>
            <Button
              size="small"
              icon={<EditOutlined />}
              onClick={() => setProfileModal({
                mode: "rename",
                name: editingProfile?.name ?? "",
              })}
            >
              {t("gateway.profileRename", { defaultValue: "重命名" })}
            </Button>
            <Popconfirm
              title={t("gateway.profileDeleteConfirm", { defaultValue: "删除这套档案？已绑定的 Agent 会回到默认档案。" })}
              disabled={profileId === SHARED_PROFILE_ID}
              onConfirm={() => void handleDeleteProfile()}
            >
              <Button
                size="small"
                danger
                icon={<DeleteOutlined />}
                disabled={profileId === SHARED_PROFILE_ID}
                loading={profileBusy}
              >
                {t("gateway.profileDelete", { defaultValue: "删除" })}
              </Button>
            </Popconfirm>
          </Space>
        </Space>
        <Text type="secondary" style={{ display: "block", marginTop: 8 }}>
          {t("gateway.profileToolbarHint", {
            defaultValue: "这里改的是档案内容，和各 Agent Auto 卡选用哪一套无关。",
          })}
        </Text>
      </Card>
      <Card
        size="small"
        title={t("gateway.routeModes", { defaultValue: "路由模式" })}
        extra={
          <Space size={8}>
            <Button
              size="small"
              icon={<ExperimentOutlined />}
              onClick={() => setSimulateOpen(true)}
            >
              {t("gateway.simulate", { defaultValue: "试跑" })}
            </Button>
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
                    void patchMode(row.id, { enabled: checked });
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
                    void patchMode(row.id, { model: value ?? "" });
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
                      void patchMode(row.id, { thinkingConfigJson: JSON.stringify(thinking) });
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
                    void patchMode(row.id, { fallbackModels: value.slice(0, 3) });
                  }}
                />
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
                        void patchMode(row.id, { threshold: value ?? 0 });
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

      <RouteRulesCard
        rules={rulesQuery.data ?? []}
        loading={rulesQuery.isLoading}
        modelOptions={modelOptions}
        profileId={profileId}
      />
      </>
      ) : null}

      <RouteModesHelpDrawer
        open={modesHelpOpen}
        onClose={() => setModesHelpOpen(false)}
        tab={modesHelpTab}
        onTabChange={setModesHelpTab}
      />
      <RouteSimulatorDrawer
        open={simulateOpen}
        onClose={() => setSimulateOpen(false)}
        modelOptions={modelOptions}
        profileId={profileId}
      />
      <Modal
        open={profileModal != null}
        title={
          profileModal?.mode === "rename"
            ? t("gateway.profileRename", { defaultValue: "重命名" })
            : profileModal?.mode === "clone"
              ? t("gateway.profileClone", { defaultValue: "复制" })
              : t("gateway.profileCreate", { defaultValue: "新建" })
        }
        confirmLoading={profileBusy}
        onOk={() => void handleProfileModalOk()}
        onCancel={() => setProfileModal(null)}
      >
        <Input
          autoFocus
          value={profileModal?.name ?? ""}
          placeholder={t("gateway.profileNamePlaceholder", { defaultValue: "档案名称" })}
          onChange={(event) => {
            setProfileModal((current) => current ? { ...current, name: event.target.value } : current);
          }}
          onPressEnter={() => void handleProfileModalOk()}
        />
      </Modal>

      {section === "logs" ? (
      <Card size="small" title={t("proxy.recentRoutes")} className="gateway-route-logs">
        <Table
          size="small"
          rowKey="id"
          tableLayout="fixed"
          scroll={{ x: 1080 }}
          dataSource={routesQuery.data?.data ?? []}
          loading={routesQuery.isPending && !routesQuery.data}
          pagination={{
            current: (routesQuery.data?.page ?? routeLogPage) + 1,
            pageSize: routesQuery.data?.pageSize ?? ROUTE_LOG_PAGE_SIZE,
            total: routesQuery.data?.total ?? 0,
            showSizeChanger: false,
            onChange: (page) => setRouteLogPage(page - 1),
          }}
          columns={[
            {
              title: t("gateway.time", { defaultValue: "时间" }),
              dataIndex: "createdAt",
              width: 96,
              render: (value: number) => new Date(value).toLocaleTimeString(),
            },
            {
              title: t("gateway.reason", { defaultValue: "依据" }),
              width: 128,
              ellipsis: true,
              render: (_: unknown, row: GatewayRouteLog) => {
                const modeKey = routeLogModeKey(row);
                const label = modeKey
                  ? t(`gateway.modes.${modeKey}`, { defaultValue: modeKey })
                  : (row.routeReason?.trim() || "—");
                const detail = row.routeReason?.trim() || label;
                return (
                  <div className="gateway-route-logs__reason">
                    <Tooltip title={detail}>
                      <Tag
                        color={routeModeColor(row.routeMode || modeKey)}
                        className="gateway-route-logs__tag"
                      >
                        {label}
                      </Tag>
                    </Tooltip>
                  </div>
                );
              },
            },
            {
              title: t("gateway.requested", { defaultValue: "请求" }),
              dataIndex: "requestedModel",
              ellipsis: true,
              width: 180,
              render: (value: string | null) => <EllipsisText value={value} />,
            },
            {
              title: t("gateway.model", { defaultValue: "模型" }),
              dataIndex: "model",
              ellipsis: true,
              width: 160,
              render: (value: string | null) => <EllipsisText value={value} />,
            },
            {
              title: t("gateway.upstream", { defaultValue: "上游" }),
              dataIndex: "providerName",
              ellipsis: true,
              width: 140,
              render: (value: string | null) => <EllipsisText value={value} />,
            },
            {
              title: t("gateway.duration", { defaultValue: "耗时" }),
              dataIndex: "durationMs",
              width: 88,
              render: (value: number) => `${value}ms`,
            },
            {
              title: "Token",
              width: 110,
              render: (_: unknown, row: GatewayRouteLog) => {
                const input = row.inputTokens + row.cacheReadInputTokens + row.cacheCreationInputTokens;
                return `${formatCompactNumber(input)} / ${formatCompactNumber(row.outputTokens)}`;
              },
            },
            {
              title: t("gateway.cost", { defaultValue: "费用" }),
              dataIndex: "estimatedCost",
              width: 88,
              render: (value: number) => `$${Number(value ?? 0).toFixed(4)}`,
            },
            { title: "HTTP", dataIndex: "statusCode", width: 64 },
          ]}
        />
      </Card>
      ) : null}
    </Space>
  );
}
