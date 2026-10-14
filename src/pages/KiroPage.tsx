import { ExpandableSection } from "@/components/common/ExpandableSection";
import { useState } from "react";
import { Alert, Button, Card, Empty, Input, Popover, Skeleton, Space, Tag, Typography, message } from "antd";
import { openUrl } from "@tauri-apps/plugin-opener";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
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
  refreshKiroModels,
  startKiroSocialLogin,
  stopKiroGateway,
  submitKiroLoginCallback,
  type KiroExitProxyInput,
  type KiroOutboundMode,
} from "@/services/kiro";
import {
  AccountCard,
  AccountPoolOverview,
  GatewayCard,
  ImportAccountsModal,
  KIRO_MODEL_GROUPS,
  KIRO_MODELS_QUERY_KEY,
  kiroContextLabel,
  kiroModelGroup,
  useKiroModels,
} from "@/components/kiro";
import { errMsg } from "@/lib/errMsg";

const { Text } = Typography;


function modelGroupColor(group: (typeof KIRO_MODEL_GROUPS)[number]): string {
  switch (group) {
    case "sonnet":
      return "purple";
    case "opus":
      return "geekblue";
    case "haiku":
      return "cyan";
    case "gpt":
      return "green";
    case "other":
      return "default";
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
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [pastedCallback, setPastedCallback] = useState("");

  const accountsQuery = useQuery({
    queryKey: ["kiro-accounts"],
    queryFn: listKiroAccounts,
  });
  const statusQuery = useQuery({
    queryKey: ["kiro-gateway"],
    queryFn: getKiroGatewayStatus,
    refetchInterval: 5_000,
  });
  const modelsQuery = useKiroModels();
  const accounts = accountsQuery.data ?? [];
  const status = statusQuery.data;
  const modelList = modelsQuery.data;

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["kiro-accounts"] }),
      queryClient.invalidateQueries({ queryKey: ["kiro-gateway"] }),
      queryClient.invalidateQueries({ queryKey: KIRO_MODELS_QUERY_KEY }),
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

  const modelsMutation = useMutation({
    mutationFn: refreshKiroModels,
    onSuccess: async (list) => {
      message.success(t("kiro.modelsRefreshed", { count: list.models.length }));
      await queryClient.invalidateQueries({ queryKey: KIRO_MODELS_QUERY_KEY });
    },
    onError: (error: unknown) => message.error(errMsg(error), 10),
  });

  const pasteMutation = useMutation({
    mutationFn: (url: string) => submitKiroLoginCallback(url),
    onSuccess: async (nextUrl) => {
      setPastedCallback("");
      if (nextUrl) await openUrl(nextUrl);
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

      {socialMutation.isPending ? (
        <Alert
          type="info"
          showIcon
          message={t("kiro.pasteCallbackTitle")}
          description={
            <Space direction="vertical" style={{ width: "100%" }} size={8}>
              <Text type="secondary">{t("kiro.pasteCallbackHint")}</Text>
              <Input.Search
                value={pastedCallback}
                onChange={(event) => setPastedCallback(event.target.value)}
                placeholder="http://localhost:3128/oauth/callback?code=…&state=…"
                enterButton={t("kiro.pasteCallbackSubmit")}
                loading={pasteMutation.isPending}
                onSearch={(value) => {
                  if (value.trim()) pasteMutation.mutate(value.trim());
                }}
              />
            </Space>
          }
        />
      ) : null}

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

      <Card
        title={t("kiro.models")}
        size="small"
        extra={
          <Button
            size="small"
            loading={modelsMutation.isPending}
            disabled={accounts.length === 0}
            onClick={() => modelsMutation.mutate()}
          >
            {t("kiro.refreshModels")}
          </Button>
        }
      >
        <ExpandableSection
          title={<Text strong>{t("kiro.modelsHintToggle", { defaultValue: "查看支持的模型" })} <Text type="secondary">({modelList?.models?.length ?? 0})</Text></Text>}
          defaultOpen={false}
        >
        <Space direction="vertical" style={{ width: "100%" }} size={8}>
          <Text type="secondary">
            {modelList?.source === "live" ? t("kiro.modelsHintLive") : t("kiro.modelsHint")}
          </Text>
          {KIRO_MODEL_GROUPS.map((group) => {
            const rows = (modelList?.models ?? []).filter((model) => kiroModelGroup(model.id) === group);
            if (rows.length === 0) return null;
            return (
              <div key={group}>
                <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 4 }}>
                  {group[0].toUpperCase()}
                  {group.slice(1)}
                </Text>
                <Space wrap size={[4, 4]}>
                  {rows.map((model) => {
                    const context = kiroContextLabel(model);
                    return (
                      <Tag key={model.id} color={modelGroupColor(group)} style={{ marginInlineEnd: 0 }}>
                        {model.id}
                        {context ? ` · ${context}` : ""}
                      </Tag>
                    );
                  })}
                </Space>
              </div>
            );
          })}
        </Space>
        </ExpandableSection>
      </Card>

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
