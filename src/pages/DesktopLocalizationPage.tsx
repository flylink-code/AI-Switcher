import { type ReactNode } from "react";
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
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  installClaudeCodeLocalization,
  installEditorLocalizationHelper,
  uninstallClaudeCodeLocalization,
  uninstallEditorLocalizationHelper,
  updateClaudeCodeLocalization,
  updateEditorLocalizationHelper,
} from "@/services/api";
import {
  localizationHubOptions,
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
  const hubQuery = useQuery(localizationHubOptions);
  const upstreamQuery = useQuery(localizationUpstreamOptions);
  const hub = hubQuery.data;
  const upstream = upstreamQuery.data;

  const refreshHub = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: localizationHubOptions.queryKey }),
      queryClient.invalidateQueries({ queryKey: localizationUpstreamOptions.queryKey }),
    ]);
  };

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
    installClaudeCode.isPending ||
    updateClaudeCode.isPending ||
    uninstallClaudeCode.isPending ||
    installEditorHelper.isPending ||
    updateEditorHelper.isPending ||
    uninstallEditorHelper.isPending;

  const codeResourceInstalled = Boolean(
    hub?.claudeCode.pluginEnabled || hub?.claudeCode.pluginVersion,
  );
  const codeRelation = compareResourceVersion(
    hub?.claudeCode.pluginVersion,
    upstream?.claudeCode.version,
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
                  title={
                    codeResourceInstalled
                      ? t("env.localization.confirmCodeUpdate")
                      : t("env.localization.confirmCodeInstall")
                  }
                  description={
                    codeResourceInstalled
                      ? t("env.localization.confirmCodeUpdateDescription")
                      : t("env.localization.confirmCodeInstallDescription")
                  }
                  onConfirm={() => updateClaudeCode.mutate()}
                >
                  <Button
                    loading={updateClaudeCode.isPending}
                    disabled={
                      busy ||
                      !hub?.claudeCode.installed ||
                      (codeResourceInstalled && codeRelation === "same")
                    }
                  >
                    {codeResourceInstalled && codeRelation === "same"
                      ? t("env.localization.alreadyLatest")
                      : !codeResourceInstalled
                        ? t("env.localization.installFromUpstream")
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
              title={
                editor.helperInstalled
                  ? t("env.localization.confirmEditorUpdate", { editor: editor.label })
                  : t("env.localization.confirmEditorInstall", { editor: editor.label })
              }
              description={
                editor.helperInstalled
                  ? t("env.localization.confirmEditorUpdateDescription")
                  : t("env.localization.confirmEditorInstallDescription")
              }
              onConfirm={onUpdate}
            >
              <Button
                loading={updating}
                disabled={
                  busy ||
                  !canManage ||
                  (editor.helperInstalled && relation === "same")
                }
              >
                {editor.helperInstalled && relation === "same"
                  ? t("env.localization.alreadyLatest")
                  : !editor.helperInstalled
                    ? t("env.localization.installFromUpstream")
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
