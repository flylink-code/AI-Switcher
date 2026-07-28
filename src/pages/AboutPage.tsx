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
} from "antd";
import CloudDownloadOutlined from "@ant-design/icons/es/icons/CloudDownloadOutlined";
import CodeOutlined from "@ant-design/icons/es/icons/CodeOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { restartApp, restoreOnboardingTips, runClaudeCodeUpdate } from "@/services/api";
import { claudeVersionOptions, localClaudeVersionOptions } from "@/lib/appQueries";
import { checkForAppUpdate } from "@/lib/appUpdater";
import { OnboardingTip } from "@/components/OnboardingTip";

const { Text, Paragraph } = Typography;

export default function AboutPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [checkingApp, setCheckingApp] = useState(false);
  const [updatingClaude, setUpdatingClaude] = useState(false);
  const [restoringTips, setRestoringTips] = useState(false);
  const localClaudeQuery = useQuery(localClaudeVersionOptions);
  const claudeQuery = useQuery({
    ...claudeVersionOptions,
    placeholderData: () => localClaudeQuery.data,
  });
  const claudeInfo = claudeQuery.data ?? localClaudeQuery.data ?? null;

  useEffect(() => {
    void getVersion().then(setAppVersion).catch(() => setAppVersion(null));
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
            await update.downloadAndInstall();
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
            <Button loading={restoringTips} onClick={() => void restoreTips()}>
              {t("about.restoreOnboardingTips")}
            </Button>
          </Space>
        </Card>

        <Card
          size="small"
          title={
            <Space>
              <CodeOutlined />
              {t("about.claudeCodeSection")}
            </Space>
          }
          extra={
            <Button
              size="small"
              icon={<ReloadOutlined spin={claudeQuery.isFetching} />}
              onClick={() => void claudeQuery.refetch()}
            >
              {t("common.refresh")}
            </Button>
          }
        >
          <Space direction="vertical" style={{ width: "100%" }}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("about.claudeCurrentVersion")}>
                {claudeInfo?.installedButBroken ? (
                  <Tag color="red">{t("about.installedButBroken")}</Tag>
                ) : claudeInfo?.installed ? (
                  <Text code>{claudeInfo.currentVersion ?? t("about.unknown")}</Text>
                ) : (
                  <Tag>{t("about.notInstalled")}</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("about.claudeLatestVersion")}>
                <Text code>{claudeInfo?.latestVersion ?? t("about.unknown")}</Text>
              </Descriptions.Item>
              <Descriptions.Item label={t("about.claudeStatus")}>
                {claudeInfo?.updateAvailable ? (
                  <Tag color="orange">{t("about.updateAvailable")}</Tag>
                ) : claudeInfo?.installedButBroken ? (
                  <Tag color="red">{t("about.installedButBroken")}</Tag>
                ) : claudeInfo?.installed ? (
                  <Tag color="green">{t("about.upToDate")}</Tag>
                ) : (
                  <Tag>{t("about.notInstalled")}</Tag>
                )}
              </Descriptions.Item>
              <Descriptions.Item label={t("about.claudeEnvironment")}>
                <Space size="small">
                  <Tag>{claudeInfo?.environment ?? "—"}</Tag>
                  {claudeInfo?.wslDistro && <Text code>{claudeInfo.wslDistro}</Text>}
                </Space>
              </Descriptions.Item>
              <Descriptions.Item label={t("about.claudeInstallSource")}>
                <Text>{claudeInfo?.source ?? "—"}</Text>
              </Descriptions.Item>
              <Descriptions.Item label={t("about.claudeExecutablePath")}>
                <Text code copyable={Boolean(claudeInfo?.executablePath)}>
                  {claudeInfo?.executablePath ?? "—"}
                </Text>
              </Descriptions.Item>
            </Descriptions>

            {claudeInfo?.error && (
              <Alert type="warning" showIcon message={claudeInfo.error} />
            )}

            <Paragraph type="secondary" style={{ marginBottom: 0 }}>
              {t("about.claudeCommandHint")}
            </Paragraph>
            <Space wrap>
              <Text code copyable>
                {claudeInfo?.installed ? claudeInfo.updateCommand : claudeInfo?.installCommand}
              </Text>
              <Button
                size="small"
                icon={<CopyOutlined />}
                onClick={() =>
                  void copyCommand(
                    claudeInfo?.installed
                      ? claudeInfo.updateCommand
                      : claudeInfo?.installCommand ?? "",
                  )
                }
              >
                {t("about.copyCommand")}
              </Button>
              <Button
                size="small"
                type="primary"
                loading={updatingClaude}
                onClick={() => void updateClaudeCode()}
              >
                {claudeInfo?.installed ? t("about.runClaudeUpdate") : t("about.runClaudeInstall")}
              </Button>
            </Space>
          </Space>
        </Card>
      </Space>
    </>
  );
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
