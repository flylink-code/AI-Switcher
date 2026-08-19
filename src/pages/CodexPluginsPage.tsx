import { useState } from "react";
import { Alert, Button, Card, Empty, Input, Popconfirm, Select, Space, Switch, Table, Tag, Typography, message } from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  codexPluginCatalogOptions,
  codexPluginMarketplacesOptions,
  codexPluginsOptions,
} from "@/lib/appQueries";
import {
  addCodexPluginMarketplace,
  checkCodexPluginUpdate,
  checkCodexPluginUpdates,
  installCodexPlugin,
  removeCodexPluginMarketplace,
  setCodexPluginEnabled,
  uninstallCodexPlugin,
  updateCodexPlugin,
  upgradeCodexPluginMarketplace,
} from "@/services/api";
import type { CodexCatalogPlugin, CodexMarketplace, CodexPlugin, CodexPluginUpdateStatus } from "@/types/backend";

const { Paragraph, Text } = Typography;

const MARKETPLACE_PAGE_SIZE = 5;
const PLUGIN_PAGE_SIZE = 8;

export default function CodexPluginsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const pluginsQuery = useQuery(codexPluginsOptions);
  const catalogQuery = useQuery(codexPluginCatalogOptions);
  const marketplacesQuery = useQuery(codexPluginMarketplacesOptions);
  const [busyPluginId, setBusyPluginId] = useState<string | null>(null);
  const [marketplaceBusy, setMarketplaceBusy] = useState(false);
  const [marketplaceSource, setMarketplaceSource] = useState("");
  const [installId, setInstallId] = useState<string | undefined>(undefined);
  const [installBusy, setInstallBusy] = useState(false);
  const [updateStatuses, setUpdateStatuses] = useState<Record<string, CodexPluginUpdateStatus>>({});
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
      queryClient.invalidateQueries({ queryKey: ["codexPlugins"] }),
      queryClient.invalidateQueries({ queryKey: ["codexPluginCatalog"] }),
      queryClient.invalidateQueries({ queryKey: ["codexPluginMarketplaces"] }),
    ]);
  };

  const refreshMarketplaces = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["codexPluginMarketplaces"] }),
      queryClient.invalidateQueries({ queryKey: ["codexPluginCatalog"] }),
    ]);
  };

  const toggleEnabled = async (plugin: CodexPlugin, enabled: boolean) => {
    setBusyPluginId(plugin.pluginId);
    try {
      await setCodexPluginEnabled(plugin.pluginId, enabled);
      await queryClient.invalidateQueries({ queryKey: ["codexPlugins"] });
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
    const hide = message.loading(t("codexPlugins.marketplaceAdding"), 0);
    try {
      const result = await addCodexPluginMarketplace(marketplaceSource.trim());
      void message.success(result.message || t("codexPlugins.marketplaceAdded"));
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
      const result = await removeCodexPluginMarketplace(name);
      void message.success(result.message || t("codexPlugins.marketplaceRemoved"));
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
    const hide = message.loading(t("codexPlugins.installing"), 0);
    try {
      const result = await installCodexPlugin(pluginId);
      void message.success(result.message || t("codexPlugins.installed"));
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
    const hide = message.loading(t("codexPlugins.marketplaceUpdating"), 0);
    try {
      const result = await upgradeCodexPluginMarketplace(null);
      void message.success(result.message || t("codexPlugins.marketplaceUpdated"));
      await refreshMarketplaces();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setMarketplaceBusy(false);
    }
  };

  const checkOneUpdate = async (plugin: CodexPlugin) => {
    setCheckingPluginId(plugin.pluginId);
    try {
      const status = await checkCodexPluginUpdate(plugin.pluginId);
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
    const hide = message.loading(t("codexPlugins.checkingUpdates"), 0);
    try {
      const statuses = await checkCodexPluginUpdates();
      setUpdateStatuses(Object.fromEntries(statuses.map((status) => [status.pluginId, status])));
      const available = statuses.filter((status) => status.status === "update_available").length;
      void message.success(t("codexPlugins.checkUpdatesDone", { available, total: statuses.length }));
      await refreshAll();
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      hide();
      setCheckingUpdates(false);
    }
  };

  const applyUpdate = async (plugin: CodexPlugin) => {
    setUpdatingPluginId(plugin.pluginId);
    const hide = message.loading(t("codexPlugins.updating"), 0);
    try {
      const result = await updateCodexPlugin(plugin.pluginId);
      void message.success(result.message || t("codexPlugins.updated"));
      const status = await checkCodexPluginUpdate(plugin.pluginId);
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

  const catalogOptions = catalog.map((item: CodexCatalogPlugin) => ({
    value: item.pluginId,
    label: installedIds.has(item.pluginId)
      ? `${item.name} @${item.marketplace} · ${t("codexPlugins.alreadyInstalled")}`
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
        title={t("codexPlugins.marketplaceTitle")}
        extra={
          <Space>
            <Button
              loading={marketplaceBusy}
              disabled={marketplaceBusyUi}
              onClick={() => void refreshMarketplaceSources()}
            >
              {t("codexPlugins.marketplaceUpdate")}
            </Button>
            <Button
              icon={<ReloadOutlined />}
              loading={marketplaceBusyUi}
              onClick={() => void refreshMarketplaces()}
            >
              {t("codexPlugins.marketplaceRefresh")}
            </Button>
          </Space>
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
        {marketplaceError ? (
          <Alert
            type="error"
            showIcon
            style={{ marginBottom: 12 }}
            message={t("codexPlugins.marketplaceLoadError")}
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
            showTotal: (total) => t("codexPlugins.tableTotal", { total }),
            hideOnSinglePage: false,
          }}
          locale={{
            emptyText: (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t("codexPlugins.marketplaceEmpty")}
              />
            ),
          }}
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
              title: t("codexPlugins.actions"),
              width: 100,
              render: (_: unknown, row: CodexMarketplace) => (
                <Popconfirm
                  title={t("codexPlugins.confirmRemoveMarketplace")}
                  onConfirm={() => void removeMarketplace(row.name)}
                >
                  <Button size="small" danger icon={<DeleteOutlined />} disabled={marketplaceBusyUi}>
                    {t("codexPlugins.marketplaceRemove")}
                  </Button>
                </Popconfirm>
              ),
            },
          ]}
        />
      </Card>

      <Card
        className="page-surface"
        title={t("codexPlugins.title")}
        extra={
          <Space>
            <Button
              loading={checkingUpdates}
              disabled={installBusy || marketplaceBusy || plugins.length === 0}
              onClick={() => void checkAllUpdates()}
            >
              {t("codexPlugins.checkAllUpdates")}
            </Button>
            <Button
              type="primary"
              disabled={!updateAvailableIds.length || checkingUpdates || updatingPluginId !== null}
              onClick={() => void applyAvailableUpdates()}
            >
              {t("codexPlugins.updateSelected", { count: updateAvailableIds.length })}
            </Button>
            <Button
              icon={<ReloadOutlined />}
              loading={pluginsQuery.isFetching || catalogQuery.isFetching}
              onClick={() => void refreshAll()}
            >
              {t("codexPlugins.refresh")}
            </Button>
          </Space>
        }
      >
        <Paragraph type="secondary">{t("codexPlugins.description")}</Paragraph>
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
            placeholder={t("codexPlugins.installSelectPlaceholder")}
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
                      <Text type="success"> · {t("codexPlugins.alreadyInstalled")}</Text>
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
                  description={
                    marketplaces.length > 0
                      ? t("codexPlugins.catalogEmptyWithMarkets")
                      : t("codexPlugins.catalogEmpty")
                  }
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
            {t("codexPlugins.install")}
          </Button>
        </div>
        <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
          {t("codexPlugins.installHint")}
        </Text>
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
          pagination={{
            pageSize: PLUGIN_PAGE_SIZE,
            showSizeChanger: true,
            pageSizeOptions: ["8", "15", "30"],
            showTotal: (total) => t("codexPlugins.tableTotal", { total }),
            hideOnSinglePage: false,
          }}
          locale={{
            emptyText: (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={queryError ? t("codexPlugins.loadError") : t("codexPlugins.empty")}
              />
            ),
          }}
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
              title: t("codexPlugins.updateStatus"),
              width: 150,
              render: (_: unknown, plugin: CodexPlugin) => (
                <PluginUpdateStatusTag status={updateStatuses[plugin.pluginId]} t={t} ns="codexPlugins" />
              ),
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
              width: 220,
              render: (_: unknown, plugin: CodexPlugin) => (
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
                    {t("codexPlugins.checkUpdate")}
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
                      {t("codexPlugins.update")}
                    </Button>
                  )}
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
  status?: CodexPluginUpdateStatus;
  t: (key: string) => string;
  ns: "codexPlugins";
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
