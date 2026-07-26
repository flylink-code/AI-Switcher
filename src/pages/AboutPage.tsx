import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  Space,
  Spin,
  Tag,
  Typography,
  message,
  Modal,
} from "antd";
import CloudDownloadOutlined from "@ant-design/icons/es/icons/CloudDownloadOutlined";
import CodeOutlined from "@ant-design/icons/es/icons/CodeOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import InfoCircleOutlined from "@ant-design/icons/es/icons/InfoCircleOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import type { ClaudeCodeVersionInfo } from "@/types/backend";
import { getClaudeCodeVersion, runClaudeCodeUpdate } from "@/services/api";

const { Text, Paragraph } = Typography;

export default function AboutPage() {
  const { t } = useTranslation();
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [claudeInfo, setClaudeInfo] = useState<ClaudeCodeVersionInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [checkingApp, setCheckingApp] = useState(false);
  const [checkingClaude, setCheckingClaude] = useState(false);
  const [updatingClaude, setUpdatingClaude] = useState(false);

  const refreshClaude = useCallback(async () => {
    setCheckingClaude(true);
    try {
      setClaudeInfo(await getClaudeCodeVersion());
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setCheckingClaude(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [version] = await Promise.all([
        getVersion().catch(() => null),
        refreshClaude(),
      ]);
      setAppVersion(version);
    } finally {
      setLoading(false);
    }
  }, [refreshClaude]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const checkAppUpdate = async () => {
    setCheckingApp(true);
    try {
      const update = await check();
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
          await update.downloadAndInstall();
        },
      });
    } catch {
      void message.error(t("about.appUpdateFailed"));
    } finally {
      setCheckingApp(false);
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
      await refreshClaude();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setUpdatingClaude(false);
    }
  };

  return (
    <Spin spinning={loading}>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Alert
          type="info"
          showIcon
          icon={<InfoCircleOutlined />}
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
              icon={<ReloadOutlined />}
              loading={checkingClaude}
              onClick={() => void refreshClaude()}
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
    </Spin>
  );
}
