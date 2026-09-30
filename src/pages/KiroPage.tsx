import { useState } from "react";
import { Button, Card, Empty, Popover, Skeleton, Space, Tag, Typography, message } from "antd";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { listProviders } from "@/services/api";
import {
  ensureKiroProvider,
  getKiroGatewayStatus,
  importKiroAccounts,
  listKiroAccounts,
  probeKiroExitLatency,
  probeKiroExitProxy,
  refreshKiroQuotas,
  removeKiroAccount,
  setKiroExitProxy,
  setKiroGatewayApiKey,
  setKiroGatewayPort,
  setKiroOutboundProxy,
  startKiroBuilderIdLogin,
  startKiroGateway,
  startKiroSocialLogin,
  stopKiroGateway,
  type KiroExitProxyInput,
  type KiroOutboundMode,
} from "@/services/kiro";
import {
  AccountCard,
  AccountPoolOverview,
  BindAppsCard,
  GatewayCard,
  ImportAccountsModal,
  KIRO_CATALOG,
  KIRO_MODEL_GROUPS,
} from "@/components/kiro";
import { BIND_TARGETS } from "@/components/antigravity";
import type { ProviderTarget } from "@/types/backend";

const { Text } = Typography;

function errMsg(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (error && typeof error === "object" && "message" in error) {
    const msg = (error as { message?: unknown }).message;
    if (typeof msg === "string" && msg.trim()) return msg;
  }
  return String(error ?? "未知错误");
}

function modelGroupColor(group: (typeof KIRO_MODEL_GROUPS)[number]): string {
  switch (group) {
    case "sonnet":
      return "purple";
    case "opus":
      return "geekblue";
    case "haiku":
      return "cyan";
    case "fable":
      return "gold";
    default: {
      const _exhaustive: never = group;
      return _exhaustive;
    }
  }
}

export default function KiroPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [importOpen, setImportOpen] = useState(false);
  const [bindingTarget, setBindingTarget] = useState<ProviderTarget | null>(null);
  const [removingId, setRemovingId] = useState<string | null>(null);

  const accountsQuery = useQuery({
    queryKey: ["kiro-accounts"],
    queryFn: listKiroAccounts,
  });
  const statusQuery = useQuery({
    queryKey: ["kiro-gateway"],
    queryFn: getKiroGatewayStatus,
    refetchInterval: 5_000,
  });
  const boundQuery = useQuery({
    queryKey: ["kiro-bound-providers"],
    queryFn: async () => {
      const entries = await Promise.all(
        BIND_TARGETS.map(async (target) => {
          const providers = await listProviders(target);
          return [target, providers.some((provider) => provider.providerKind === "kiro")] as const;
        }),
      );
      return new Map<ProviderTarget, boolean>(entries);
    },
  });

  const accounts = accountsQuery.data ?? [];
  const status = statusQuery.data;

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["kiro-accounts"] }),
      queryClient.invalidateQueries({ queryKey: ["kiro-gateway"] }),
    ]);
  };

  const importMutation = useMutation({
    mutationFn: (raw: string) => importKiroAccounts(raw),
    onSuccess: async (count) => {
      message.success(t("kiro.importSuccess", { count }));
      setImportOpen(false);
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const builderMutation = useMutation({
    mutationFn: startKiroBuilderIdLogin,
    onSuccess: async (account) => {
      message.success(t("kiro.loginSuccess", { label: account.label || account.id }));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error), 10),
  });

  const socialMutation = useMutation({
    mutationFn: startKiroSocialLogin,
    onSuccess: async (account) => {
      message.success(t("kiro.loginSuccess", { label: account.label || account.id }));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error), 10),
  });

  const startMutation = useMutation({
    mutationFn: async ({
      port,
      apiKey,
      outboundMode,
      outboundUrl,
    }: {
      port: number;
      apiKey: string;
      outboundMode: KiroOutboundMode;
      outboundUrl: string;
    }) => {
      await setKiroGatewayPort(port);
      if (apiKey.trim()) await setKiroGatewayApiKey(apiKey.trim());
      await setKiroOutboundProxy(outboundMode, outboundUrl);
      return startKiroGateway(port);
    },
    onSuccess: async () => {
      message.success(t("kiro.started"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const stopMutation = useMutation({
    mutationFn: stopKiroGateway,
    onSuccess: async () => {
      message.success(t("kiro.stopped"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const outboundMutation = useMutation({
    mutationFn: ({ mode, url }: { mode: KiroOutboundMode; url: string }) => setKiroOutboundProxy(mode, url),
    onSuccess: async () => {
      message.success(t("kiro.outboundSaved"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const quotaMutation = useMutation({
    mutationFn: refreshKiroQuotas,
    onSuccess: async () => {
      message.success(t("kiro.quotaRefreshed"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const exitMutation = useMutation({
    mutationFn: (entries: KiroExitProxyInput[]) => setKiroExitProxy(entries),
    onSuccess: async () => {
      message.success(t("kiro.exitSaved"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
  });

  const ensureMutation = useMutation({
    mutationFn: (target: ProviderTarget) => ensureKiroProvider(target),
    onMutate: (target) => setBindingTarget(target),
    onSuccess: async (_provider, target) => {
      message.success(t("kiro.providerReady", { target: t(`workspace.${target}`) }));
      await refresh();
      await queryClient.invalidateQueries({ queryKey: ["kiro-bound-providers"] });
      await queryClient.invalidateQueries({ queryKey: ["providers"] });
    },
    onError: (error: unknown) => message.error(errMsg(error)),
    onSettled: () => setBindingTarget(null),
  });

  const removeMutation = useMutation({
    mutationFn: (id: string) => removeKiroAccount(id),
    onMutate: (id) => setRemovingId(id),
    onSuccess: async () => {
      message.success(t("kiro.removed"));
      await refresh();
    },
    onError: (error: unknown) => message.error(errMsg(error)),
    onSettled: () => setRemovingId(null),
  });

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
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
            title={t("kiro.howToAddTitle")}
            content={
              <div style={{ maxWidth: 360 }}>
                <p style={{ marginBottom: 8 }}>{t("kiro.howToAddBuilder")}</p>
                <p style={{ marginBottom: 8 }}>{t("kiro.howToAddSocial")}</p>
                <p style={{ marginBottom: 0 }}>{t("kiro.howToAddJson")}</p>
              </div>
            }
          >
            <Button size="small" icon={<QuestionCircleOutlined />}>
              {t("kiro.whyAccountMissing")}
            </Button>
          </Popover>
          <Button icon={<ImportOutlined />} onClick={() => setImportOpen(true)}>
            {t("kiro.import")}
          </Button>
          <Button loading={quotaMutation.isPending} onClick={() => quotaMutation.mutate()}>
            {t("kiro.refreshAllQuotas")}
          </Button>
          <Button
            icon={<LoginOutlined />}
            loading={socialMutation.isPending}
            onClick={() => socialMutation.mutate()}
          >
            {socialMutation.isPending ? t("kiro.loginWaiting") : t("kiro.socialLogin")}
          </Button>
          <Button
            type="primary"
            icon={<LoginOutlined />}
            loading={builderMutation.isPending}
            onClick={() => builderMutation.mutate()}
          >
            {builderMutation.isPending ? t("kiro.loginWaiting") : t("kiro.builderLogin")}
          </Button>
        </Space>
      </div>

      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("kiro.scopeHint")}
      </Text>

      <section>
        <Space align="center" style={{ marginBottom: 12 }}>
          <UserOutlined />
          <Text strong>{t("kiro.accounts")}</Text>
          <Text type="secondary" style={{ fontSize: 13 }}>
            ({accounts.length})
          </Text>
        </Space>
        {accountsQuery.isLoading ? (
          <Skeleton active paragraph={{ rows: 3 }} />
        ) : accounts.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={t("kiro.emptyAccounts")}>
            <Button
              type="primary"
              icon={<LoginOutlined />}
              loading={builderMutation.isPending}
              onClick={() => builderMutation.mutate()}
            >
              {t("kiro.builderLogin")}
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
            {accounts.map((account) => (
              <AccountCard
                key={account.id}
                account={account}
                isPending={removingId === account.id}
                onRemove={(id) => removeMutation.mutate(id)}
              />
            ))}
          </div>
        )}
      </section>

      <GatewayCard
        status={status}
        isStarting={startMutation.isPending}
        isStopping={stopMutation.isPending}
        isSavingOutbound={outboundMutation.isPending}
        onStartGateway={async (port, apiKey, outboundMode, outboundUrl) => {
          await startMutation.mutateAsync({ port, apiKey, outboundMode, outboundUrl });
        }}
        onStopGateway={async () => {
          await stopMutation.mutateAsync();
        }}
        onSaveOutbound={async (mode, url) => {
          await outboundMutation.mutateAsync({ mode, url });
        }}
        onSaveExit={async (entries) => {
          await exitMutation.mutateAsync(entries);
        }}
        onProbeExit={(id, url) => probeKiroExitProxy(id, url)}
        onProbeLatency={(id, url) => probeKiroExitLatency(id, url)}
        isSavingExit={exitMutation.isPending}
        onRefresh={() => {
          void refresh();
        }}
      />

      <Card title={t("kiro.models")} size="small">
        <Space direction="vertical" style={{ width: "100%" }} size={8}>
          <Text type="secondary">{t("kiro.modelsHint")}</Text>
          {KIRO_MODEL_GROUPS.map((group) => (
            <div key={group}>
              <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 4 }}>
                {group[0].toUpperCase()}
                {group.slice(1)}
              </Text>
              <Space wrap size={[4, 4]}>
                {KIRO_CATALOG.filter((model) => model.group === group).map((model) => (
                  <Tag key={model.id} color={modelGroupColor(group)} style={{ marginInlineEnd: 0 }}>
                    {model.id}
                  </Tag>
                ))}
              </Space>
            </div>
          ))}
        </Space>
      </Card>

      <BindAppsCard
        boundMap={boundQuery.data}
        onEnsureBind={(target) => ensureMutation.mutate(target)}
        bindingTarget={bindingTarget}
        accountCount={accounts.length}
      />

      <ImportAccountsModal
        open={importOpen}
        onClose={() => setImportOpen(false)}
        isImporting={importMutation.isPending}
        onImport={async (raw) => {
          await importMutation.mutateAsync(raw);
        }}
      />
    </div>
  );
}
