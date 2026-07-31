import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Space,
  Tag,
  Typography,
  message,
  Modal,
  Input,
  Switch,
} from "antd";
import CloudDownloadOutlined from "@ant-design/icons/es/icons/CloudDownloadOutlined";
import CodeOutlined from "@ant-design/icons/es/icons/CodeOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import {
  getUpdateMirrorSettings,
  restartApp,
  restoreOnboardingTips,
  runClaudeCodeUpdate,
  runCodexCliUpdate,
  setUpdateMirrorSettings,
} from "@/services/api";
import {
  claudeVersionOptions,
  codexCliVersionOptions,
  localClaudeVersionOptions,
  localCodexCliVersionOptions,
} from "@/lib/appQueries";
import { checkForAppUpdate, installAvailableAppUpdate } from "@/lib/appUpdater";
import type { ClaudeCodeVersionInfo, CodexCliVersionInfo, UpdateMirrorSettings } from "@/types/backend";
import { OnboardingTip } from "@/components/OnboardingTip";

const { Text, Paragraph } = Typography;

export default function AboutPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [checkingApp, setCheckingApp] = useState(false);
  const [updatingClaude, setUpdatingClaude] = useState(false);
  const [updatingCodex, setUpdatingCodex] = useState(false);
  const [restoringTips, setRestoringTips] = useState(false);
  const [updateMirrorSettings, setUpdateMirrorSettingsState] = useState<UpdateMirrorSettings | null>(null);
  const [savingUpdateMirrorSettings, setSavingUpdateMirrorSettings] = useState(false);
  const localClaudeQuery = useQuery(localClaudeVersionOptions);
  const claudeQuery = useQuery({
    ...claudeVersionOptions,
    placeholderData: () => localClaudeQuery.data,
  });
  const claudeInfo = claudeQuery.data ?? localClaudeQuery.data ?? null;
  const localCodexQuery = useQuery(localCodexCliVersionOptions);
  const codexQuery = useQuery({
    ...codexCliVersionOptions,
    placeholderData: () => localCodexQuery.data,
  });
  const codexInfo = codexQuery.data ?? localCodexQuery.data ?? null;

  useEffect(() => {
    void getVersion().then(setAppVersion).catch(() => setAppVersion(null));
  }, []);

  useEffect(() => {
    void getUpdateMirrorSettings().then(setUpdateMirrorSettingsState).catch((error) => {
      console.warn("Failed to load update mirror settings", error);
    });
  }, []);

  const checkAppUpdate = async () => {
    setCheckingApp(true);
    try {
      const update = await checkForAppUpdate(t("about.appUpdateTimedOut"));
      if (!update) {
        void message.info(t("about.appUpToDate"));
        return;
      }
      Modal.confirm({
        title: t("about.appUpdateAvailable", { version: update.version }),
        content: t("about.appUpdatePrompt"),
        okText: t("about.appUpdateInstall"),
        cancelText: t("providers.cancel"),
        onOk: async () => {
          try {
            await installAvailableAppUpdate(update.version);
            await restartApp();
          } catch (error) {
            console.error("Application update installation failed", error);
            void message.error(
              t("about.appUpdateFailedDetail", { error: errMsg(error) }),
            );
            throw error;
          }
        },
      });
    } catch (error) {
      console.error("Application update check failed", error);
      void message.error(
        t("about.appUpdateFailedDetail", { error: errMsg(error) }),
      );
    } finally {
      setCheckingApp(false);
    }
  };

  const restoreTips = async () => {
    setRestoringTips(true);
    try {
      await restoreOnboardingTips();
      queryClient.setQueryData(["dismissed-onboarding-tips"], []);
      void message.success(t("about.onboardingTipsRestored"));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setRestoringTips(false);
    }
  };

  const saveUpdateSettings = async () => {
    if (!updateMirrorSettings) return;
    setSavingUpdateMirrorSettings(true);
    try {
      const saved = await setUpdateMirrorSettings(updateMirrorSettings);
      setUpdateMirrorSettingsState(saved);
      void message.success(t("about.updateMirrorSaved"));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setSavingUpdateMirrorSettings(false);
    }
  };

  const copyCommand = async (command: string) => {
    try {
      await navigator.clipboard.writeText(command);
      void message.success(t("about.commandCopied"));
    } catch {
      void message.error(t("about.commandCopyFailed"));
    }
  };

  const updateClaudeCode = async () => {
    setUpdatingClaude(true);
    try {
      const result = await runClaudeCodeUpdate();
      void message.success(result);
      await queryClient.invalidateQueries({ queryKey: ["claude-code-version"] });
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setUpdatingClaude(false);
    }
  };

  const updateCodexCli = async () => {
    setUpdatingCodex(true);
    try {
      const result = await runCodexCliUpdate();
      void message.success(result);
      await queryClient.invalidateQueries({ queryKey: ["codex-cli-version"] });
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setUpdatingCodex(false);
    }
  };

  return (
    <>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <OnboardingTip
          tipKey="about"
          message={t("about.title")}
          description={t("about.description")}
        />

        <Card size="small" title={t("about.appSection")}>
          <Space direction="vertical" style={{ width: "100%" }}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("about.appName")}>
                {t("app.name")}
              </Descriptions.Item>
              <Descriptions.Item label={t("about.appVersion")}>
                <Tag color="blue">v{appVersion ?? "—"}</Tag>
              </Descriptions.Item>
            </Descriptions>
            <Button
              type="primary"
              icon={<CloudDownloadOutlined />}
              loading={checkingApp}
              onClick={() => void checkAppUpdate()}
            >
              {t("about.checkAppUpdate")}
            </Button>
            <Card size="small" type="inner" title={t("about.updateMirrorTitle")}>
              <Space direction="vertical" style={{ width: "100%" }}>
                <Space wrap>
                  <Switch
                    checked={updateMirrorSettings?.useMirror ?? false}
                    disabled={!updateMirrorSettings}
                    onChange={(useMirror) => setUpdateMirrorSettingsState((current) => current ? { ...current, useMirror } : current)}
                  />
                  <Text>{t("about.useUpdateMirror")}</Text>
                </Space>
                <Input
                  disabled={!updateMirrorSettings?.useMirror}
                  value={updateMirrorSettings?.mirrorBase ?? ""}
                  placeholder="https://gh-proxy.com/"
                  onChange={(event) => setUpdateMirrorSettingsState((current) => current ? { ...current, mirrorBase: event.target.value } : current)}
                />
                <Text type="secondary">{t("about.updateMirrorHint")}</Text>
                <Text code copyable={Boolean(updateMirrorSettings?.useMirror)} style={{ wordBreak: "break-all" }}>
                  {updateMirrorSettings?.useMirror
                    ? `${updateMirrorSettings.mirrorBase.replace(/\/$/, "")}/https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest-mirror.json`
                    : "https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest.json"}
                </Text>
                <Button loading={savingUpdateMirrorSettings} disabled={!updateMirrorSettings} onClick={() => void saveUpdateSettings()}>
                  {t("about.saveUpdateMirror")}
                </Button>
              </Space>
            </Card>
            <Button loading={restoringTips} onClick={() => void restoreTips()}>
              {t("about.restoreOnboardingTips")}
            </Button>
          </Space>
        </Card>

        <CliToolCard
          title={t("about.claudeCodeSection")}
          info={claudeInfo}
          fetching={claudeQuery.isFetching}
          updating={updatingClaude}
          onRefresh={() => void claudeQuery.refetch()}
          onCopy={(command) => void copyCommand(command)}
          onInstallOrUpdate={() => void updateClaudeCode()}
          labels={{
            current: t("about.claudeCurrentVersion"),
            latest: t("about.claudeLatestVersion"),
            status: t("about.claudeStatus"),
            environment: t("about.claudeEnvironment"),
            source: t("about.claudeInstallSource"),
            executable: t("about.claudeExecutablePath"),
            hint: t("about.claudeCommandHint"),
            copy: t("about.copyCommand"),
            install: t("about.runClaudeInstall"),
            update: t("about.runClaudeUpdate"),
            notInstalled: t("about.notInstalled"),
            broken: t("about.installedButBroken"),
            unknown: t("about.unknown"),
            updateAvailable: t("about.updateAvailable"),
            upToDate: t("about.upToDate"),
            refresh: t("common.refresh"),
          }}
        />

        <CliToolCard
          title={t("about.codexCliSection")}
          info={codexInfo}
          fetching={codexQuery.isFetching}
          updating={updatingCodex}
          onRefresh={() => void codexQuery.refetch()}
          onCopy={(command) => void copyCommand(command)}
          onInstallOrUpdate={() => void updateCodexCli()}
          labels={{
            current: t("about.codexCurrentVersion"),
            latest: t("about.codexLatestVersion"),
            status: t("about.codexStatus"),
            environment: t("about.codexEnvironment"),
            source: t("about.codexInstallSource"),
            executable: t("about.codexExecutablePath"),
            hint: t("about.codexCommandHint"),
            copy: t("about.copyCommand"),
            install: t("about.runCodexInstall"),
            update: t("about.runCodexUpdate"),
            notInstalled: t("about.notInstalled"),
            broken: t("about.installedButBroken"),
            unknown: t("about.unknown"),
            updateAvailable: t("about.updateAvailable"),
            upToDate: t("about.upToDate"),
            refresh: t("common.refresh"),
          }}
        />
      </Space>
    </>
  );
}

type CliInfo = ClaudeCodeVersionInfo | CodexCliVersionInfo;

function CliToolCard({
  title,
  info,
  fetching,
  updating,
  onRefresh,
  onCopy,
  onInstallOrUpdate,
  labels,
}: {
  title: string;
  info: CliInfo | null;
  fetching: boolean;
  updating: boolean;
  onRefresh: () => void;
  onCopy: (command: string) => void;
  onInstallOrUpdate: () => void;
  labels: {
    current: string;
    latest: string;
    status: string;
    environment: string;
    source: string;
    executable: string;
    hint: string;
    copy: string;
    install: string;
    update: string;
    notInstalled: string;
    broken: string;
    unknown: string;
    updateAvailable: string;
    upToDate: string;
    refresh: string;
  };
}) {
  const command = info?.installed ? info.updateCommand : info?.installCommand ?? "";
  return (
    <Card
      size="small"
      title={
        <Space>
          <CodeOutlined />
          {title}
        </Space>
      }
      extra={
        <Button size="small" icon={<ReloadOutlined spin={fetching} />} onClick={onRefresh}>
          {labels.refresh}
        </Button>
      }
    >
      <Space direction="vertical" style={{ width: "100%" }}>
        <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label={labels.current}>
            {info?.installedButBroken ? (
              <Tag color="red">{labels.broken}</Tag>
            ) : info?.installed ? (
              <Text code>{info.currentVersion ?? labels.unknown}</Text>
            ) : (
              <Tag>{labels.notInstalled}</Tag>
            )}
          </Descriptions.Item>
          <Descriptions.Item label={labels.latest}>
            <Text code>{info?.latestVersion ?? labels.unknown}</Text>
          </Descriptions.Item>
          <Descriptions.Item label={labels.status}>
            {info?.updateAvailable ? (
              <Tag color="orange">{labels.updateAvailable}</Tag>
            ) : info?.installedButBroken ? (
              <Tag color="red">{labels.broken}</Tag>
            ) : info?.installed ? (
              <Tag color="green">{labels.upToDate}</Tag>
            ) : (
              <Tag>{labels.notInstalled}</Tag>
            )}
          </Descriptions.Item>
          <Descriptions.Item label={labels.environment}>
            <Space size="small">
              <Tag>{info?.environment ?? "—"}</Tag>
              {"wslDistro" in (info ?? {}) && (info as ClaudeCodeVersionInfo).wslDistro && (
                <Text code>{(info as ClaudeCodeVersionInfo).wslDistro}</Text>
              )}
            </Space>
          </Descriptions.Item>
          <Descriptions.Item label={labels.source}>
            <Text>{info?.source ?? "—"}</Text>
          </Descriptions.Item>
          <Descriptions.Item label={labels.executable}>
            <Text code copyable={Boolean(info?.executablePath)}>
              {info?.executablePath ?? "—"}
            </Text>
          </Descriptions.Item>
        </Descriptions>

        {info?.error && <Alert type="warning" showIcon message={info.error} />}

        <Paragraph type="secondary" style={{ marginBottom: 0 }}>
          {labels.hint}
        </Paragraph>
        <Space wrap>
          <Text code copyable>
            {command}
          </Text>
          <Button size="small" icon={<CopyOutlined />} onClick={() => onCopy(command)}>
            {labels.copy}
          </Button>
          <Button size="small" type="primary" loading={updating} onClick={onInstallOrUpdate}>
            {info?.installed ? labels.update : labels.install}
          </Button>
        </Space>
      </Space>
    </Card>
  );
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
