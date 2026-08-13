import { useEffect, useState } from "react";
import {
  Button,
  Card,
  ConfigProvider,
  Empty,
  Popover,
  Segmented,
  Skeleton,
  Space,
  Tag,
  Typography,
  message,
  theme,
} from "antd";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import {
  ensureAntigravityProvider,
  getAntigravityGatewayStatus,
  importAntigravityAccounts,
  listAntigravityAccounts,
  listAntigravityModels,
  refreshAntigravityQuotas,
  removeAntigravityAccount,
  setAntigravityActiveAccount,
  setAntigravityGatewayApiKey,
  setAntigravityGatewayPort,
  setAntigravityOutboundProxy,
  startAntigravityGateway,
  startAntigravityOauthLogin,
  stopAntigravityGateway,
  listProviders,
} from "@/services/api";
import type { AntigravityAccountPublic, AntigravityCatalogModel, AntigravityGatewayStatus, ProviderTarget } from "@/types/backend";
import { usageSourceSegmentLabel } from "@/components/UsageSourceIcons";
import {
  AccountPoolOverview,
  AccountCard,
  GatewayCard,
  BindAppsCard,
  ImportAccountsModal,
  BIND_TARGETS,
} from "@/components/antigravity";

const { Text } = Typography;

const ANTIGRAVITY_QUOTA_REFRESH_MS = 5 * 60_000;
const ANTIGRAVITY_QUOTA_REFRESH_EVENT = "antigravity-quota-refreshed";

function errMsg(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (error && typeof error === "object" && "message" in error) {
    const msg = (error as { message?: unknown }).message;
    if (typeof msg === "string" && msg.trim()) return msg;
  }
  return String(error ?? "未知错误");
}

export default function AntigravityPage() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const queryClient = useQueryClient();

  const [importModalOpen, setImportModalOpen] = useState(false);
  const [bindingTarget, setBindingTarget] = useState<ProviderTarget | null>(null);
  const [actionAccountId, setActionAccountId] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState("antigravity");

  const accountsQuery = useQuery({
    queryKey: ["antigravity-accounts"],
    queryFn: listAntigravityAccounts,
  });

  const statusQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 5_000,
  });

  const modelsQuery = useQuery({
    queryKey: ["antigravity-models"],
    queryFn: listAntigravityModels,
  });

  // 各应用是否已有内建 Antigravity 供应商（用于「已添加」标记）
  const boundProvidersQuery = useQuery({
    queryKey: ["antigravity-bound-providers"],
    queryFn: async () => {
      const entries = await Promise.all(
        BIND_TARGETS.map(async (target) => {
          const providers = await listProviders(target);
          return [
            target,
            providers.some((provider) => provider.providerKind === "antigravity"),
          ] as const;
        }),
      );
      return new Map<ProviderTarget, boolean>(entries);
    },
  });

  const accounts = accountsQuery.data ?? [];
  const status = statusQuery.data;
  const models = modelsQuery.data ?? [];

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["antigravity-accounts"] }),
      queryClient.invalidateQueries({ queryKey: ["antigravity-gateway"] }),
      queryClient.invalidateQueries({ queryKey: ["antigravity-models"] }),
    ]);
  };

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen(ANTIGRAVITY_QUOTA_REFRESH_EVENT, () => {
      void refresh();
    })
      .then((disposeListener) => {
        if (disposed) disposeListener();
        else unlisten = disposeListener;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [queryClient]);

  // While this page is open, refresh ALL account quotas on enter + on an interval.
  // Listing alone only re-reads the local store and will not hit Cloud Code.
  useEffect(() => {
    if (accounts.length === 0) return;

    let cancelled = false;
    let inFlight = false;

    const refreshAllQuotas = async () => {
      if (cancelled || inFlight) return;
      inFlight = true;
      try {
        await refreshAntigravityQuotas();
        if (!cancelled) {
          await queryClient.invalidateQueries({ queryKey: ["antigravity-accounts"] });
          await queryClient.invalidateQueries({ queryKey: ["antigravity-gateway"] });
          await queryClient.invalidateQueries({ queryKey: ["antigravity-models"] });
        }
      } catch {
        // Keep quiet for background refresh; manual button still surfaces errors.
      } finally {
        inFlight = false;
      }
    };

    void refreshAllQuotas();
    const timer = window.setInterval(() => {
      void refreshAllQuotas();
    }, ANTIGRAVITY_QUOTA_REFRESH_MS);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [accounts.length, queryClient]);

  const oauthMutation = useMutation({
    mutationFn: startAntigravityOauthLogin,
    onSuccess: async (account) => {
      message.success(t("antigravity.oauthSuccess", { email: account.email }));
      await refresh();
    },
    onError: (error: unknown) => {
      message.error(errMsg(error), 10);
    },
  });

  const importMutation = useMutation({
    mutationFn: (importJson: string) => importAntigravityAccounts(importJson),
    onSuccess: async (count) => {
      message.success(t("antigravity.importSuccess", { count }));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const startMutation = useMutation({
    mutationFn: async ({
      port,
      apiKey,
      outboundMode,
      outboundUrl,
    }: {
      port: number;
      apiKey?: string;
      outboundMode?: string;
      outboundUrl?: string;
    }) => {
      if (port != null) await setAntigravityGatewayPort(port);
      if (apiKey != null && apiKey.trim()) {
        await setAntigravityGatewayApiKey(apiKey.trim());
      }
      if (outboundMode != null || outboundUrl != null) {
        await setAntigravityOutboundProxy(
          outboundMode as "direct" | "system" | "custom",
          outboundUrl || "socks5://127.0.0.1:17891"
        );
      }
      return startAntigravityGateway(port);
    },
    onSuccess: async () => {
      message.success(t("antigravity.started"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const stopMutation = useMutation({
    mutationFn: stopAntigravityGateway,
    onSuccess: async () => {
      message.success(t("antigravity.stopped"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const outboundMutation = useMutation({
    mutationFn: ({ mode, url }: { mode: "direct" | "system" | "custom"; url: string }) =>
      setAntigravityOutboundProxy(mode, url),
    onSuccess: async () => {
      message.success(t("antigravity.outboundSaved"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const ensureMutation = useMutation({
    mutationFn: (target: ProviderTarget) => ensureAntigravityProvider(target),
    onMutate: (target) => setBindingTarget(target),
    onSuccess: async (_provider, target) => {
      message.success(
        t("antigravity.providerReadyForTarget", {
          target: t(`workspace.${target}`),
        }),
      );
      await refresh();
      await queryClient.invalidateQueries({
        queryKey: ["antigravity-bound-providers"],
      });
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error: unknown) => message.error(errMsg(error)),
    onSettled: () => setBindingTarget(null),
  });

  const quotaMutation = useMutation({
    mutationFn: refreshAntigravityQuotas,
    onSuccess: async () => {
      message.success(t("antigravity.quotaRefreshed"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error), 10),
  });

  const handleSetActive = async (id: string) => {
    setActionAccountId(id);
    try {
      await setAntigravityActiveAccount(id);
      try {
        await refreshAntigravityQuotas();
      } catch {
        // Active switch still succeeds even if a quota probe fails.
      }
      await refresh();
    } catch (error) {
      message.error(errMsg(error));
    } finally {
      setActionAccountId(null);
    }
  };

  const handleRemoveAccount = async (id: string) => {
    setActionAccountId(id);
    try {
      await removeAntigravityAccount(id);
      message.success(t("antigravity.removed"));
      await refresh();
    } catch (error) {
      message.error(errMsg(error));
    } finally {
      setActionAccountId(null);
    }
  };

  // 分类排序：Active 账号优先，其余按 Email 保持稳定性
  const sortedAccounts = [...accounts].sort((a, b) => {
    if (a.isActive && !b.isActive) return -1;
    if (!a.isActive && b.isActive) return 1;
    return a.email.localeCompare(b.email);
  });

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
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
        <Segmented<string>
          size="small"
          value={activeTab}
          onChange={setActiveTab}
          style={{
            border: `1px solid ${token.colorBorder}`,
            borderRadius: token.borderRadius,
            boxSizing: "border-box",
            alignSelf: "flex-start",
          }}
          options={[
            {
              value: "antigravity",
              label: usageSourceSegmentLabel(
                "antigravity",
                t("antigravity.tabAntigravity", { defaultValue: "Antigravity" }),
              ),
            },
          ]}
        />
      </ConfigProvider>

      {activeTab === "antigravity" && (
        <AntigravityContent
          t={t}
          accounts={accounts}
          sortedAccounts={sortedAccounts}
          status={status}
          models={models}
          accountsQuery={accountsQuery}
          oauthMutation={oauthMutation}
          quotaMutation={quotaMutation}
          importMutation={importMutation}
          startMutation={startMutation}
          stopMutation={stopMutation}
          outboundMutation={outboundMutation}
          ensureMutation={ensureMutation}
          boundProvidersQuery={boundProvidersQuery}
          bindingTarget={bindingTarget}
          actionAccountId={actionAccountId}
          importModalOpen={importModalOpen}
          setImportModalOpen={setImportModalOpen}
          refresh={refresh}
          handleSetActive={handleSetActive}
          handleRemoveAccount={handleRemoveAccount}
        />
      )}
    </div>
  );
}

interface AntigravityContentProps {
  t: (key: string, options?: Record<string, unknown>) => string;
  accounts: AntigravityAccountPublic[];
  sortedAccounts: AntigravityAccountPublic[];
  status: AntigravityGatewayStatus | undefined;
  models: AntigravityCatalogModel[];
  accountsQuery: { isLoading: boolean };
  oauthMutation: { isPending: boolean; mutate: () => void };
  quotaMutation: { isPending: boolean; mutate: () => void };
  importMutation: { isPending: boolean; mutateAsync: (json: string) => Promise<unknown> };
  startMutation: { isPending: boolean; mutateAsync: (args: { port: number; apiKey: string; outboundMode: string; outboundUrl: string }) => Promise<unknown> };
  stopMutation: { isPending: boolean; mutateAsync: () => Promise<unknown> };
  outboundMutation: { isPending: boolean; mutateAsync: (args: { mode: "direct" | "system" | "custom"; url: string }) => Promise<unknown> };
  ensureMutation: { mutate: (target: ProviderTarget) => void };
  boundProvidersQuery: { data?: Map<ProviderTarget, boolean> };
  bindingTarget: ProviderTarget | null;
  actionAccountId: string | null;
  importModalOpen: boolean;
  setImportModalOpen: (open: boolean) => void;
  refresh: () => Promise<void>;
  handleSetActive: (id: string) => Promise<void>;
  handleRemoveAccount: (id: string) => Promise<void>;
}

function AntigravityContent({
  t,
  accounts,
  sortedAccounts,
  status,
  models,
  accountsQuery,
  oauthMutation,
  quotaMutation,
  importMutation,
  startMutation,
  stopMutation,
  outboundMutation,
  ensureMutation,
  boundProvidersQuery,
  bindingTarget,
  actionAccountId,
  importModalOpen,
  setImportModalOpen,
  refresh,
  handleSetActive,
  handleRemoveAccount,
}: AntigravityContentProps) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      {/* Compact runtime summary + page actions */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          gap: 12,
        }}
      >
        <AccountPoolOverview accounts={accounts} status={status} />
        <Space wrap>
          <Popover
            trigger="click"
            placement="bottomRight"
            title={t("antigravity.howToAddTitle")}
            content={
              <div style={{ maxWidth: 360 }}>
                <p style={{ marginBottom: 8 }}>{t("antigravity.howToAddOauth")}</p>
                <p style={{ marginBottom: 8 }}>{t("antigravity.howToAddNotIde")}</p>
                <p style={{ marginBottom: 0 }}>{t("antigravity.howToAddJson")}</p>
              </div>
            }
          >
            <Button size="small" icon={<QuestionCircleOutlined />}>
              {t("antigravity.whyAccountMissing", { defaultValue: "为什么账号没有出现?" })}
            </Button>
          </Popover>
          <Button
            icon={<ReloadOutlined />}
            loading={quotaMutation.isPending}
            disabled={accounts.length === 0}
            onClick={() => quotaMutation.mutate()}
          >
            {t("antigravity.refreshQuota")}
          </Button>
          <Button icon={<ImportOutlined />} onClick={() => setImportModalOpen(true)}>
            {t("antigravity.import")}
          </Button>
          <Button
            type="primary"
            icon={<LoginOutlined />}
            loading={oauthMutation.isPending}
            onClick={() => oauthMutation.mutate()}
          >
            {oauthMutation.isPending
              ? t("antigravity.oauthWaiting")
              : t("antigravity.oauthLogin")}
          </Button>
        </Space>
      </div>

      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("antigravity.personalUseNotice")}
      </Text>

      {/* Account Pool */}
      <section>
        <Space align="center" style={{ marginBottom: 12 }}>
          <UserOutlined />
          <Text strong>{t("antigravity.accounts")}</Text>
          <Text type="secondary" style={{ fontSize: 13 }}>
            ({accounts.length})
          </Text>
        </Space>

        {accountsQuery.isLoading ? (
          <Skeleton active paragraph={{ rows: 3 }} />
        ) : accounts.length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t("antigravity.emptyAccounts")}
          >
            <Button
              type="primary"
              icon={<LoginOutlined />}
              loading={oauthMutation.isPending}
              onClick={() => oauthMutation.mutate()}
            >
              {t("antigravity.oauthLogin")}
            </Button>
          </Empty>
        ) : (
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(340px, 1fr))",
              gap: 12,
            }}
          >
            {sortedAccounts.map((account) => (
              <AccountCard
                key={account.id}
                account={account}
                onSetActive={handleSetActive}
                onRemove={handleRemoveAccount}
                isPending={actionAccountId === account.id}
              />
            ))}
          </div>
        )}
      </section>

      {/* Gateway Controls */}
      <GatewayCard
        status={status}
        models={models}
        onStartGateway={async (port, apiKey, outboundMode, outboundUrl) => {
          await startMutation.mutateAsync({ port, apiKey: apiKey ?? "", outboundMode: outboundMode ?? "", outboundUrl: outboundUrl ?? "" });
        }}
        onStopGateway={async () => {
          await stopMutation.mutateAsync();
        }}
        onSaveOutbound={async (mode, url) => {
          await outboundMutation.mutateAsync({ mode, url });
        }}
        onRefresh={refresh}
        isStarting={startMutation.isPending}
        isStopping={stopMutation.isPending}
        isSavingOutbound={outboundMutation.isPending}
      />

      {/* Available Models */}
      <Card title={t("antigravity.models")} size="small">
        <Space direction="vertical" style={{ width: "100%" }} size={8}>
          <Text type="secondary">{t("antigravity.modelsHint")}</Text>
          {models.length === 0 ? (
            <Text type="secondary">{t("antigravity.modelsEmpty")}</Text>
          ) : (
            <Space direction="vertical" size={6} style={{ width: "100%" }}>
              {[
                {
                  key: "gemini",
                  label: "Gemini",
                  color: "blue",
                  items: models.filter((model) => model.id.startsWith("gemini")),
                },
                {
                  key: "other",
                  label: "Claude / GPT",
                  color: "purple",
                  items: models.filter((model) => !model.id.startsWith("gemini")),
                },
              ]
                .filter((group) => group.items.length > 0)
                .map((group) => (
                  <div key={group.key}>
                    <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 4 }}>
                      {group.label}
                    </Text>
                    <Space wrap size={[4, 4]}>
                      {group.items.map((model) => (
                        <Tag key={model.id} color={group.color} style={{ marginInlineEnd: 0 }}>
                          {model.displayName?.trim() || model.id}
                        </Tag>
                      ))}
                    </Space>
                  </div>
                ))}
            </Space>
          )}
        </Space>
      </Card>

      {/* Bind Apps */}
      <BindAppsCard
        boundMap={boundProvidersQuery.data}
        onEnsureBind={(target) => ensureMutation.mutate(target)}
        bindingTarget={bindingTarget}
        accountCount={accounts.length}
      />

      {/* Import Modal */}
      <ImportAccountsModal
        open={importModalOpen}
        onClose={() => setImportModalOpen(false)}
        onImport={async (json) => {
          await importMutation.mutateAsync(json);
        }}
        isImporting={importMutation.isPending}
      />
    </div>
  );
}
