import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Input,
  InputNumber,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
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
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";

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

  const targetLabel = (() => {
    switch (target) {
      case "claude_code":
        return t("providers.claudeCode");
      case "claude_desktop":
        return t("providers.claudeDesktop");
      case "codex":
        return "Codex";
      case "opencode":
        return "OpenCode";
      default: {
        const _exhaustive: never = target;
        return _exhaustive;
      }
    }
  })();

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <OnboardingTip
        tipKey="proxy"
        message={t("proxy.title")}
        description={
          <Space direction="vertical" size={4}>
            <Text type="secondary">{t("proxy.description")}</Text>
            <Text type="secondary">{t("proxy.hotSwitchDescription")}</Text>
          </Space>
        }
      />
      {statusQuery.error && (
        <Alert type="error" showIcon message={errMsg(statusQuery.error)} />
      )}

      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 12 }}>
        <WorkspaceTargetSegmented
          value={target}
          onChange={setProxyTarget}
          t={t}
          ariaLabel={t("workspace.target")}
        />
        <Typography.Text type="secondary" style={{ fontSize: 13 }}>
          {t("workspace.currentProvider", { name: targetLabel })}
        </Typography.Text>
      </div>

      <Card
        size="small"
        className="page-surface"
        title={
          <Space align="center">
            <ApiOutlined style={{ color: "var(--ant-color-primary)" }} />
            <Text strong>{t("proxy.status")}</Text>
          </Space>
        }
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={refreshing}
            onClick={() => void handleRefresh()}
          >
            {t("proxy.refresh")}
          </Button>
        }
      >
        {target === "opencode" ? (
          <Alert type="info" showIcon message={t("proxy.opencodeDirectHint")} />
        ) : !status ? (
          <Text type="secondary">{t("proxy.statusUnavailable")}</Text>
        ) : (
          <Descriptions column={1} size="small" bordered style={{ borderRadius: 8, overflow: "hidden" }}>
            <Descriptions.Item label={t("proxy.fieldRunning")}>
              <Space align="center" size={8}>
                <span
                  style={{
                    display: "inline-block",
                    width: 8,
                    height: 8,
                    borderRadius: "50%",
                    backgroundColor: status.running
                      ? "var(--as-status-running, #34c759)"
                      : status.phase === "starting"
                        ? "#1677ff"
                        : "var(--as-status-stopped, #8e8e93)",
                    boxShadow: status.running
                      ? "0 0 6px var(--as-status-running, #34c759)"
                      : "none",
                  }}
                />
                {status.phase === "starting" ? (
                  <Tag color="processing">{t("proxy.starting")}</Tag>
                ) : status.running ? (
                  <Tag color="green">{t("proxy.running")}</Tag>
                ) : status.phase === "error" ? (
                  <Tag color="red">{t("proxy.failed")}</Tag>
                ) : (
                  <Tag>{t("proxy.stopped")}</Tag>
                )}
              </Space>
            </Descriptions.Item>
            <Descriptions.Item label={t("proxy.fieldPort")}>
              <Text code>{status.port}</Text>
            </Descriptions.Item>
            <Descriptions.Item label={t("proxy.fieldTarget")}>
              {status.targetProvider ? (
                <Text strong>{status.targetProvider}</Text>
              ) : (
                <Text type="secondary">{t("proxy.noTarget")}</Text>
              )}
            </Descriptions.Item>
            <Descriptions.Item label={t("proxy.fieldEndpoint")}>
              <Text copyable code style={{ fontSize: 12 }}>
                {target === "codex"
                  ? `http://127.0.0.1:${status.port}/v1/responses`
                  : `http://127.0.0.1:${status.port}/v1/messages`}
              </Text>
            </Descriptions.Item>
          </Descriptions>
        )}
        {status?.lastError && (
          <Alert
            style={{ marginTop: 12 }}
            type="error"
            showIcon
            message={status.lastError}
          />
        )}
      </Card>

      {target !== "opencode" && (
        <>
          <Card size="small" className="page-surface" title={t("proxy.failoverTitle")}>
            <Space direction="vertical" style={{ width: "100%" }} size={12}>
              <Text type="secondary">{t("proxy.failoverDescription")}</Text>
              <Text type="secondary">{t("proxy.failoverGroupHint")}</Text>
              <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
                <Switch
                  checked={failoverQuery.data ?? false}
                  loading={failoverQuery.isPending || failoverSaving}
                  disabled={failoverSaving}
                  checkedChildren={t("common.enabled")}
                  unCheckedChildren={t("common.disabled")}
                  onChange={(enabled) => void handleFailoverChange(enabled)}
                />
                <Text style={{ fontSize: 13 }}>
                  {failoverQuery.data ? t("proxy.failoverEnabled") : t("proxy.failoverDisabled")}
                </Text>
              </div>
              <Text type="secondary" style={{ marginTop: 4 }}>{t("proxy.retryCodesHint")}</Text>
              <Space.Compact style={{ width: "100%", maxWidth: 460 }}>
                <Input
                  value={retryCodes}
                  onChange={(event) => setRetryCodes(event.target.value)}
                  placeholder="400-404,408,429,500-599"
                />
                <Button loading={retrySaving} onClick={() => void handleRetryCodesSave()}>
                  {t("common.save")}
                </Button>
              </Space.Compact>
              <Text type="secondary" style={{ marginTop: 4 }}>{t("proxy.idleTimeoutHint")}</Text>
              <Space>
                <InputNumber
                  min={5}
                  max={3600}
                  value={idleTimeout}
                  onChange={(value) => value != null && setIdleTimeout(value)}
                />
                <Button loading={idleSaving} onClick={() => void handleIdleTimeoutSave()}>
                  {t("common.save")}
                </Button>
              </Space>
            </Space>
          </Card>

          <Card
            size="small"
            className="page-surface"
            title={
              <Space align="center">
                <GlobalOutlined />
                <Text strong>{t("proxy.control")}</Text>
              </Space>
            }
          >
            <Space wrap align="center" size={16}>
              <Space align="center">
                <Text>{t("proxy.port")}</Text>
                <InputNumber
                  min={1024}
                  max={65535}
                  value={port}
                  onChange={(v) => v != null && setPort(v)}
                  disabled={busy || status?.running || status?.phase === "starting"}
                  style={{ width: 120 }}
                />
              </Space>
              {status?.running ? (
                <Button
                  type="primary"
                  danger
                  icon={<StopOutlined />}
                  loading={busy}
                  onClick={handleStop}
                >
                  {t("proxy.stop")}
                </Button>
              ) : (
                <Button
                  type="primary"
                  icon={<PlayCircleOutlined />}
                  loading={busy}
                  onClick={handleStart}
                >
                  {t("proxy.start")}
                </Button>
              )}
            </Space>
          </Card>
        </>
      )}
    </Space>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
