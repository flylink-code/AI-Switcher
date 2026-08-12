import { useEffect, useState } from "react";
import {
  Button,
  Card,
  Descriptions,
  Input,
  Modal,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import CloudDownloadOutlined from "@ant-design/icons/es/icons/CloudDownloadOutlined";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import {
  getUpdateMirrorSettings,
  restartApp,
  restoreOnboardingTips,
  setUpdateMirrorSettings,
} from "@/services/api";
import {
  checkForAppUpdate,
  installAvailableAppUpdate,
  isAppUpdatePackagePendingError,
  isNoAppUpdateAvailableError,
} from "@/lib/appUpdater";
import type { UpdateMirrorSettings } from "@/types/backend";
import { OnboardingTip } from "@/components/OnboardingTip";

const { Text } = Typography;

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** App-only About: version, updater, onboarding tips. CLI tools live under Settings → Runtime Tools. */
export default function AboutPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [checkingApp, setCheckingApp] = useState(false);
  const [restoringTips, setRestoringTips] = useState(false);
  const [updateMirrorSettings, setUpdateMirrorSettingsState] = useState<UpdateMirrorSettings | null>(null);
  const [savingUpdateMirrorSettings, setSavingUpdateMirrorSettings] = useState(false);

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
      const raw = errMsg(error);
      if (isNoAppUpdateAvailableError(raw)) {
        void message.info(t("about.appUpToDate"));
        return;
      }
      if (isAppUpdatePackagePendingError(raw)) {
        void message.warning(t("about.appUpdatePackagePending"));
        return;
      }
      void message.error(
        t("about.appUpdateFailedDetail", { error: raw }),
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

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
      <OnboardingTip
        tipKey="about"
        message={t("about.title")}
        description={t("about.appOnlyDescription", {
          defaultValue: "查看本应用版本、检查更新，以及恢复引导提示。Agent 工具请到设置 → Agent 工具。",
        })}
      />

      <Card size="small" className="page-surface" title={t("about.appSection")}>
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
                  onChange={(useMirror) =>
                    setUpdateMirrorSettingsState((current) =>
                      current ? { ...current, useMirror } : current,
                    )
                  }
                />
                <Text>{t("about.useUpdateMirror")}</Text>
              </Space>
              <Input
                disabled={!updateMirrorSettings?.useMirror}
                value={updateMirrorSettings?.mirrorBase ?? ""}
                placeholder="https://gh-proxy.com/"
                onChange={(event) =>
                  setUpdateMirrorSettingsState((current) =>
                    current ? { ...current, mirrorBase: event.target.value } : current,
                  )
                }
              />
              <Text type="secondary">{t("about.updateMirrorHint")}</Text>
              <Text code copyable={Boolean(updateMirrorSettings?.useMirror)} style={{ wordBreak: "break-all" }}>
                {updateMirrorSettings?.useMirror
                  ? `${updateMirrorSettings.mirrorBase.replace(/\/$/, "")}/https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest-mirror.json`
                  : "https://github.com/flylink-code/AI-Switcher/releases/latest/download/latest.json"}
              </Text>
              <Button
                loading={savingUpdateMirrorSettings}
                disabled={!updateMirrorSettings}
                onClick={() => void saveUpdateSettings()}
              >
                {t("about.saveUpdateMirror")}
              </Button>
            </Space>
          </Card>
          <Button loading={restoringTips} onClick={() => void restoreTips()}>
            {t("about.restoreOnboardingTips")}
          </Button>
        </Space>
      </Card>
    </Space>
  );
}
