import { useEffect, useState } from "react";
import { Alert, Typography, message } from "antd";
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
import { ProxyRuntimeCard, ProxyRoutePanel, ResilienceSettings } from "@/components/proxy";
import { Stack } from "@/components/ui";

const { Text } = Typography;

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

  // Page-local Agent target (independent persisted slice, switched from the
  // runtime card header). No global Agent context anymore.
  const target = usePagePreferencesStore((state) => state.proxyTarget);
  const setProxyTarget = usePagePreferencesStore((state) => state.setProxyTarget);
  const statusQuery = useQuery(proxyStatusOptions(target));
  const status = statusQuery.data ?? null;

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

  return (
    <Stack gap="md" style={{ width: "100%", minWidth: 0 }}>
      {/* Onboarding Tip */}
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

      {/* Hero Runtime Control（页内 Agent 切换器在卡片头部） */}
      <ProxyRuntimeCard
        status={status}
        target={target}
        clientLabel={t(`workspace.${target}`)}
        busy={busy}
        refreshing={refreshing}
        onStart={() => void handleStart()}
        onStop={() => void handleStop()}
        onRefresh={() => void handleRefresh()}
        headerExtra={
          <AgentTargetSwitcher value={target} onChange={setProxyTarget} />
        }
      />

      {/* Route + Resilience balanced columns */}
      {target !== "opencode" ? (
        <div
          style={{
            display: "flex",
            gap: "var(--space-4)",
            alignItems: "flex-start",
            flexWrap: "wrap",
            minWidth: 0,
          }}
        >
          <ProxyRoutePanel
            style={{ flex: "1 1 360px", minWidth: 0 }}
            status={status}
            target={target}
            port={port}
            onPortChange={setPort}
            busy={busy}
          />
          <ResilienceSettings
            style={{ flex: "1 1 360px", minWidth: 0 }}
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
        </div>
      ) : (
        <ProxyRoutePanel
          status={status}
          target={target}
          port={port}
          onPortChange={setPort}
          busy={busy}
        />
      )}
    </Stack>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
