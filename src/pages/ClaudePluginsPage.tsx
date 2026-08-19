import { useState } from "react";
import { Alert, Button, Card, Empty, Input, Popconfirm, Select, Space, Switch, Table, Tag, Typography, message } from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  claudePluginCatalogOptions,
  claudePluginMarketplacesOptions,
  claudePluginsOptions,
} from "@/lib/appQueries";
import {
  addClaudePluginMarketplace,
  checkClaudePluginUpdate,
  checkClaudePluginUpdates,
  installClaudePlugin,
  removeClaudePluginMarketplace,
  setClaudePluginEnabled,
  uninstallClaudePlugin,
  updateClaudePlugin,
  updateClaudePluginMarketplace,
} from "@/services/api";
import type { ClaudeCatalogPlugin, ClaudeMarketplace, ClaudePlugin, ClaudePluginUpdateStatus } from "@/types/backend";

const { Paragraph, Text } = Typography;

const MARKETPLACE_PAGE_SIZE = 5;
const PLUGIN_PAGE_SIZE = 8;

export default function ClaudePluginsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const pluginsQuery = useQuery(claudePluginsOptions);
  const catalogQuery = useQuery(claudePluginCatalogOptions);
  const marketplacesQuery = useQuery(claudePluginMarketplacesOptions);
  const [busyPluginId, setBusyPluginId] = useState<string | null>(null);
  const [marketplaceBusy, setMarketplaceBusy] = useState(false);
  const [marketplaceSource, setMarketplaceSource] = useState("");
  const [installId, setInstallId] = useState<string | undefined>(undefined);
  const [installBusy, setInstallBusy] = useState(false);
  const [updateStatuses, setUpdateStatuses] = useState<Record<string, ClaudePluginUpdateStatus>>({});
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [checkingPluginId, setCheckingPluginId] = useState<string | null>(null);
  const [updatingPluginId, setUpdatingPluginId] = useState<string | null>(null);

  const snapshot = pluginsQuery.data;
  const plugins = snapshot?.plugins ?? [];
  const marketplaces = marketplacesQuery.data?.marketplaces ?? [];
  const catalog = catalogQuery.data?.plugins ?? [];
  const installedIds = new Set(plugins.filter((plugin) => plugin.installed).map((plugin) => plugin.pluginId));
  const queryError =
    pluginsQuery.error instanceof Error
      ? pluginsQuery.error.message
      : pluginsQuery.error
        ? String(pluginsQuery.error)
        : null;
  const marketplaceError =
    marketplacesQuery.error instanceof Error
      ? marketplacesQuery.error.message
      : marketplacesQuery.error
        ? String(marketplacesQuery.error)
        : null;

  const refreshAll = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["claudePlugins"] }),
      queryClient.invalidateQueries({ queryKey: ["claudePluginCatalog"] }),
      queryClient.invalidateQueries({ queryKey: ["claudePluginMarketplaces"] }),
    ]);
  };

  const refreshMarketplaces = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["claudePluginMarketplaces"] }),
      queryClient.invalidateQueries({ queryKey: ["claudePluginCatalog"] }),
    ]);
  };

  const toggleEnabled = async (plugin: ClaudePlugin, enabled: boolean) => {
    setBusyPluginId(plugin.pluginId);
    try {
      await setClaudePluginEnabled(plugin.pluginId, enabled);
      await queryClient.invalidateQueries({ queryKey: ["claudePlugins"] });
      void message.success(t("claudePlugins.toggled"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyPluginId(null);
    }
  };

  const uninstall = async (plugin: ClaudePlugin) => {
    setBusyPluginId(plugin.pluginId);
    try {
      const result = await uninstallClaudePlugin(plugin.pluginId);
      void message.success(result.message || t("claudePlugins.uninstalled"));
      await refreshAll();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyPluginId(null);
    }
  };

  const addMarketplace = async () => {
    if (!marketplaceSource.trim()) return;
    setMarketplaceBusy(true);
    const hide = message.loading(t("claudePlugins.marketplaceAdding"), 0);
    try {
      const result = await addClaudePluginMarketplace(marketplaceSource.trim());
      void message.success(result.message || t("claudePlugins.marketplaceAdded"));
      setMarketplaceSource("");
      await refreshMarketplaces();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setMarketplaceBusy(false);
    }
  };

  const removeMarketplace = async (name: string) => {
    setMarketplaceBusy(true);
    try {
      const result = await removeClaudePluginMarketplace(name);
      void message.success(result.message || t("claudePlugins.marketplaceRemoved"));
      await refreshMarketplaces();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setMarketplaceBusy(false);
    }
  };

  const install = async () => {
    const pluginId = installId?.trim();
    if (!pluginId) return;
    setInstallBusy(true);
    const hide = message.loading(t("claudePlugins.installing"), 0);
    try {
      const result = await installClaudePlugin(pluginId);
      void message.success(result.message || t("claudePlugins.installed"));
      setInstallId(undefined);
      await refreshAll();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setInstallBusy(false);
    }
  };

  const refreshMarketplaceSources = async () => {
    setMarketplaceBusy(true);
    const hide = message.loading(t("claudePlugins.marketplaceUpdating"), 0);
    try {
      const result = await updateClaudePluginMarketplace(null);
      void message.success(result.message || t("claudePlugins.marketplaceUpdated"));
      await refreshMarketplaces();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setMarketplaceBusy(false);
    }
  };

  const checkOneUpdate = async (plugin: ClaudePlugin) => {
    setCheckingPluginId(plugin.pluginId);
    try {
      const status = await checkClaudePluginUpdate(plugin.pluginId);
      setUpdateStatuses((current) => ({ ...current, [plugin.pluginId]: status }));
      void message.info(status.message);
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setCheckingPluginId(null);
    }
  };

  const checkAllUpdates = async () => {
    setCheckingUpdates(true);
    const hide = message.loading(t("claudePlugins.checkingUpdates"), 0);
    try {
      const statuses = await checkClaudePluginUpdates();
      setUpdateStatuses(Object.fromEntries(statuses.map((status) => [status.pluginId, status])));
      const available = statuses.filter((status) => status.status === "update_available").length;
      void message.success(t("claudePlugins.checkUpdatesDone", { available, total: statuses.length }));
      await refreshAll();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setCheckingUpdates(false);
    }
  };

  const applyUpdate = async (plugin: ClaudePlugin) => {
    setUpdatingPluginId(plugin.pluginId);
    const hide = message.loading(t("claudePlugins.updating"), 0);
    try {
      const result = await updateClaudePlugin(plugin.pluginId);
      void message.success(result.message || t("claudePlugins.updated"));
      const status = await checkClaudePluginUpdate(plugin.pluginId);
      setUpdateStatuses((current) => ({ ...current, [plugin.pluginId]: status }));
      await refreshAll();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setUpdatingPluginId(null);
    }
  };

  const updateAvailableIds = plugins
    .filter((plugin) => updateStatuses[plugin.pluginId]?.status === "update_available")
    .map((plugin) => plugin.pluginId);

  const applyAvailableUpdates = async () => {
    if (!updateAvailableIds.length) return;
    for (const pluginId of updateAvailableIds) {
      const plugin = plugins.find((item) => item.pluginId === pluginId);
      if (plugin) await applyUpdate(plugin);
    }
  };

  const catalogOptions = catalog.map((item: ClaudeCatalogPlugin) => ({
    value: item.pluginId,
    // Compact string for the closed Select; rich UI is in optionRender.
    label: installedIds.has(item.pluginId)
      ? `${item.name} @${item.marketplace} · ${t("claudePlugins.alreadyInstalled")}`
      : `${item.name} @${item.marketplace}`,
    name: item.name,
    marketplace: item.marketplace,
    description: item.description ?? "",
    pluginId: item.pluginId,
    installed: installedIds.has(item.pluginId),
  }));

  const marketplaceBusyUi = marketplaceBusy || marketplacesQuery.isFetching;

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <Card
        className="page-surface"
        title={t("claudePlugins.marketplaceTitle")}
        extra={
          <Space>
            <Button
              loading={marketplaceBusy}
              disabled={marketplaceBusyUi}
              onClick={() => void refreshMarketplaceSources()}
            >
              {t("claudePlugins.marketplaceUpdate")}
            </Button>
            <Button
              icon={<ReloadOutlined />}
              loading={marketplaceBusyUi}
              onClick={() => void refreshMarketplaces()}
            >
              {t("claudePlugins.marketplaceRefresh")}
            </Button>
          </Space>
        }
      >
        <Paragraph type="secondary">{t("claudePlugins.marketplaceDescription")}</Paragraph>
        <Space.Compact style={{ width: "100%", marginBottom: 12 }}>
          <Input
            placeholder={t("claudePlugins.marketplaceSourcePlaceholder")}
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
            {t("claudePlugins.marketplaceAdd")}
          </Button>
        </Space.Compact>
        {marketplaceError ? (
          <Alert
            type="error"
            showIcon
            style={{ marginBottom: 12 }}
            message={t("claudePlugins.marketplaceLoadError")}
            description={marketplaceError}
          />
        ) : null}
        <Table
          size="small"
          rowKey="name"
          loading={marketplacesQuery.isLoading || marketplaceBusy}
          dataSource={marketplaces}
          pagination={{
            pageSize: MARKETPLACE_PAGE_SIZE,
            showSizeChanger: true,
            pageSizeOptions: ["5", "10", "20"],
            showTotal: (total) => t("claudePlugins.tableTotal", { total }),
            hideOnSinglePage: false,
          }}
          locale={{
            emptyText: (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t("claudePlugins.marketplaceEmpty")}
              />
            ),
          }}
          columns={[
            { title: t("claudePlugins.marketplace"), dataIndex: "name" },
            {
              title: t("claudePlugins.path"),
              dataIndex: "root",
              ellipsis: true,
              render: (_: unknown, row: ClaudeMarketplace) =>
                row.root || row.source || <Text type="secondary">—</Text>,
            },
            {
              title: t("claudePlugins.actions"),
              width: 100,
              render: (_: unknown, row: ClaudeMarketplace) => (
                <Popconfirm
                  title={t("claudePlugins.confirmRemoveMarketplace")}
                  onConfirm={() => void removeMarketplace(row.name)}
                >
                  <Button size="small" danger icon={<DeleteOutlined />} disabled={marketplaceBusyUi}>
                    {t("claudePlugins.marketplaceRemove")}
                  </Button>
                </Popconfirm>
              ),
            },
          ]}
        />
      </Card>

      <Card
        className="page-surface"
        title={t("claudePlugins.title")}
        extra={
          <Space>
            <Button
              loading={checkingUpdates}
              disabled={installBusy || marketplaceBusy || plugins.length === 0}
              onClick={() => void checkAllUpdates()}
            >
              {t("claudePlugins.checkAllUpdates")}
            </Button>
            <Button
              type="primary"
              disabled={!updateAvailableIds.length || checkingUpdates || updatingPluginId !== null}
              onClick={() => void applyAvailableUpdates()}
            >
              {t("claudePlugins.updateSelected", { count: updateAvailableIds.length })}
            </Button>
            <Button
              icon={<ReloadOutlined />}
              loading={pluginsQuery.isFetching || catalogQuery.isFetching}
              onClick={() => void refreshAll()}
            >
              {t("claudePlugins.refresh")}
            </Button>
          </Space>
        }
      >
        <Paragraph type="secondary">{t("claudePlugins.description")}</Paragraph>
        <div
          style={{
            display: "flex",
            alignItems: "center",
            width: "100%",
            maxWidth: 640,
            marginBottom: 8,
            gap: 8,
          }}
        >
          <Select
            showSearch
            allowClear
            style={{ flex: 1, minWidth: 0 }}
            placeholder={t("claudePlugins.installSelectPlaceholder")}
            value={installId}
            loading={catalogQuery.isLoading}
            disabled={installBusy}
            options={catalogOptions}
            optionFilterProp="pluginId"
            optionRender={(option) => {
              const data = option.data as (typeof catalogOptions)[number];
              return (
                <div style={{ lineHeight: 1.35, padding: "2px 0", whiteSpace: "normal" }}>
                  <div>
                    <Text>{data.name}</Text>
                    <Text type="secondary"> @{data.marketplace}</Text>
                    {data.installed ? (
                      <Text type="success"> · {t("claudePlugins.alreadyInstalled")}</Text>
                    ) : null}
                  </div>
                  {data.description ? (
                    <Text
                      type="secondary"
                      style={{
                        display: "-webkit-box",
                        WebkitLineClamp: 2,
                        WebkitBoxOrient: "vertical",
                        overflow: "hidden",
                        fontSize: 12,
                        whiteSpace: "normal",
                      }}
                    >
                      {data.description}
                    </Text>
                  ) : null}
                </div>
              );
            }}
            filterOption={(input, option) => {
              const q = input.toLowerCase();
              const pluginId = String(option?.pluginId ?? option?.value ?? "").toLowerCase();
              const name = String(option?.name ?? "").toLowerCase();
              const marketplace = String(option?.marketplace ?? "").toLowerCase();
              const description = String(option?.description ?? "").toLowerCase();
              return (
                pluginId.includes(q) ||
                name.includes(q) ||
                marketplace.includes(q) ||
                description.includes(q)
              );
            }}
            onChange={(value) => setInstallId(value)}
            notFoundContent={
              catalogQuery.isLoading ? null : (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={t("claudePlugins.catalogEmpty")}
                />
              )
            }
          />
          <Button
            type="primary"
            icon={<PlusOutlined />}
            loading={installBusy}
            disabled={!installId?.trim()}
            onClick={() => void install()}
            style={{ flexShrink: 0 }}
          >
            {t("claudePlugins.install")}
          </Button>
        </div>
        <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
          {t("claudePlugins.installHint")}
        </Text>
        {queryError ? (
          <Alert
            type="error"
            showIcon
            style={{ marginBottom: 16 }}
            message={t("claudePlugins.loadError")}
            description={queryError}
          />
        ) : null}
        {!queryError && snapshot && !snapshot.parseOk ? (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 16 }}
            message={t("claudePlugins.parseError")}
            description={snapshot.parseError ?? undefined}
          />
        ) : null}
        {!queryError && snapshot && plugins.length === 0 ? (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 16 }}
            message={t("claudePlugins.empty")}
            description={t("claudePlugins.emptyHint", {
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
          pagination={{
            pageSize: PLUGIN_PAGE_SIZE,
            showSizeChanger: true,
            pageSizeOptions: ["8", "15", "30"],
            showTotal: (total) => t("claudePlugins.tableTotal", { total }),
            hideOnSinglePage: false,
          }}
          locale={{
            emptyText: (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={queryError ? t("claudePlugins.loadError") : t("claudePlugins.empty")}
              />
            ),
          }}
          columns={[
            {
              title: t("claudePlugins.name"),
              dataIndex: "name",
              render: (name: string, plugin: ClaudePlugin) => (
                <Space direction="vertical" size={0}>
                  <Text strong>{name}</Text>
                  <Text type="secondary">{plugin.pluginId}</Text>
                </Space>
              ),
            },
            {
              title: t("claudePlugins.marketplace"),
              dataIndex: "marketplace",
              width: 180,
            },
            {
              title: t("claudePlugins.version"),
              dataIndex: "version",
              width: 120,
              render: (version?: string | null) => version ?? "—",
            },
            {
              title: t("claudePlugins.updateStatus"),
              width: 150,
              render: (_: unknown, plugin: ClaudePlugin) => (
                <PluginUpdateStatusTag status={updateStatuses[plugin.pluginId]} t={t} ns="claudePlugins" />
              ),
            },
            {
              title: t("claudePlugins.path"),
              dataIndex: "path",
              ellipsis: true,
              render: (path?: string | null) =>
                path ? <Text copyable={{ text: path }}>{path}</Text> : <Text type="secondary">—</Text>,
            },
            {
              title: t("claudePlugins.enabled"),
              width: 100,
              render: (_: unknown, plugin: ClaudePlugin) => (
                <Switch
                  checked={plugin.enabled}
                  loading={busyPluginId === plugin.pluginId}
                  disabled={busyPluginId !== null && busyPluginId !== plugin.pluginId}
                  onChange={(checked) => void toggleEnabled(plugin, checked)}
                />
              ),
            },
            {
              title: t("claudePlugins.actions"),
              width: 220,
              render: (_: unknown, plugin: ClaudePlugin) => (
                <Space size="small" wrap>
                  <Button
                    type="link"
                    size="small"
                    loading={checkingPluginId === plugin.pluginId}
                    disabled={
                      checkingUpdates ||
                      (checkingPluginId !== null && checkingPluginId !== plugin.pluginId) ||
                      updatingPluginId !== null
                    }
                    onClick={() => void checkOneUpdate(plugin)}
                  >
                    {t("claudePlugins.checkUpdate")}
                  </Button>
                  {(updateStatuses[plugin.pluginId]?.status === "update_available" ||
                    updateStatuses[plugin.pluginId]?.status === "unknown") && (
                    <Button
                      type="link"
                      size="small"
                      loading={updatingPluginId === plugin.pluginId}
                      disabled={updatingPluginId !== null && updatingPluginId !== plugin.pluginId}
                      onClick={() => void applyUpdate(plugin)}
                    >
                      {t("claudePlugins.update")}
                    </Button>
                  )}
                  <Popconfirm
                    title={t("claudePlugins.confirmUninstall")}
                    onConfirm={() => void uninstall(plugin)}
                  >
                    <Button
                      size="small"
                      danger
                      icon={<DeleteOutlined />}
                      loading={busyPluginId === plugin.pluginId}
                      disabled={busyPluginId !== null && busyPluginId !== plugin.pluginId}
                    >
                      {t("claudePlugins.uninstall")}
                    </Button>
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
      </Card>
    </Space>
  );
}

function PluginUpdateStatusTag({
  status,
  t,
  ns,
}: {
  status?: ClaudePluginUpdateStatus;
  t: (key: string) => string;
  ns: "claudePlugins";
}) {
  if (!status) return <Text type="secondary">—</Text>;
  const color =
    status.status === "update_available"
      ? "orange"
      : status.status === "up_to_date"
        ? "green"
        : status.status === "error"
          ? "red"
          : "default";
  const label =
    status.status === "update_available"
      ? t(`${ns}.updateAvailable`)
      : status.status === "up_to_date"
        ? t(`${ns}.upToDate`)
        : status.message;
  return (
    <Text title={status.message}>
      <Tag color={color}>{label}</Tag>
    </Text>
  );
}
