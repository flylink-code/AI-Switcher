import { useEffect, useState } from "react";
import { Alert, Badge, Button, Space, Typography, message } from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  getProxyFailoverEnabled,
  getProxyRetryableStatusCodes,
  getProxyStreamingIdleTimeoutSecs,
  setProxyFailoverEnabled,
  setProxyRetryableStatusCodes,
  setProxyStreamingIdleTimeoutSecs,
  setProxyPort,
  startProxy,
  stopProxy,
} from "@/services/api";
import { proxyStatusOptions } from "@/lib/appQueries";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import { OnboardingTip } from "@/components/OnboardingTip";
import { AgentTargetSwitcher } from "@/components/AgentTargetSwitcher";
import { ProxyRoutePanel, ResilienceSettings } from "@/components/proxy";
import { Stack } from "@/components/ui";
import type { ProviderTarget } from "@/types/backend";

const { Text } = Typography;

const PROXY_TARGETS: ProviderTarget[] = [
  "claude_code",
  "claude_desktop",
  "codex",
  "opencode",
  "pi",
  "cline",
];

export default function ProxyPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [port, setPort] = useState<number>(15821);
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [failoverSaving, setFailoverSaving] = useState(false);
  const [retryCodes, setRetryCodes] = useState("400-404,408,429,500-599");
  const [retrySaving, setRetrySaving] = useState(false);
  const [idleTimeout, setIdleTimeout] = useState(180);
  const [idleSaving, setIdleSaving] = useState(false);

  // Page-local Agent target (independent persisted slice).
  const target = usePagePreferencesStore((state) => state.proxyTarget);
  const setProxyTarget = usePagePreferencesStore((state) => state.setProxyTarget);
  const statusQuery = useQuery(proxyStatusOptions(target));
  const status = statusQuery.data ?? null;
  const isOpencode = target === "opencode";
  const isRunning = status?.running ?? false;

  const failoverQuery = useQuery({ queryKey: ["proxy-failover-enabled"], queryFn: getProxyFailoverEnabled });
  const retryQuery = useQuery({
    queryKey: ["proxy-retryable-status-codes"],
    queryFn: getProxyRetryableStatusCodes,
  });
  const idleQuery = useQuery({
    queryKey: ["proxy-streaming-idle-timeout"],
    queryFn: getProxyStreamingIdleTimeoutSecs,
  });

  useEffect(() => {
    if (status) setPort(status.port);
  }, [status]);

  useEffect(() => {
    if (retryQuery.data) setRetryCodes(retryQuery.data);
  }, [retryQuery.data]);

  useEffect(() => {
    if (typeof idleQuery.data === "number") setIdleTimeout(idleQuery.data);
  }, [idleQuery.data]);

  const handleStart = async () => {
    setBusy(true);
    try {
      await setProxyPort(port, target);
      const s = await startProxy(port, target);
      queryClient.setQueryData(["proxy-status", target], s);
      void message.success(t("proxy.started", { port: s.port }));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleStop = async () => {
    setBusy(true);
    try {
      const s = await stopProxy(target);
      queryClient.setQueryData(["proxy-status", target], s);
      void message.success(t("proxy.stopped"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await statusQuery.refetch();
    } finally {
      setRefreshing(false);
    }
  };

  const handleFailoverChange = async (enabled: boolean) => {
    setFailoverSaving(true);
    try {
      await setProxyFailoverEnabled(enabled);
      queryClient.setQueryData(["proxy-failover-enabled"], enabled);
      void message.success(t(enabled ? "proxy.failoverEnabled" : "proxy.failoverDisabled"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setFailoverSaving(false);
    }
  };

  const handleRetryCodesSave = async () => {
    setRetrySaving(true);
    try {
      await setProxyRetryableStatusCodes(retryCodes);
      const saved = await getProxyRetryableStatusCodes();
      setRetryCodes(saved);
      queryClient.setQueryData(["proxy-retryable-status-codes"], saved);
      void message.success(t("proxy.retryCodesSaved"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setRetrySaving(false);
    }
  };

  const handleIdleTimeoutSave = async () => {
    setIdleSaving(true);
    try {
      await setProxyStreamingIdleTimeoutSecs(idleTimeout);
      const saved = await getProxyStreamingIdleTimeoutSecs();
      setIdleTimeout(saved);
      queryClient.setQueryData(["proxy-streaming-idle-timeout"], saved);
      void message.success(t("proxy.idleTimeoutSaved"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setIdleSaving(false);
    }
  };

  const statusBadge = (() => {
    if (isOpencode) {
      return <Badge status="processing" text={t("workbench.proxyDirect")} />;
    }
    if (!status) {
      return <Badge status="default" text={t("proxy.statusUnavailable", { defaultValue: "状态不可用" })} />;
    }
    if (status.phase === "starting") {
      return <Badge status="processing" text={t("proxy.starting", { defaultValue: "启动中..." })} />;
    }
    if (isRunning) {
      return <Badge status="success" text={t("proxy.running", { defaultValue: "运行中" })} />;
    }
    if (status.phase === "error") {
      return <Badge status="error" text={t("proxy.failed", { defaultValue: "异常" })} />;
    }
    return <Badge status="default" text={t("proxy.stopped", { defaultValue: "已停止" })} />;
  })();

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%", minWidth: 0 }}>
      <OnboardingTip
        tipKey="proxy"
        message={t("proxy.title")}
        description={
          <Stack gap="xs">
            <Text type="secondary">{t("proxy.description")}</Text>
            <Text type="secondary">{t("proxy.hotSwitchDescription")}</Text>
          </Stack>
        }
      />

      {statusQuery.error && (
        <Alert type="error" showIcon message={errMsg(statusQuery.error)} />
      )}

      {/* Same page-header pattern as Providers: switcher + status | actions */}
      <div className="cc-workbench-header">
        <div className="cc-header-left">
          <AgentTargetSwitcher value={target} onChange={setProxyTarget} targets={PROXY_TARGETS} />
          {statusBadge}
        </div>
        {!isOpencode && (
          <div className="cc-header-right">
            <Button
              icon={<ReloadOutlined spin={refreshing} />}
              loading={refreshing}
              onClick={() => void handleRefresh()}
            >
              {t("proxy.refresh", { defaultValue: "刷新" })}
            </Button>
            {isRunning ? (
              <Button
                type="primary"
                danger
                icon={<StopOutlined />}
                loading={busy}
                onClick={() => void handleStop()}
              >
                {t("proxy.stop", { defaultValue: "停止代理" })}
              </Button>
            ) : (
              <Button
                type="primary"
                icon={<PlayCircleOutlined />}
                loading={busy}
                disabled={status?.phase === "starting"}
                onClick={() => void handleStart()}
              >
                {t("proxy.start", { defaultValue: "启动代理" })}
              </Button>
            )}
          </div>
        )}
      </div>

      <ProxyRoutePanel
        status={status}
        target={target}
        port={port}
        onPortChange={setPort}
        busy={busy}
        clientLabel={t(`workspace.${target}`)}
      />

      {!isOpencode && (
        <ResilienceSettings
          failoverEnabled={failoverQuery.data ?? false}
          failoverSaving={failoverQuery.isPending || failoverSaving}
          onFailoverChange={(enabled) => void handleFailoverChange(enabled)}
          retryCodes={retryCodes}
          onRetryCodesChange={setRetryCodes}
          retrySaving={retrySaving}
          onRetryCodesSave={() => void handleRetryCodesSave()}
          idleTimeout={idleTimeout}
          onIdleTimeoutChange={setIdleTimeout}
          idleSaving={idleSaving}
          onIdleTimeoutSave={() => void handleIdleTimeoutSave()}
        />
      )}
    </Space>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
