import { type ReactNode } from "react";
import { Button, Card, Descriptions, Popconfirm, Space, Tag, Typography } from "antd";
import RightOutlined from "@ant-design/icons/es/icons/RightOutlined";
import { useTranslation } from "react-i18next";
import {
  compareResourceVersion,
  normalizeResourceVersion,
} from "@/lib/localizationVersion";
import type {
  EditorLocalizationStatus,
  LocalizationUpstreamRelease,
} from "@/types/backend";

const { Text } = Typography;

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function PathValue({ value }: { value?: string | null }) {
  const { t } = useTranslation();
  if (!value) return <Tag>{t("env.localization.notDetected")}</Tag>;
  return <Text copyable code style={{ wordBreak: "break-all" }}>{value}</Text>;
}

export function ResourceActions({ children }: { children: ReactNode }) {
  return <Space wrap>{children}</Space>;
}

export function updateButtonLabel(
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

export function upstreamDisplay(
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

/**
 * 紧凑行外壳：标题 + 状态 Tag + 主操作 + 「查看详情」开关。
 * 详情（children）默认收起，展开状态由调用方用 useState 持有，不持久化。
 */
export function LocalizationRow({
  statusTag,
  action,
  expanded,
  onToggle,
  hint,
  children,
}: {
  statusTag: ReactNode;
  action: ReactNode;
  expanded: boolean;
  onToggle: () => void;
  hint?: ReactNode;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <div>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 12,
          flexWrap: "wrap",
        }}
      >
        <Space size={8} wrap>
          <span>{t("agentHub.localization.title")}</span>
          {statusTag}
        </Space>
        <Space size={8} wrap>
          {action}
          <Button
            type="text"
            size="small"
            aria-expanded={expanded}
            onClick={onToggle}
            icon={
              <RightOutlined
                style={{
                  fontSize: 10,
                  transform: expanded ? "rotate(90deg)" : undefined,
                  transition: "transform 0.15s ease",
                }}
              />
            }
          >
            {expanded
              ? t("agentHub.localization.hideDetails")
              : t("agentHub.localization.showDetails")}
          </Button>
        </Space>
      </div>
      {hint ? (
        <Text type="secondary" style={{ display: "block", marginTop: 4, fontSize: 12 }}>
          {hint}
        </Text>
      ) : null}
      {expanded ? <div style={{ marginTop: 8 }}>{children}</div> : null}
    </div>
  );
}

export function EditorLocalizationCard({
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
