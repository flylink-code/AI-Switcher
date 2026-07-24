import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  InputNumber,
  Space,
  Segmented,
  Spin,
  Tag,
  Typography,
  message,
} from "antd";
import {
  PlayCircleOutlined,
  StopOutlined,
  ReloadOutlined,
  ApiOutlined,
  GlobalOutlined,
} from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import type { ProxyStatus, ProviderTarget } from "@/types/backend";
import { getProxyStatus, setProxyPort, startProxy, stopProxy } from "@/services/api";

const { Text } = Typography;

export default function ProxyPage() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [port, setPort] = useState<number>(15821);
  const [busy, setBusy] = useState(false);
  const [target, setTarget] = useState<ProviderTarget>("claude_desktop");

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const s = await getProxyStatus(target);
      setStatus(s);
      setPort(s.port);
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [target]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleStart = async () => {
    setBusy(true);
    try {
      await setProxyPort(port, target);
      const s = await startProxy(port, target);
      setStatus(s);
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
      setStatus(s);
      void message.success(t("proxy.stopped"));
    } catch (e) {
      void message.error(errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Spin spinning={loading}>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Alert
          type="info"
          showIcon
          message={t("proxy.title")}
          description={t("proxy.description")}
        />

        <Segmented<ProviderTarget>
          value={target}
          onChange={setTarget}
          options={[
            { value: "claude_code", label: t("providers.claudeCode") },
            { value: "claude_desktop", label: t("providers.claudeDesktop") },
          ]}
        />

        <Card
          size="small"
          title={
            <Space>
              <ApiOutlined />
              {t("proxy.status")}
            </Space>
          }
          extra={
            <Button icon={<ReloadOutlined />} loading={busy} onClick={() => void refresh()}>
              {t("proxy.refresh")}
            </Button>
          }
        >
          {status && (
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("proxy.fieldRunning")}>
                {status.running ? (
                  <Tag color="green">{t("proxy.running")}</Tag>
                ) : (
                  <Tag>{t("proxy.stopped")}</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("proxy.fieldPort")}>
                <Text code>{status.port}</Text>
              </Descriptions.Item>
              <Descriptions.Item label={t("proxy.fieldTarget")}>
                {status.targetProvider ? (
                  <Text>{status.targetProvider}</Text>
                ) : (
                  <Text type="secondary">{t("proxy.noTarget")}</Text>
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("proxy.fieldEndpoint")}>
                <Text copyable code>{`http://127.0.0.1:${status.port}/v1/messages`}</Text>
              </Descriptions.Item>
            </Descriptions>
          )}
        </Card>

        <Card
          size="small"
          title={
            <Space>
              <GlobalOutlined />
              {t("proxy.control")}
            </Space>
          }
        >
          <Space wrap>
            <Space>
              <Text>{t("proxy.port")}</Text>
              <InputNumber
                min={1024}
                max={65535}
                value={port}
                onChange={(v) => v != null && setPort(v)}
                disabled={busy || status?.running}
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
      </Space>
    </Spin>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
