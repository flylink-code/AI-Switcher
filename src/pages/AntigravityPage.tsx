import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Empty,
  Skeleton,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
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
import type { ProviderTarget } from "@/types/backend";
import { ContextHeader } from "@/components/layout";
import {
  AccountPoolOverview,
  AccountCard,
  GatewayCard,
  BindAppsCard,
  ImportAccountsModal,
} from "@/components/antigravity";

const { Text } = Typography;

const ANTIGRAVITY_QUOTA_REFRESH_MS = 5 * 60_000;
const ANTIGRAVITY_QUOTA_REFRESH_EVENT = "antigravity-quota-refreshed";
const BIND_TARGETS: ProviderTarget[] = [
  "claude_code",
  "claude_desktop",
  "codex",
  "opencode",
];

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
  const queryClient = useQueryClient();

  const [importModalOpen, setImportModalOpen] = useState(false);
  const [bindingTarget, setBindingTarget] = useState<ProviderTarget | null>(null);
  const [actionAccountId, setActionAccountId] = useState<string | null>(null);

  const accountsQuery = useQuery({
    queryKey: ["antigravity-accounts"],
    queryFn: listAntigravityAccounts,
    refetchInterval: ANTIGRAVITY_QUOTA_REFRESH_MS,
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
      {/* ContextHeader */}
      <ContextHeader
        title={t("antigravity.title")}
        description={t("antigravity.subtitle")}
        actions={
          <Space wrap>
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
        }
      />

      <Alert type="info" showIcon message={t("antigravity.personalUseNotice")} />

      {/* Pool Overview */}
      <AccountPoolOverview accounts={accounts} status={status} />

      {/* Account Pool Cards */}
      <Card
        title={
          <Space>
            <UserOutlined />
            <span>{t("antigravity.accounts")}</span>
            <Text type="secondary" style={{ fontSize: 13, fontWeight: "normal" }}>
              ({accounts.length})
            </Text>
          </Space>
        }
        size="small"
      >
        <Space direction="vertical" style={{ width: "100%" }} size={12}>
          <Alert
            type="warning"
            showIcon
            message={t("antigravity.howToAddTitle")}
            description={
              <div>
                <p style={{ marginBottom: 4 }}>{t("antigravity.howToAddOauth")}</p>
                <p style={{ marginBottom: 4 }}>{t("antigravity.howToAddNotIde")}</p>
                <p style={{ marginBottom: 0 }}>{t("antigravity.howToAddJson")}</p>
              </div>
            }
          />

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
        </Space>
      </Card>

      {/* Gateway Controls */}
      <GatewayCard
        status={status}
        models={models}
        onStartGateway={async (port, apiKey, outboundMode, outboundUrl) => {
          await startMutation.mutateAsync({ port, apiKey, outboundMode, outboundUrl });
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
            <Space wrap size={[4, 4]}>
              {models.map((model) => (
                <Tag key={model.id} color={model.id.startsWith("gemini") ? "blue" : "purple"}>
                  {model.displayName?.trim() || model.id}
                </Tag>
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
