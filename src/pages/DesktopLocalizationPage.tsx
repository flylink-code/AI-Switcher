import { useMemo, type ReactNode } from "react";
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
  uninstallClaudeCodeLocalization,
  uninstallEditorLocalizationHelper,
  updateClaudeCodeLocalization,
  updateDesktopLocalization,
  updateEditorLocalizationHelper,
  validateDesktopLocalizationPack,
} from "@/services/api";
import {
  localizationHubOptions,
  localizationOptions,
  localizationUpstreamOptions,
} from "@/lib/appQueries";
import { OnboardingTip } from "@/components/OnboardingTip";
import type {
  EditorLocalizationStatus,
  LocalizationUpstreamRelease,
} from "@/types/backend";

const { Text } = Typography;

type VersionRelation = "newer" | "same" | "unknown";

function PathValue({ value }: { value?: string | null }) {
  const { t } = useTranslation();
  if (!value) return <Tag>{t("env.notDetected")}</Tag>;
  return <Text copyable code style={{ wordBreak: "break-all" }}>{value}</Text>;
}

function normalizeResourceVersion(value?: string | null): string | null {
  const trimmed = value?.trim().replace(/^v/i, "");
  return trimmed ? trimmed : null;
}

function compareResourceVersion(
  local?: string | null,
  remote?: string | null,
): VersionRelation {
  const from = normalizeResourceVersion(local);
  const to = normalizeResourceVersion(remote);
  if (!to) return "unknown";
  if (!from) return "newer";
  if (from === to) return "same";
  const fromParts = from.split(/[.-]/).map((part) => Number.parseInt(part, 10));
  const toParts = to.split(/[.-]/).map((part) => Number.parseInt(part, 10));
  if (fromParts.some(Number.isNaN) || toParts.some(Number.isNaN)) {
    return "unknown";
  }
  const length = Math.max(fromParts.length, toParts.length);
  for (let index = 0; index < length; index += 1) {
    const a = fromParts[index] ?? 0;
    const b = toParts[index] ?? 0;
    if (b > a) return "newer";
    if (b < a) return "same";
  }
  return "same";
}

function updateButtonLabel(
  t: (key: string, options?: Record<string, string>) => string,
  local?: string | null,
  remote?: string | null,
): string {
  const from = normalizeResourceVersion(local);
  const to = normalizeResourceVersion(remote);
  if (from && to && from !== to) {
    return t("env.localization.updateResourceVersions", { from, to });
  }
  return t("env.localization.updateResource");
}

function upstreamDisplay(
  release: LocalizationUpstreamRelease | undefined,
  checking: boolean,
  failed: boolean,
  t: (key: string) => string,
): string {
  if (checking && !release) return t("env.localization.onlineChecking");
  if (failed && !release) return t("env.localization.onlineUnavailable");
  if (!release) return t("env.localization.onlineUnavailable");
  if (release.available && release.version) return release.version;
  return t("env.localization.onlineUnavailable");
}

function ResourceActions({ children }: { children: ReactNode }) {
  return <Space wrap>{children}</Space>;
}

export default function DesktopLocalizationPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const statusQuery = useQuery(localizationOptions);
  const hubQuery = useQuery(localizationHubOptions);
  const upstreamQuery = useQuery(localizationUpstreamOptions);
  const localization = statusQuery.data;
  const hub = hubQuery.data;
  const upstream = upstreamQuery.data;

  const refreshHub = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: localizationHubOptions.queryKey }),
      queryClient.invalidateQueries({ queryKey: localizationUpstreamOptions.queryKey }),
    ]);
  };

  const refreshDesktop = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: localizationOptions.queryKey }),
      queryClient.invalidateQueries({ queryKey: localizationUpstreamOptions.queryKey }),
    ]);
  };

  const selectPack = useMutation({
    mutationFn: async () => {
      const path = await selectDesktopLocalizationPack();
      return path ? validateDesktopLocalizationPack(path) : null;
    },
    onSuccess: async (result) => {
      if (!result) return;
      void message.success(result.message);
      await refreshDesktop();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const installDesktop = useMutation({
    mutationFn: async () => {
      let packPath = localization?.packPath;
      if (!localization?.packValid || !packPath) {
        const downloaded = await downloadDesktopLocalizationPack();
        packPath = downloaded.packPath;
      }
      if (!packPath) throw new Error(t("env.localization.selectPack"));
      return installDesktopLocalization(packPath);
    },
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshDesktop();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshDesktop();
    },
  });

  const updateDesktop = useMutation({
    mutationFn: updateDesktopLocalization,
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshDesktop();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshDesktop();
    },
  });

  const restore = useMutation({
    mutationFn: restoreDesktopLocalization,
    onSuccess: async (result) => {
      void message.success(result.message);
      await refreshDesktop();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshDesktop();
    },
  });

  const installClaudeCode = useMutation({
    mutationFn: installClaudeCodeLocalization,
    onSuccess: async (result) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const updateClaudeCode = useMutation({
    mutationFn: updateClaudeCodeLocalization,
    onSuccess: async (result) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const uninstallClaudeCode = useMutation({
    mutationFn: uninstallClaudeCodeLocalization,
    onSuccess: async (result) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const installEditorHelper = useMutation({
    mutationFn: installEditorLocalizationHelper,
    onSuccess: async (result) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const updateEditorHelper = useMutation({
    mutationFn: updateEditorLocalizationHelper,
    onSuccess: async (result) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const uninstallEditorHelper = useMutation({
    mutationFn: uninstallEditorLocalizationHelper,
    onSuccess: async (result) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error) => void message.error(errorMessage(error)),
  });

  const busy =
    selectPack.isPending ||
    installDesktop.isPending ||
    updateDesktop.isPending ||
    restore.isPending ||
    installClaudeCode.isPending ||
    updateClaudeCode.isPending ||
    uninstallClaudeCode.isPending ||
    installEditorHelper.isPending ||
    updateEditorHelper.isPending ||
    uninstallEditorHelper.isPending;
  const diagnostics = useMemo(
    () => localization?.diagnostics.filter(Boolean).join("\n") ?? "",
    [localization?.diagnostics],
  );

  const codeResourceInstalled = Boolean(
    hub?.claudeCode.pluginEnabled || hub?.claudeCode.pluginVersion,
  );
  const codeRelation = compareResourceVersion(
    hub?.claudeCode.pluginVersion,
    upstream?.claudeCode.version,
  );
  const desktopInstalled = localization?.state === "installed";
  const desktopRelation = compareResourceVersion(
    localization?.packVersion,
    upstream?.desktop.version,
  );

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <OnboardingTip
        tipKey="localization"
        message={t("env.localization.hubTitle")}
        description={t("env.localization.hubDescription")}
      />
      <OnboardingTip
        tipKey="localization_third_party"
        type="warning"
        message={t("env.localization.thirdPartyTitle")}
        description={
          <Space direction="vertical" size={0}>
            <Text>{t("env.localization.thirdPartyDescription")}</Text>
            <Space wrap>
              <Button
                type="link"
                size="small"
                style={{ paddingInline: 0 }}
                onClick={() => void openUrl("https://github.com/taekchef/claude-code-zh-cn")}
              >
                Claude Code
              </Button>
              <Button
                type="link"
                size="small"
                style={{ paddingInline: 0 }}
                onClick={() => void openUrl("https://github.com/shanjiancaofu/claude-code-vscode-zh-cn")}
              >
                VS Code / Cursor
              </Button>
              <Button
                type="link"
                size="small"
                style={{ paddingInline: 0 }}
                onClick={() => void openUrl("https://github.com/javaht/claude-desktop-zh-cn")}
              >
                Claude Desktop
              </Button>
            </Space>
          </Space>
        }
      />

      <Card
        size="small"
        className="page-surface"
        title={t("env.localization.claudeCodeTitle")}
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined spin={hubQuery.isFetching || upstreamQuery.isFetching} />}
            disabled={busy}
            onClick={() => void refreshHub()}
          >
            {t("common.refresh")}
          </Button>
        }
      >
        {hubQuery.isPending ? (
          <Skeleton active paragraph={{ rows: 4 }} />
        ) : hubQuery.error ? (
          <Alert type="error" showIcon message={errorMessage(hubQuery.error)} />
        ) : (
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label={t("env.localization.status")}>
              <Tag color={codeResourceInstalled ? "green" : "default"}>
                {codeResourceInstalled
                  ? t("env.localization.configured")
                  : t("env.localization.notConfigured")}
              </Tag>
              <Text type="secondary"> {hub?.claudeCode.message}</Text>
            </Descriptions.Item>
            <Descriptions.Item label={t("env.localization.hostVersion")}>
              {hub?.claudeCode.version ?? "—"}
            </Descriptions.Item>
            <Descriptions.Item label={t("env.localization.installPath")}>
              <PathValue value={hub?.claudeCode.executablePath} />
            </Descriptions.Item>
            <Descriptions.Item label={t("env.localization.localResourceVersion")}>
              {hub?.claudeCode.pluginVersion ?? t("env.localization.notInstalledResource")}
            </Descriptions.Item>
            <Descriptions.Item label={t("env.localization.upstreamVersion")}>
              {upstreamDisplay(
                upstream?.claudeCode,
                upstreamQuery.isFetching,
                Boolean(upstreamQuery.error),
                t,
              )}
            </Descriptions.Item>
            <Descriptions.Item label={t("env.localization.actions")}>
              <ResourceActions>
                <Popconfirm
                  title={t("env.localization.confirmCodeInstall")}
                  description={t("env.localization.confirmCodeInstallDescription")}
                  onConfirm={() => installClaudeCode.mutate()}
                >
                  <Button
                    type="primary"
                    loading={installClaudeCode.isPending}
                    disabled={busy || !hub?.claudeCode.installed || codeResourceInstalled}
                  >
                    {t("env.localization.installChinese")}
                  </Button>
                </Popconfirm>
                <Popconfirm
                  title={t("env.localization.confirmCodeUpdate")}
                  description={t("env.localization.confirmCodeUpdateDescription")}
                  onConfirm={() => updateClaudeCode.mutate()}
                >
                  <Button
                    loading={updateClaudeCode.isPending}
                    disabled={
                      busy ||
                      !hub?.claudeCode.installed ||
                      !codeResourceInstalled ||
                      codeRelation === "same"
                    }
                  >
                    {codeRelation === "same"
                      ? t("env.localization.alreadyLatest")
                      : updateButtonLabel(
                          t,
                          hub?.claudeCode.pluginVersion,
                          upstream?.claudeCode.version,
                        )}
                  </Button>
                </Popconfirm>
                <Popconfirm
                  title={t("env.localization.confirmCodeUninstall")}
                  description={t("env.localization.confirmCodeUninstallDescription")}
                  onConfirm={() => uninstallClaudeCode.mutate()}
                >
                  <Button
                    danger
                    loading={uninstallClaudeCode.isPending}
                    disabled={busy || !hub?.claudeCode.installed || !codeResourceInstalled}
                  >
                    {t("env.localization.uninstallChinese")}
                  </Button>
                </Popconfirm>
              </ResourceActions>
            </Descriptions.Item>
          </Descriptions>
        )}
      </Card>

      <Card size="small" className="page-surface" title={t("env.localization.editorTitle")}>
        {hubQuery.isPending ? (
          <Skeleton active paragraph={{ rows: 4 }} />
        ) : (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Text type="secondary">{t("env.localization.applyPatchHint")}</Text>
            {hub?.editors.map((editor) => (
              <EditorLocalizationCard
                key={editor.id}
                editor={editor}
                remote={upstream?.editor}
                checking={upstreamQuery.isFetching}
                failed={Boolean(upstreamQuery.error)}
                busy={busy}
                installing={installEditorHelper.isPending && installEditorHelper.variables === editor.id}
                updating={updateEditorHelper.isPending && updateEditorHelper.variables === editor.id}
                uninstalling={
                  uninstallEditorHelper.isPending && uninstallEditorHelper.variables === editor.id
                }
                onInstall={() => installEditorHelper.mutate(editor.id)}
                onUpdate={() => updateEditorHelper.mutate(editor.id)}
                onUninstall={() => uninstallEditorHelper.mutate(editor.id)}
              />
            ))}
          </Space>
        )}
      </Card>

      <OnboardingTip
        tipKey="localization_safe_mode"
        message={t("env.localization.safeMode")}
        description={t("env.localization.safeModeDescription")}
      />
      <Card
        size="small"
        className="page-surface"
        title={t("env.localization.title")}
        extra={
          <Button
            size="small"
            icon={<ReloadOutlined spin={statusQuery.isFetching || upstreamQuery.isFetching} />}
            disabled={busy}
            onClick={() => void refreshDesktop()}
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
              <Descriptions.Item label={t("env.localization.hostVersion")}>
                {localization?.claudeVersion ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.installPath")}>
                <PathValue value={localization?.installPath} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.locale")}>
                {localization?.configuredLocale ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.localResourceVersion")}>
                {localization?.packVersion ?? t("env.localization.notInstalledResource")}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.upstreamVersion")}>
                {upstreamDisplay(
                  upstream?.desktop,
                  upstreamQuery.isFetching,
                  Boolean(upstreamQuery.error),
                  t,
                )}
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
              <Descriptions.Item label={t("env.localization.packPath")}>
                <PathValue value={localization?.packPath} />
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
            <ResourceActions>
              <Popconfirm
                title={t("env.localization.confirmDesktopInstall")}
                description={t("env.localization.confirmDesktopInstallDescription")}
                onConfirm={() => installDesktop.mutate()}
              >
                <Button
                  type="primary"
                  loading={installDesktop.isPending}
                  disabled={
                    busy ||
                    !localization?.platformSupported ||
                    !localization.installDetected ||
                    desktopInstalled
                  }
                >
                  {t("env.localization.installChinese")}
                </Button>
              </Popconfirm>
              <Popconfirm
                title={t("env.localization.confirmDesktopUpdate")}
                description={t("env.localization.confirmDesktopUpdateDescription")}
                onConfirm={() => updateDesktop.mutate()}
              >
                <Button
                  loading={updateDesktop.isPending}
                  disabled={
                    busy ||
                    !localization?.platformSupported ||
                    (desktopInstalled && desktopRelation === "same")
                  }
                >
                  {desktopInstalled && desktopRelation === "same"
                    ? t("env.localization.alreadyLatest")
                    : updateButtonLabel(
                        t,
                        localization?.packVersion,
                        upstream?.desktop.version,
                      )}
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
                  {t("env.localization.uninstallChinese")}
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
            </ResourceActions>
          </Space>
        )}
      </Card>
    </Space>
  );
}

function EditorLocalizationCard({
  editor,
  remote,
  checking,
  failed,
  busy,
  installing,
  updating,
  uninstalling,
  onInstall,
  onUpdate,
  onUninstall,
}: {
  editor: EditorLocalizationStatus;
  remote?: LocalizationUpstreamRelease;
  checking: boolean;
  failed: boolean;
  busy: boolean;
  installing: boolean;
  updating: boolean;
  uninstalling: boolean;
  onInstall: () => void;
  onUpdate: () => void;
  onUninstall: () => void;
}) {
  const { t } = useTranslation();
  const relation = compareResourceVersion(editor.helperVersion, remote?.version);
  const canManage = Boolean(editor.editorCliPath);
  const canInstall = Boolean(editor.claudeExtensionPath && editor.editorCliPath);

  return (
    <Card type="inner" size="small" title={editor.label}>
      <Descriptions column={1} size="small">
        <Descriptions.Item label={t("env.localization.status")}>
          <Tag color={editor.helperInstalled ? "green" : "default"}>
            {editor.helperInstalled
              ? t("env.localization.configured")
              : t("env.localization.notConfigured")}
          </Tag>
          <Text type="secondary"> {editor.message}</Text>
        </Descriptions.Item>
        <Descriptions.Item label={t("env.localization.extensionPath")}>
          <PathValue value={editor.claudeExtensionPath} />
        </Descriptions.Item>
        <Descriptions.Item label={t("env.localization.editorCliPath")}>
          <PathValue value={editor.editorCliPath} />
        </Descriptions.Item>
        <Descriptions.Item label={t("env.localization.localResourceVersion")}>
          {editor.helperVersion ?? t("env.localization.notInstalledResource")}
        </Descriptions.Item>
        <Descriptions.Item label={t("env.localization.upstreamVersion")}>
          {upstreamDisplay(remote, checking, failed, t)}
        </Descriptions.Item>
        <Descriptions.Item label={t("env.localization.actions")}>
          <ResourceActions>
            <Popconfirm
              title={t("env.localization.confirmEditorInstall", { editor: editor.label })}
              description={t("env.localization.confirmEditorInstallDescription")}
              onConfirm={onInstall}
            >
              <Button
                type="primary"
                loading={installing}
                disabled={busy || !canInstall || editor.helperInstalled}
              >
                {t("env.localization.installChinese")}
              </Button>
            </Popconfirm>
            <Popconfirm
              title={t("env.localization.confirmEditorUpdate", { editor: editor.label })}
              description={t("env.localization.confirmEditorUpdateDescription")}
              onConfirm={onUpdate}
            >
              <Button
                loading={updating}
                disabled={busy || !canManage || !editor.helperInstalled || relation === "same"}
              >
                {relation === "same"
                  ? t("env.localization.alreadyLatest")
                  : updateButtonLabel(t, editor.helperVersion, remote?.version)}
              </Button>
            </Popconfirm>
            <Popconfirm
              title={t("env.localization.confirmEditorUninstall", { editor: editor.label })}
              description={t("env.localization.confirmEditorUninstallDescription")}
              onConfirm={onUninstall}
            >
              <Button
                danger
                loading={uninstalling}
                disabled={busy || !canManage || !editor.helperInstalled}
              >
                {t("env.localization.uninstallChinese")}
              </Button>
            </Popconfirm>
          </ResourceActions>
        </Descriptions.Item>
      </Descriptions>
    </Card>
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
