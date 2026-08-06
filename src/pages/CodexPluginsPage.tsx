import { useState } from "react";
import { Alert, Button, Card, Input, Popconfirm, Space, Switch, Table, Typography, message } from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { codexPluginsOptions } from "@/lib/appQueries";
import {
  addCodexPluginMarketplace,
  listCodexPluginMarketplaces,
  removeCodexPluginMarketplace,
  setCodexPluginEnabled,
  uninstallCodexPlugin,
} from "@/services/api";
import type { CodexMarketplace, CodexPlugin } from "@/types/backend";

const { Paragraph, Text } = Typography;

export default function CodexPluginsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const pluginsQuery = useQuery(codexPluginsOptions);
  const [busyPluginId, setBusyPluginId] = useState<string | null>(null);
  const [marketplaces, setMarketplaces] = useState<CodexMarketplace[]>([]);
  const [marketplaceBusy, setMarketplaceBusy] = useState(false);
  const [marketplaceSource, setMarketplaceSource] = useState("");
  const [marketplaceLoaded, setMarketplaceLoaded] = useState(false);

  const snapshot = pluginsQuery.data;
  const plugins = snapshot?.plugins ?? [];
  const queryError =
    pluginsQuery.error instanceof Error
      ? pluginsQuery.error.message
      : pluginsQuery.error
        ? String(pluginsQuery.error)
        : null;

  const refresh = async () => {
    await queryClient.invalidateQueries({ queryKey: ["codexPlugins"] });
  };

  const refreshMarketplaces = async () => {
    setMarketplaceBusy(true);
    try {
      const result = await listCodexPluginMarketplaces();
      setMarketplaces(result.marketplaces);
      setMarketplaceLoaded(true);
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setMarketplaceBusy(false);
    }
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

  const uninstall = async (plugin: CodexPlugin) => {
    setBusyPluginId(plugin.pluginId);
    try {
      const result = await uninstallCodexPlugin(plugin.pluginId);
      void message.success(result.message || t("codexPlugins.uninstalled"));
      await refresh();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyPluginId(null);
    }
  };

  const addMarketplace = async () => {
    if (!marketplaceSource.trim()) return;
    setMarketplaceBusy(true);
    try {
      const result = await addCodexPluginMarketplace(marketplaceSource.trim());
      void message.success(result.message || t("codexPlugins.marketplaceAdded"));
      setMarketplaceSource("");
      await refreshMarketplaces();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setMarketplaceBusy(false);
    }
  };

  const removeMarketplace = async (name: string) => {
    setMarketplaceBusy(true);
    try {
      const result = await removeCodexPluginMarketplace(name);
      void message.success(result.message || t("codexPlugins.marketplaceRemoved"));
      await refreshMarketplaces();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setMarketplaceBusy(false);
    }
  };

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Card
        className="page-surface"
        title={t("codexPlugins.marketplaceTitle")}
        extra={
          <Button icon={<ReloadOutlined />} loading={marketplaceBusy} onClick={() => void refreshMarketplaces()}>
            {t("codexPlugins.marketplaceRefresh")}
          </Button>
        }
      >
        <Paragraph type="secondary">{t("codexPlugins.marketplaceDescription")}</Paragraph>
        <Space.Compact style={{ width: "100%", marginBottom: 12 }}>
          <Input
            placeholder={t("codexPlugins.marketplaceSourcePlaceholder")}
            value={marketplaceSource}
            onChange={(event) => setMarketplaceSource(event.target.value)}
            onPressEnter={() => void addMarketplace()}
          />
          <Button
            type="primary"
            icon={<PlusOutlined />}
            loading={marketplaceBusy}
            disabled={!marketplaceSource.trim()}
            onClick={() => void addMarketplace()}
          >
            {t("codexPlugins.marketplaceAdd")}
          </Button>
        </Space.Compact>
        {!marketplaceLoaded ? (
          <Text type="secondary">{t("codexPlugins.marketplaceHint")}</Text>
        ) : (
          <Table
            size="small"
            rowKey="name"
            loading={marketplaceBusy}
            dataSource={marketplaces}
            pagination={false}
            locale={{ emptyText: t("codexPlugins.marketplaceEmpty") }}
            columns={[
              { title: t("codexPlugins.marketplace"), dataIndex: "name" },
              {
                title: t("codexPlugins.path"),
                dataIndex: "root",
                ellipsis: true,
                render: (_: unknown, row: CodexMarketplace) =>
                  row.root || row.source || <Text type="secondary">—</Text>,
              },
              {
                title: t("codexPlugins.enabled"),
                width: 100,
                render: (_: unknown, row: CodexMarketplace) => (
                  <Popconfirm
                    title={t("codexPlugins.confirmRemoveMarketplace")}
                    onConfirm={() => void removeMarketplace(row.name)}
                  >
                    <Button size="small" danger icon={<DeleteOutlined />} disabled={marketplaceBusy}>
                      {t("codexPlugins.marketplaceRemove")}
                    </Button>
                  </Popconfirm>
                ),
              },
            ]}
          />
        )}
      </Card>

      <Card
        className="page-surface"
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
        {queryError ? (
          <Alert
            type="error"
            showIcon
            style={{ marginBottom: 16 }}
            message={t("codexPlugins.loadError")}
            description={queryError}
          />
        ) : null}
        {!queryError && snapshot && !snapshot.parseOk ? (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 16 }}
            message={t("codexPlugins.parseError")}
            description={snapshot.parseError ?? undefined}
          />
        ) : null}
        {!queryError && snapshot && plugins.length === 0 ? (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 16 }}
            message={t("codexPlugins.empty")}
            description={t("codexPlugins.emptyHint", {
              configPath: snapshot.configPath,
              cachePath: snapshot.cachePath,
              configCount: snapshot.configPluginCount,
              cacheCount: snapshot.cachePluginCount,
            })}
          />
        ) : null}
        <Table
          rowKey="pluginId"
          loading={pluginsQuery.isLoading}
          dataSource={plugins}
          pagination={false}
          locale={{ emptyText: queryError ? t("codexPlugins.loadError") : t("codexPlugins.empty") }}
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
              render: (version?: string | null) => version ?? "—",
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
            {
              title: t("codexPlugins.actions"),
              width: 100,
              render: (_: unknown, plugin: CodexPlugin) => (
                <Popconfirm
                  title={t("codexPlugins.confirmUninstall")}
                  onConfirm={() => void uninstall(plugin)}
                >
                  <Button
                    size="small"
                    danger
                    icon={<DeleteOutlined />}
                    loading={busyPluginId === plugin.pluginId}
                    disabled={busyPluginId !== null && busyPluginId !== plugin.pluginId}
                  >
                    {t("codexPlugins.uninstall")}
                  </Button>
                </Popconfirm>
              ),
            },
          ]}
        />
      </Card>
    </Space>
  );
}
