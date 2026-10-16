import { useState } from "react";
import { Button, Descriptions, Popconfirm, Skeleton, Space, Tag, Typography, message } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { OnboardingTip } from "@/components/OnboardingTip";
import {
  EditorLocalizationCard,
  LocalizationRow,
  PathValue,
  ResourceActions,
  errorMessage,
  updateButtonLabel,
  upstreamDisplay,
} from "@/components/agent/localization/LocalizationShared";
import {
  localizationHubOptions,
  localizationUpstreamOptions,
} from "@/lib/appQueries";
import { compareResourceVersion } from "@/lib/localizationVersion";
import {
  installClaudeCodeLocalization,
  installEditorLocalizationHelper,
  uninstallClaudeCodeLocalization,
  uninstallEditorLocalizationHelper,
  updateClaudeCodeLocalization,
  updateEditorLocalizationHelper,
} from "@/services/api";

const { Text } = Typography;

/**
 * Claude Code 卡「高级设置」里的中文化区块（含 VS Code / Cursor 助手）。
 * 紧凑行只读本机状态；上游版本仅在展开详情后才请求。
 */
export function ClaudeCodeLocalizationSection() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  // 展开状态不持久化：高级设置收起时组件卸载即重置。
  const [expanded, setExpanded] = useState(false);
  const hubQuery = useQuery(localizationHubOptions);
  const upstreamQuery = useQuery({ ...localizationUpstreamOptions, enabled: expanded });
  const hub = hubQuery.data;
  const upstream = upstreamQuery.data;

  const refreshHub = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: localizationHubOptions.queryKey }),
      queryClient.invalidateQueries({ queryKey: localizationUpstreamOptions.queryKey }),
    ]);
  };

  // 六个 mutation 共用成功/失败处理：提示后刷新本机状态。
  const handlers = {
    onSuccess: async (result: string) => {
      void message.success(result);
      await refreshHub();
    },
    onError: (error: unknown) => void message.error(errorMessage(error)),
  };

  const installClaudeCode = useMutation({ mutationFn: installClaudeCodeLocalization, ...handlers });
  const updateClaudeCode = useMutation({ mutationFn: updateClaudeCodeLocalization, ...handlers });
  const uninstallClaudeCode = useMutation({ mutationFn: uninstallClaudeCodeLocalization, ...handlers });
  const installEditorHelper = useMutation({ mutationFn: installEditorLocalizationHelper, ...handlers });
  const updateEditorHelper = useMutation({ mutationFn: updateEditorLocalizationHelper, ...handlers });
  const uninstallEditorHelper = useMutation({ mutationFn: uninstallEditorLocalizationHelper, ...handlers });

  const busy =
    installClaudeCode.isPending ||
    updateClaudeCode.isPending ||
    uninstallClaudeCode.isPending ||
    installEditorHelper.isPending ||
    updateEditorHelper.isPending ||
    uninstallEditorHelper.isPending;

  const codeResourceInstalled = Boolean(
    hub?.claudeCode.pluginEnabled ||
      hub?.claudeCode.pluginVersion ||
      hub?.claudeCode.settingsConfigured,
  );
  const codeRelation = compareResourceVersion(
    hub?.claudeCode.pluginVersion,
    upstream?.claudeCode.version,
  );
  const codeInstalled = Boolean(hub?.claudeCode.installed);

  const statusTag = hubQuery.isPending ? (
    <Skeleton.Button active size="small" style={{ width: 56, minWidth: 56 }} />
  ) : hubQuery.error ? (
    <Text type="danger">{errorMessage(hubQuery.error)}</Text>
  ) : (
    <Tag color={codeResourceInstalled ? "green" : "default"} style={{ marginInlineEnd: 0 }}>
      {codeResourceInstalled
        ? t("env.localization.configured")
        : t("env.localization.notConfigured")}
    </Tag>
  );

  // 主操作：未配置 → 一键安装；已配置 → 一键卸载。均带 Popconfirm。
  const action = !hub ? null : codeResourceInstalled ? (
    <Popconfirm
      title={t("env.localization.confirmCodeUninstall")}
      description={t("env.localization.confirmCodeUninstallDescription")}
      onConfirm={() => uninstallClaudeCode.mutate()}
    >
      <Button
        size="small"
        danger
        loading={uninstallClaudeCode.isPending}
        disabled={busy || !codeInstalled}
      >
        {t("env.localization.uninstallChinese")}
      </Button>
    </Popconfirm>
  ) : (
    <Popconfirm
      title={t("env.localization.confirmCodeInstall")}
      description={t("env.localization.confirmCodeInstallDescription")}
      onConfirm={() => installClaudeCode.mutate()}
    >
      <Button
        size="small"
        type="primary"
        loading={installClaudeCode.isPending}
        disabled={busy || !codeInstalled}
      >
        {t("env.localization.installChinese")}
      </Button>
    </Popconfirm>
  );

  return (
    <LocalizationRow
      statusTag={statusTag}
      action={action}
      expanded={expanded}
      onToggle={() => setExpanded((value) => !value)}
      hint={hub && !codeInstalled ? hub.claudeCode.message : undefined}
    >
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label={t("env.localization.status")}>
            {hub?.claudeCode.message ?? "—"}
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
                    !codeInstalled ||
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
              {codeResourceInstalled ? (
                <Popconfirm
                  title={t("env.localization.confirmCodeUninstall")}
                  description={t("env.localization.confirmCodeUninstallDescription")}
                  onConfirm={() => uninstallClaudeCode.mutate()}
                >
                  <Button
                    danger
                    loading={uninstallClaudeCode.isPending}
                    disabled={busy}
                  >
                    {t("env.localization.uninstallChinese")}
                  </Button>
                </Popconfirm>
              ) : null}
              <Button
                icon={<ReloadOutlined spin={hubQuery.isFetching || upstreamQuery.isFetching} />}
                disabled={busy}
                onClick={() => void refreshHub()}
              >
                {t("common.refresh")}
              </Button>
            </ResourceActions>
          </Descriptions.Item>
        </Descriptions>

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

        <Space direction="vertical" size="small" style={{ width: "100%" }}>
          <Text strong>{t("env.localization.editorTitle")}</Text>
          {hubQuery.isPending ? (
            <Skeleton active paragraph={{ rows: 2 }} />
          ) : (
            <>
              <Space direction="vertical" size={4}>
                <Text type="secondary">{t("env.localization.applyPatchHint")}</Text>
                <Text type="secondary">{t("env.localization.applyPatchFileLockHint")}</Text>
              </Space>
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
            </>
          )}
        </Space>
      </Space>
    </LocalizationRow>
  );
}
