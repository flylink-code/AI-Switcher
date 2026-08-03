import { useState } from "react";
import { Button, Card, Space, Switch, Table, Typography, message } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { codexPluginsOptions } from "@/lib/appQueries";
import { setCodexPluginEnabled } from "@/services/api";
import type { CodexPlugin } from "@/types/backend";

const { Paragraph, Text } = Typography;

export default function CodexPluginsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const pluginsQuery = useQuery(codexPluginsOptions);
  const [busyPluginId, setBusyPluginId] = useState<string | null>(null);

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: ["codexPlugins"] });
  };

  const toggleEnabled = async (plugin: CodexPlugin, enabled: boolean) => {
    setBusyPluginId(plugin.pluginId);
    try {
      await setCodexPluginEnabled(plugin.pluginId, enabled);
      await refresh();
      void message.success(t("codexPlugins.toggled"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyPluginId(null);
    }
  };

  return (
    <Card
      title={t("codexPlugins.title")}
      extra={
        <Button
          icon={<ReloadOutlined />}
          loading={pluginsQuery.isFetching}
          onClick={() => void refresh()}
        >
          {t("codexPlugins.refresh")}
        </Button>
      }
    >
      <Paragraph type="secondary">{t("codexPlugins.description")}</Paragraph>
      <Table
        rowKey="pluginId"
        loading={pluginsQuery.isLoading}
        dataSource={pluginsQuery.data ?? []}
        pagination={false}
        locale={{ emptyText: t("codexPlugins.empty") }}
        columns={[
          {
            title: t("codexPlugins.name"),
            dataIndex: "name",
            render: (name: string, plugin: CodexPlugin) => (
              <Space direction="vertical" size={0}>
                <Text strong>{name}</Text>
                <Text type="secondary">{plugin.pluginId}</Text>
              </Space>
            ),
          },
          {
            title: t("codexPlugins.marketplace"),
            dataIndex: "marketplace",
            width: 180,
          },
          {
            title: t("codexPlugins.version"),
            dataIndex: "version",
            width: 120,
          },
          {
            title: t("codexPlugins.path"),
            dataIndex: "path",
            ellipsis: true,
            render: (path?: string | null) =>
              path ? <Text copyable={{ text: path }}>{path}</Text> : <Text type="secondary">—</Text>,
          },
          {
            title: t("codexPlugins.enabled"),
            width: 100,
            render: (_: unknown, plugin: CodexPlugin) => (
              <Switch
                checked={plugin.enabled}
                loading={busyPluginId === plugin.pluginId}
                disabled={busyPluginId !== null && busyPluginId !== plugin.pluginId}
                onChange={(checked) => void toggleEnabled(plugin, checked)}
              />
            ),
          },
        ]}
      />
    </Card>
  );
}
