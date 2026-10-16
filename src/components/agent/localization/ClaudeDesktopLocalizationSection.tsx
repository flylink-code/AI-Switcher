import { useMemo, useState } from "react";
import { Alert, Button, Descriptions, Popconfirm, Skeleton, Space, Tag, Typography, message } from "antd";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { OnboardingTip } from "@/components/OnboardingTip";
import {
  LocalizationRow,
  PathValue,
  ResourceActions,
  errorMessage,
  updateButtonLabel,
  upstreamDisplay,
} from "@/components/agent/localization/LocalizationShared";
import { localizationOptions, localizationUpstreamOptions } from "@/lib/appQueries";
import { compareResourceVersion } from "@/lib/localizationVersion";
import type { DesktopLocalizationActionResult } from "@/types/backend";
import {
  downloadDesktopLocalizationPack,
  installDesktopLocalization,
  restoreDesktopLocalization,
  selectDesktopLocalizationPack,
  updateDesktopLocalization,
  validateDesktopLocalizationPack,
} from "@/services/api";

const { Text } = Typography;

/**
 * Claude Desktop 卡「高级设置」里的中文化区块（走 desktop_localization IPC，仅 Windows 可写入）。
 * 紧凑行只读本机状态；上游版本仅在展开详情后才请求。
 */
export function ClaudeDesktopLocalizationSection() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  // 展开状态不持久化：高级设置收起时组件卸载即重置。
  const [expanded, setExpanded] = useState(false);
  // 最近一次安装/更新/恢复的结果，仅在详情里展示，不持久化。
  const [lastResult, setLastResult] = useState<DesktopLocalizationActionResult | null>(null);
  const statusQuery = useQuery(localizationOptions);
  const upstreamQuery = useQuery({ ...localizationUpstreamOptions, enabled: expanded });
  const localization = statusQuery.data;
  const upstream = upstreamQuery.data;

  const refreshDesktop = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: localizationOptions.queryKey }),
      queryClient.invalidateQueries({ queryKey: localizationUpstreamOptions.queryKey }),
    ]);
  };

  // 成功提示停 8 秒（含需确认 Language 的说明）；自动重启失败再单独给一条警告。
  const handleActionResult = (result: DesktopLocalizationActionResult) => {
    setLastResult(result);
    void message.success(result.message, 8);
    if (result.restart === "failed") {
      void message.warning(
        `${t("env.localization.restartFailed")}: ${result.restartDetail ?? ""}`,
        10,
      );
    }
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
      // 已有通过校验的资源包就直接用，否则先下载上游 latest。
      let packPath = localization?.packPath;
      if (!localization?.packValid || !packPath) {
        const downloaded = await downloadDesktopLocalizationPack();
        packPath = downloaded.packPath;
      }
      if (!packPath) throw new Error(t("env.localization.selectPack"));
      return installDesktopLocalization(packPath);
    },
    onSuccess: async (result) => {
      handleActionResult(result);
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
      handleActionResult(result);
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
      handleActionResult(result);
      await refreshDesktop();
    },
    onError: async (error) => {
      void message.error(errorMessage(error));
      await refreshDesktop();
    },
  });

  const busy =
    selectPack.isPending ||
    installDesktop.isPending ||
    updateDesktop.isPending ||
    restore.isPending;

  const diagnostics = useMemo(
    () => localization?.diagnostics.filter(Boolean).join("\n") ?? "",
    [localization?.diagnostics],
  );

  const desktopInstalled = localization?.state === "installed";
  const desktopPartial = localization?.state === "partial";
  const partialReasons = localization?.partialReasons ?? [];
  const desktopRelation = compareResourceVersion(
    localization?.packVersion,
    upstream?.desktop.version,
  );

  const statusTag = statusQuery.isPending ? (
    <Skeleton.Button active size="small" style={{ width: 56, minWidth: 56 }} />
  ) : statusQuery.error || !localization ? (
    <Text type="danger">
      {statusQuery.error ? errorMessage(statusQuery.error) : t("env.localization.statusFailed")}
    </Text>
  ) : localization.state === "installed" ? (
    <Tag color="green" style={{ marginInlineEnd: 0 }}>{t("env.localization.configured")}</Tag>
  ) : localization.state === "notInstalled" ? (
    <Tag style={{ marginInlineEnd: 0 }}>{t("env.localization.notConfigured")}</Tag>
  ) : localization.state === "partial" ? (
    <Tag color="orange" style={{ marginInlineEnd: 0 }}>{t("env.localization.states.partial")}</Tag>
  ) : (
    <Tag style={{ marginInlineEnd: 0 }}>{t("env.localization.states.unsupported")}</Tag>
  );

  const canRestore = Boolean(
    desktopInstalled || desktopPartial || localization?.backupAvailable,
  );

  const restoreButton = (size: "small" | "middle") => (
    <Popconfirm
      title={t("env.localization.confirmRestore")}
      description={t("env.localization.confirmRestoreDescription")}
      onConfirm={() => restore.mutate()}
    >
      <Button
        size={size}
        danger
        loading={restore.isPending}
        disabled={busy || !canRestore}
      >
        {t("env.localization.uninstallChinese")}
      </Button>
    </Popconfirm>
  );

  const installButton = (size: "small" | "middle") => (
    <Popconfirm
      title={t("env.localization.confirmDesktopInstall")}
      description={t("env.localization.confirmDesktopInstallDescription")}
      onConfirm={() => installDesktop.mutate()}
    >
      <Button
        size={size}
        type="primary"
        loading={installDesktop.isPending}
        disabled={busy || !localization?.platformSupported || !localization?.installDetected}
      >
        {desktopPartial
          ? t("env.localization.reinstallChinese")
          : t("env.localization.installChinese")}
      </Button>
    </Popconfirm>
  );

  // 主操作：已配置 → 一键卸载（恢复原版）；状态不完整 → 重新安装 + 一键卸载并列；否则 → 一键安装。
  const action = !localization ? null : desktopInstalled ? (
    restoreButton("small")
  ) : desktopPartial ? (
    <Space size={6} wrap>
      {installButton("small")}
      {restoreButton("small")}
    </Space>
  ) : (
    installButton("small")
  );

  // 不可用时不报错，只给一行次要提示。
  const hint = !localization
    ? undefined
    : !localization.platformSupported
      ? localization.message || t("agentHub.localization.unsupportedPlatform")
      : !localization.installDetected
        ? t("agentHub.localization.installHint")
        : undefined;

  return (
    <LocalizationRow
      statusTag={statusTag}
      action={action}
      expanded={expanded}
      onToggle={() => setExpanded((value) => !value)}
      hint={hint}
    >
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        {localization?.multipleInstalls ? (
          <Alert type="warning" showIcon message={t("env.localization.multipleInstalls")} />
        ) : null}
        {desktopPartial && partialReasons.length > 0 ? (
          <Alert
            type="warning"
            showIcon
            message={t("env.localization.partialReasonsTitle")}
            description={
              <Space direction="vertical" size={4}>
                <ul style={{ margin: 0, paddingInlineStart: 18 }}>
                  {partialReasons.map((reason) => (
                    <li key={reason}>{reason}</li>
                  ))}
                </ul>
                <Text type="secondary">{t("env.localization.partialReasonsHint")}</Text>
              </Space>
            }
          />
        ) : null}
        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label={t("env.localization.status")}>
            {localization?.message ?? "—"}
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
          <Descriptions.Item label={t("env.localization.patchedJsCount")}>
            {localization?.patchedJsCount ?? "—"}
          </Descriptions.Item>
          <Descriptions.Item label={t("env.localization.lastInstalledAt")}>
            {localization?.lastInstalledAt
              ? new Date(localization.lastInstalledAt).toLocaleString()
              : "—"}
          </Descriptions.Item>
          <Descriptions.Item label={t("env.localization.logPath")}>
            <PathValue value={lastResult?.logPath ?? localization?.logPath} />
          </Descriptions.Item>
          {lastResult ? (
            <Descriptions.Item label={t("env.localization.lastAction")}>
              {lastResult.message}
            </Descriptions.Item>
          ) : null}
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
        {localization && !localization.installDetected && diagnostics ? (
          <Alert
            type="warning"
            showIcon
            message={localization.message || t("env.localization.statusFailed")}
            description={<pre style={{ margin: 0, whiteSpace: "pre-wrap" }}>{diagnostics}</pre>}
          />
        ) : null}
        <OnboardingTip
          tipKey="localization_safe_mode"
          message={t("env.localization.safeMode")}
          description={t("env.localization.safeModeDescription")}
        />
        <OnboardingTip
          tipKey="localization_third_party"
          type="warning"
          message={t("env.localization.thirdPartyTitle")}
          description={
            <Space direction="vertical" size={0}>
              <Text>{t("env.localization.thirdPartyDescription")}</Text>
              <Button
                type="link"
                size="small"
                style={{ paddingInline: 0 }}
                onClick={() => void openUrl("https://github.com/javaht/claude-desktop-zh-cn")}
              >
                Claude Desktop
              </Button>
            </Space>
          }
        />
        <ResourceActions>
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
                : updateButtonLabel(t, localization?.packVersion, upstream?.desktop.version)}
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
          {localization ? restoreButton("middle") : null}
          <Button
            icon={<ReloadOutlined spin={statusQuery.isFetching || upstreamQuery.isFetching} />}
            disabled={busy}
            onClick={() => void refreshDesktop()}
          >
            {t("common.refresh")}
          </Button>
        </ResourceActions>
      </Space>
    </LocalizationRow>
  );
}
