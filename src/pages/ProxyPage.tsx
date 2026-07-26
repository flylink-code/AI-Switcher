import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  InputNumber,
  Skeleton,
  Space,
  Segmented,
  Tag,
  Typography,
  message,
} from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import GlobalOutlined from "@ant-design/icons/es/icons/GlobalOutlined";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { ProviderTarget } from "@/types/backend";
import { setProxyPort, startProxy, stopProxy } from "@/services/api";
import { proxyStatusOptions } from "@/lib/appQueries";

const { Text } = Typography;

export default function ProxyPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [port, setPort] = useState<number>(15821);
  const [busy, setBusy] = useState(false);
  const [target, setTarget] = useState<ProviderTarget>("claude_desktop");
  const statusQuery = useQuery({
    ...proxyStatusOptions(target),
    placeholderData: keepPreviousData,
  });
  const status = statusQuery.data ?? null;

  useEffect(() => {
    if (status) setPort(status.port);
  }, [status]);

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

  return (
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
            <Button
              icon={<ReloadOutlined />}
              loading={statusQuery.isFetching}
              onClick={() => void statusQuery.refetch()}
            >
              {t("proxy.refresh")}
            </Button>
          }
        >
          {!status ? (
            <Skeleton active paragraph={{ rows: 3 }} />
          ) : (
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("proxy.fieldRunning")}>
                {status.phase === "starting" ? (
                  <Tag color="processing">{t("proxy.starting")}</Tag>
                ) : status.running ? (
                  <Tag color="green">{t("proxy.running")}</Tag>
                ) : status.phase === "error" ? (
                  <Tag color="red">{t("proxy.failed")}</Tag>
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
          {status?.lastError && (
            <Alert
              style={{ marginTop: 12 }}
              type="error"
              showIcon
              message={status.lastError}
            />
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
    </Space>
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
