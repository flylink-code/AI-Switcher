import { useMemo } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Popconfirm,
  Skeleton,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import DownloadOutlined from "@ant-design/icons/es/icons/DownloadOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  downloadDesktopLocalizationPack,
  installClaudeCodeLocalization,
  installDesktopLocalization,
  installEditorLocalizationHelper,
  restoreDesktopLocalization,
  selectDesktopLocalizationPack,
  validateDesktopLocalizationPack,
} from "@/services/api";
import { localizationHubOptions, localizationOptions } from "@/lib/appQueries";
import { OnboardingTip } from "@/components/OnboardingTip";

const { Text } = Typography;

function PathValue({ value }: { value?: string | null }) {
  const { t } = useTranslation();
  if (!value) return <Tag>{t("env.notDetected")}</Tag>;
  return <Text copyable code style={{ wordBreak: "break-all" }}>{value}</Text>;
}

export default function DesktopLocalizationPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const statusQuery = useQuery(localizationOptions);
  const hubQuery = useQuery(localizationHubOptions);
  const localization = statusQuery.data;

  const refreshStatus = async () => {
    await queryClient.invalidateQueries({ queryKey: localizationOptions.queryKey });
  };

  const selectPack = useMutation({
    mutationFn: async () => {
      const path = await selectDesktopLocalizationPack();
      return path ? validateDesktopLocalizationPack(path) : null;
    },
    onSuccess: async (result) => {
      if (!result) return;
      void message.success(result.message);
      await refreshStatus();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const downloadPack = useMutation({
    mutationFn: downloadDesktopLocalizationPack,
    onSuccess: async (result) => {
      void message.success(
        t("env.localization.downloadSuccess", {
          version: result.version ?? result.revision?.slice(0, 12) ?? "latest",
        }),
      );
      await refreshStatus();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const install = useMutation({
    mutationFn: async () => {
      if (!localization?.packPath) throw new Error(t("env.localization.selectPack"));
      return installDesktopLocalization(localization.packPath);
    },
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshStatus();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshStatus();
    },
  });

  const restore = useMutation({
    mutationFn: restoreDesktopLocalization,
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshStatus();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshStatus();
    },
  });

  const installClaudeCode = useMutation({
    mutationFn: installClaudeCodeLocalization,
    onSuccess: async (result) => {
      void message.success(result);
      await queryClient.invalidateQueries({ queryKey: localizationHubOptions.queryKey });
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const installEditorHelper = useMutation({
    mutationFn: installEditorLocalizationHelper,
    onSuccess: async (result) => {
      void message.success(result);
      await queryClient.invalidateQueries({ queryKey: localizationHubOptions.queryKey });
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const busy =
    selectPack.isPending ||
    downloadPack.isPending ||
    install.isPending ||
    restore.isPending ||
    installClaudeCode.isPending ||
    installEditorHelper.isPending;
  const diagnostics = useMemo(
    () => localization?.diagnostics.filter(Boolean).join("\n") ?? "",
    [localization?.diagnostics],
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <OnboardingTip
        tipKey="localization"
        message={t("env.localization.hubTitle")}
        description={t("env.localization.hubDescription")}
      />
      <Card
        size="small"
        title={t("env.localization.claudeCodeTitle")}
        extra={<Button size="small" icon={<ReloadOutlined spin={hubQuery.isFetching} />} disabled={busy} onClick={() => void hubQuery.refetch()}>{t("common.refresh")}</Button>}
      >
        {hubQuery.isPending ? <Skeleton active paragraph={{ rows: 3 }} /> : hubQuery.error ? (
          <Alert type="error" showIcon message={errorMessage(hubQuery.error)} />
        ) : (
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label={t("env.localization.status")}>
              <Tag color={hubQuery.data?.claudeCode.pluginEnabled ? "green" : "default"}>
                {hubQuery.data?.claudeCode.pluginEnabled ? t("env.localization.configured") : t("env.localization.notConfigured")}
              </Tag>
              <Text type="secondary"> {hubQuery.data?.claudeCode.message}</Text>
            </Descriptions.Item>
            <Descriptions.Item label={t("env.localization.version")}>{hubQuery.data?.claudeCode.version ?? "—"}</Descriptions.Item>
            <Descriptions.Item label={t("env.localization.installPath")}><PathValue value={hubQuery.data?.claudeCode.executablePath} /></Descriptions.Item>
            <Descriptions.Item label={t("env.localization.actions")}>
              <Popconfirm
                title={t("env.localization.confirmCodeInstall")}
                description={t("env.localization.confirmCodeInstallDescription")}
                onConfirm={() => installClaudeCode.mutate()}
              >
                <Button type="primary" loading={installClaudeCode.isPending} disabled={busy || !hubQuery.data?.claudeCode.installed}>
                  {t("env.localization.installCode")}
                </Button>
              </Popconfirm>
            </Descriptions.Item>
          </Descriptions>
        )}
      </Card>
      <Card size="small" title={t("env.localization.editorTitle")}>
        {hubQuery.isPending ? <Skeleton active paragraph={{ rows: 3 }} /> : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            {hubQuery.data?.editors.map((editor) => (
              <Card
                key={editor.id}
                type="inner"
                size="small"
                title={editor.label}
                extra={
                  <Popconfirm
                    title={t("env.localization.confirmEditorInstall", { editor: editor.label })}
                    description={t("env.localization.confirmEditorInstallDescription")}
                    onConfirm={() => installEditorHelper.mutate(editor.id)}
                  >
                    <Button
                      loading={installEditorHelper.isPending && installEditorHelper.variables === editor.id}
                      disabled={busy || !editor.claudeExtensionPath || !editor.editorCliPath}
                    >
                      {editor.helperInstalled ? t("env.localization.reinstallHelper") : t("env.localization.installHelper")}
                    </Button>
                  </Popconfirm>
                }
              >
                <Descriptions column={1} size="small">
                  <Descriptions.Item label={t("env.localization.status")}>
                    <Tag color={editor.claudeExtensionPath ? "green" : "default"}>{editor.claudeExtensionPath ? t("env.localization.detected") : t("env.localization.notDetected")}</Tag>
                    <Text type="secondary"> {editor.message}</Text>
                  </Descriptions.Item>
                  <Descriptions.Item label={t("env.localization.extensionPath")}><PathValue value={editor.claudeExtensionPath} /></Descriptions.Item>
                  <Descriptions.Item label={t("env.localization.editorCliPath")}><PathValue value={editor.editorCliPath} /></Descriptions.Item>
                </Descriptions>
              </Card>
            ))}
          </Space>
        )}
      </Card>
      <Alert
        type="info"
        showIcon
        message={t("env.localization.safeMode")}
        description={t("env.localization.safeModeDescription")}
      />
      <Alert
        type="warning"
        showIcon
        message={t("env.localization.thirdPartyTitle")}
        description={
          <Space direction="vertical" size={0}>
            <Text>{t("env.localization.thirdPartyDescription")}</Text>
            <Button
              type="link"
              size="small"
              style={{ paddingInline: 0, alignSelf: "flex-start" }}
              onClick={() =>
                void openUrl("https://github.com/javaht/claude-desktop-zh-cn")
              }
            >
              {t("env.localization.openRepository")}
            </Button>
          </Space>
        }
      />
      <Card
        size="small"
        title={t("env.localization.title")}
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined spin={statusQuery.isFetching} />}
            disabled={busy}
            onClick={() => void statusQuery.refetch()}
          >
            {t("common.refresh")}
          </Button>
        }
      >
        {statusQuery.isPending ? (
          <Skeleton active paragraph={{ rows: 6 }} />
        ) : statusQuery.error ? (
          <Alert type="error" showIcon message={errorMessage(statusQuery.error)} />
        ) : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            {localization?.multipleInstalls && (
              <Alert type="warning" showIcon message={t("env.localization.multipleInstalls")} />
            )}
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("env.localization.status")}>
                <Tag
                  color={
                    localization?.state === "installed"
                      ? "green"
                      : localization?.state === "partial"
                        ? "orange"
                        : "default"
                  }
                >
                  {localization
                    ? t(`env.localization.states.${localization.state}`)
                    : t("env.localization.loading")}
                </Tag>
                {localization?.message && <Text type="secondary"> {localization.message}</Text>}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.version")}>
                {localization?.claudeVersion ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.installPath")}>
                <PathValue value={localization?.installPath} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.locale")}>
                {localization?.configuredLocale ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packPath")}>
                <PathValue value={localization?.packPath} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packSource")}>
                {localization?.packSource ? (
                  <Tag color={localization.packSource === "github" ? "blue" : "default"}>
                    {t(`env.localization.packSources.${localization.packSource}`)}
                  </Tag>
                ) : (
                  "—"
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packVersion")}>
                {localization?.packVersion ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packRevision")}>
                {localization?.packRevision ? (
                  <Text copyable={{ text: localization.packRevision }} code>
                    {localization.packRevision.slice(0, 12)}
                  </Text>
                ) : (
                  "—"
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packFetchedAt")}>
                {localization?.packFetchedAt
                  ? new Date(localization.packFetchedAt).toLocaleString()
                  : "—"}
              </Descriptions.Item>
            </Descriptions>
            {!localization?.installDetected && diagnostics && (
              <Alert
                type="warning"
                showIcon
                message={localization?.message ?? t("env.localization.statusFailed")}
                description={<pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{diagnostics}</pre>}
              />
            )}
            <Space wrap>
              <Popconfirm
                title={t("env.localization.confirmDownload")}
                description={t("env.localization.confirmDownloadDescription")}
                onConfirm={() => downloadPack.mutate()}
              >
                <Button
                  icon={<DownloadOutlined />}
                  loading={downloadPack.isPending}
                  disabled={busy && !downloadPack.isPending}
                >
                  {localization?.packSource === "github"
                    ? t("env.localization.updatePack")
                    : t("env.localization.downloadPack")}
                </Button>
              </Popconfirm>
              <Button
                icon={<FolderOpenOutlined />}
                loading={selectPack.isPending}
                disabled={busy && !selectPack.isPending}
                onClick={() => selectPack.mutate()}
              >
                {t("env.localization.selectPack")}
              </Button>
              <Popconfirm
                title={t("env.localization.confirmInstall")}
                description={t("env.localization.confirmInstallDescription")}
                onConfirm={() => install.mutate()}
              >
                <Button
                  type="primary"
                  loading={install.isPending}
                  disabled={
                    busy ||
                    !localization?.platformSupported ||
                    !localization.installDetected ||
                    !localization.packValid
                  }
                >
                  {t("env.localization.install")}
                </Button>
              </Popconfirm>
              <Popconfirm
                title={t("env.localization.confirmRestore")}
                onConfirm={() => restore.mutate()}
              >
                <Button
                  danger
                  loading={restore.isPending}
                  disabled={busy || !localization?.backupAvailable}
                >
                  {t("env.localization.restore")}
                </Button>
              </Popconfirm>
            </Space>
          </Space>
        )}
      </Card>
    </Space>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
