import { useEffect, useMemo, useState } from "react";
import {
  App,
  Button,
  Card,
  Descriptions,
  Input,
  Space,
  Switch,
  Tag,
  Typography,
} from "antd";
import CloudDownloadOutlined from "@ant-design/icons/es/icons/CloudDownloadOutlined";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import {
  getUpdateMirrorSettings,
  restoreOnboardingTips,
  setUpdateMirrorSettings,
} from "@/services/api";
import {
  checkForAppUpdate,
  isAppUpdatePackagePendingError,
  isNoAppUpdateAvailableError,
} from "@/lib/appUpdater";
import { useAppUpdatePrompt } from "@/lib/appUpdateContext";
import type { UpdateMirrorSettings } from "@/types/backend";
import { OnboardingTip } from "@/components/OnboardingTip";

const { Text } = Typography;

function changelogVersionKey(version: string): string {
  return version.replace(/\./g, "_");
}

function changelogNotesForVersion(
  t: (key: string, options: { returnObjects: true }) => unknown,
  version: string | null,
): string[] | null {
  if (!version) return null;
  const notes = t(`about.changelog.${changelogVersionKey(version)}`, { returnObjects: true });
  if (!Array.isArray(notes) || notes.length === 0) return null;
  return notes.every((item) => typeof item === "string") ? notes : null;
}

function errMsg(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** App-only About: version, updater, onboarding tips. CLI tools live under Settings → Runtime Tools. */
export default function AboutPage() {
  const { t, i18n } = useTranslation();
  const { message } = App.useApp();
  const { presentUpdate } = useAppUpdatePrompt();
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
      presentUpdate(update);
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

  const changelogNotes = useMemo(
    () => changelogNotesForVersion(t, appVersion),
    [appVersion, t, i18n.language],
  );

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
        description={t("about.appOnlyDescription")}
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

      <Card
        size="small"
        className="page-surface"
        title={t("about.changelogTitle")}
        extra={appVersion ? <Tag color="blue">v{appVersion}</Tag> : null}
      >
        {appVersion == null ? null : changelogNotes ? (
          <ul style={{ margin: 0, paddingInlineStart: 20 }}>
            {changelogNotes.map((note) => (
              <li key={note} style={{ marginBottom: 8, lineHeight: 1.55 }}>
                <Text>{note}</Text>
              </li>
            ))}
          </ul>
        ) : (
          <Text type="secondary">{t("about.changelogEmpty")}</Text>
        )}
      </Card>
    </Space>
  );
}
