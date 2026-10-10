import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Badge,
  Button,
  Card,
  Input,
  InputNumber,
  Modal,
  Segmented,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  GatewayBindingsSummaryCard,
  GatewayLimitsCard,
  GatewayProfileToolbar,
  GatewayRouteLogsCard,
  GatewayRouteModesCard,
  RouteModesHelpDrawer,
  RouteRulesCard,
  RouteSimulatorDrawer,
  toCatalogModelSelectOption,
  type RouteHelpTab,
} from "@/components/proxy";
import AntigravityPage from "@/pages/AntigravityPage";
import KiroPage from "@/pages/KiroPage";
import { OnboardingTip } from "@/components/OnboardingTip";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { listModelPricing } from "@/services/usage";
import {
  createGatewayProfile,
  deleteGatewayProfile,
  getSmartGatewayStatus,
  listGatewayCatalogEntries,
  listGatewayProfiles,
  listGatewayUpstreams,
  listRouteModeUsageStats,
  listRouteModes,
  listRouteRules,
  listSmartGatewayBindings,
  renameGatewayProfile,
  rotateSmartGatewayApiKey,
  setSmartGatewayApiKey,
  setSmartGatewayPort,
  startSmartGateway,
  stopSmartGateway,
  updateGatewayProfileById,
  updateRouteMode,
} from "@/services/providers";
import type { GatewayProfile } from "@/types/backend";

const SHARED_PROFILE_ID = "gprof_shared";

const { Text, Paragraph } = Typography;

type SnippetKind = "sdk" | "openai" | "anthropic" | "responses";

function errMsg(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  return String(error ?? "未知错误");
}

type ReverseGatewayTab = "antigravity" | "kiro";

function ReverseGatewayBody({ tab }: { tab: ReverseGatewayTab }) {
  switch (tab) {
    case "antigravity":
      return <AntigravityPage embedded />;
    case "kiro":
      return <KiroPage />;
    default: {
      const _exhaustive: never = tab;
      return _exhaustive;
    }
  }
}

export default function GatewayPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const tab = usePagePreferencesStore((state) => state.gatewayTab);
  const setTab = usePagePreferencesStore((state) => state.setGatewayTab);
  const reverseTab = usePagePreferencesStore((state) => state.gatewayReverseTab);
  const setReverseTab = usePagePreferencesStore((state) => state.setGatewayReverseTab);
  const section = usePagePreferencesStore((state) => state.gatewaySection);
  const setSection = usePagePreferencesStore((state) => state.setGatewaySection);
  const profileId = usePagePreferencesStore((state) => state.gatewayProfileId);
  const setProfileId = usePagePreferencesStore((state) => state.setGatewayProfileId);
  const snippetVisible = usePagePreferencesStore((state) => state.gatewaySnippetVisible);
  const setSnippetVisible = usePagePreferencesStore((state) => state.setGatewaySnippetVisible);

  const [port, setPort] = useState(15828);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [snippetKind, setSnippetKind] = useState<SnippetKind>("sdk");
  const [modesHelpOpen, setModesHelpOpen] = useState(false);
  const [modesHelpTab, setModesHelpTab] = useState<RouteHelpTab>("guide");
  const [simulateOpen, setSimulateOpen] = useState(false);
  const [profileModal, setProfileModal] = useState<{
    mode: "create" | "clone" | "rename";
    name: string;
  } | null>(null);
  const [profileBusy, setProfileBusy] = useState(false);

  useEffect(() => {
    if (section === "upstreams") {
      setSection("service");
    }
  }, [section, setSection]);

  const effectiveSection = section === "upstreams" ? "service" : section;
  const smartTab = tab === "smart";
  const serviceSection = smartTab && effectiveSection === "service";
  const routingSection = smartTab && effectiveSection === "routing";

  const statusQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    enabled: serviceSection,
  });
  const bindingsQuery = useQuery({
    queryKey: ["smart-gateway-bindings"],
    queryFn: listSmartGatewayBindings,
    enabled: serviceSection || routingSection,
  });
  const upstreamsQuery = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
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
  const statsQuery = useQuery({
    queryKey: ["route-mode-usage"],
    queryFn: listRouteModeUsageStats,
    staleTime: 60_000,
    enabled: routingSection,
  });
  const catalogQuery = useQuery({
    queryKey: ["gateway-catalog-entries", "claude_code"],
    queryFn: () => listGatewayCatalogEntries("claude_code"),
    enabled: routingSection || serviceSection,
  });
  const pricingQuery = useQuery({
    queryKey: ["model-pricing"],
    queryFn: listModelPricing,
    enabled: routingSection || serviceSection,
  });

  const status = statusQuery.data;
  const apiKey = apiKeyDraft ?? status?.apiKey ?? "";
  const listenUrl = (status?.baseUrl ?? `http://127.0.0.1:${status?.port ?? port}`).replace(/\/+$/, "");
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
        return `curl -s ${listenUrl}/v1/messages \\\n -H "x-api-key: ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"auto","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'`;
      case "responses":
        return `curl -s ${listenUrl}/v1/responses \\\n -H "Authorization: Bearer ${key}" \\\n  -H "x-ai-switcher-target: codex" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"auto","input":"hi"}'`;
      case "openai":
        return `curl -s ${listenUrl}/v1/chat/completions \\\n -H "Authorization: Bearer ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"auto","messages":[{"role":"user","content":"hi"}]}'`;
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

  const bound = new Set(
    (bindingsQuery.data ?? [])
      .filter((item) => item.mode === "gateway")
      .map((item) => item.targetApp),
  );

  const profileUsers = (bindingsQuery.data ?? [])
    .filter(
      (item) =>
        item.mode === "gateway" && (item.profileId || SHARED_PROFILE_ID) === profileId,
    )
    .map((item) => t(`workspace.${item.targetApp}`));

  const profiles = useMemo(() => profilesQuery.data ?? [], [profilesQuery.data]);

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
        ? `${price.inputPricePerMillion}/${price.outputPricePerMillion}`
        : "";
      return {
        ...toCatalogModelSelectOption(entry, extra),
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
        queryClient.setQueryData<GatewayProfile[]>(["gateway-profiles"], (current) => {
          const rows = current ?? [];
          if (rows.some((profile) => profile.id === created.id)) {
            return rows;
          }
          return [...rows, created];
        });
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
    void updateRouteMode(id, patch, profileId)
      .then(() => {
        void queryClient.invalidateQueries({ queryKey: ["route-modes", profileId] });
      })
      .catch((error) => {
        void message.error(errMsg(error));
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
        <Segmented
          size="small"
          value={reverseTab}
          onChange={(value) => {
            if (value === "antigravity" || value === "kiro") setReverseTab(value);
          }}
          options={[
            { value: "antigravity", label: t("gateway.reverseAntigravity") },
            { value: "kiro", label: t("gateway.reverseKiro") },
          ]}
        />
        <ReverseGatewayBody tab={reverseTab} />
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
        value={effectiveSection}
        onChange={(value) => {
          switch (value) {
            case "service":
            case "routing":
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
          { value: "logs", label: t("gateway.sectionLogs", { defaultValue: "最近路由" }) },
        ]}
      />

      <OnboardingTip
        tipKey="gateway_catalog_refresh"
        message={t("gateway.catalogRefreshHint", {
          defaultValue: "Claude Code / Desktop / Codex 通过 /v1/models 拉目录，变更后需重启 Agent 才刷新。OpenCode / Pi / DSH / Cline 会立即重写配置。",
        })}
      />

      {effectiveSection === "service" ? (
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

          {/* Read-only Agent Connections Summary with single-point management jump */}
          <GatewayBindingsSummaryCard
            bindings={bindingsQuery.data ?? []}
            profiles={profiles}
            upstreams={upstreamsQuery.data ?? []}
            loading={bindingsQuery.isLoading}
          />

          <GatewayLimitsCard modelOptions={modelOptions} />
        </>
      ) : null}

      {effectiveSection === "routing" ? (
        <>
          <GatewayProfileToolbar
            profileId={profileId}
            profiles={profiles}
            editingProfile={editingProfile}
            profileUsers={profileUsers}
            profileBusy={profileBusy}
            hasBindings={bindingsQuery.isSuccess}
            onSelectProfile={(id) => setProfileId(id)}
            onCreateProfile={() => setProfileModal({ mode: "create", name: "" })}
            onCloneProfile={() =>
              setProfileModal({
                mode: "clone",
                name: `${editingProfile?.name || t("gateway.profileDefault", { defaultValue: "默认" })} 副本`,
              })
            }
            onRenameProfile={() =>
              setProfileModal({
                mode: "rename",
                name: editingProfile?.name ?? "",
              })
            }
            onDeleteProfile={() => void handleDeleteProfile()}
            onUpdateFallbackMode={async (value) => {
              try {
                await updateGatewayProfileById(profileId, { fallbackMode: value });
                await refreshProfiles();
              } catch (error) {
                void message.error(errMsg(error));
                await refreshProfiles();
              }
            }}
          />

          <GatewayRouteModesCard
            modes={modesQuery.data ?? []}
            loading={modesQuery.isLoading}
            modelOptions={modelOptions}
            catalog={catalogQuery.data ?? []}
            usageStats={statsQuery.data ?? []}
            onPatchMode={patchMode}
            onSimulate={() => setSimulateOpen(true)}
            onOpenHelp={(targetTab) => {
              setModesHelpTab(targetTab);
              setModesHelpOpen(true);
            }}
          />

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

      {/* Enhanced Route Logs and Request Diagnostics */}
      {effectiveSection === "logs" ? <GatewayRouteLogsCard /> : null}
    </Space>
  );
}
