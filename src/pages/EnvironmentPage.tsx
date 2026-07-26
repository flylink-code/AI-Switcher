import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  List,
  Modal,
  Popconfirm,
  Space,
  Spin,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import DatabaseOutlined from "@ant-design/icons/es/icons/DatabaseOutlined";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import { useTranslation } from "react-i18next";
import type {
  PathsInfo,
  DbInfo,
  ConfigBackup,
  DesktopLocalizationStatus,
  ProviderTarget,
} from "@/types/backend";
import {
  backupNow,
  getAutostartEnabled,
  getDbInfo,
  getDesktopLocalizationStatus,
  getPaths,
  installDesktopLocalization,
  listConfigBackups,
  ping,
  previewConfigBackup,
  restoreConfigBackup,
  restoreDesktopLocalization,
  selectDesktopLocalizationPack,
  validateDesktopLocalizationPack,
} from "@/services/api";

const { Text } = Typography;

interface PathRow {
  key: string;
  value: string | null;
}

interface EnvironmentPageProps {
  focusLocalization?: boolean;
}

function PathValue({ value }: { value: string | null }) {
  const { t } = useTranslation();
  if (value === null || value === "") {
    return <Tag>{t("env.notDetected")}</Tag>;
  }
  return <Text copyable code style={{ wordBreak: "break-all" }}>{value}</Text>;
}

export default function EnvironmentPage({
  focusLocalization = false,
}: EnvironmentPageProps) {
  const { t } = useTranslation();
  const [paths, setPaths] = useState<PathsInfo | null>(null);
  const [db, setDb] = useState<DbInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pingResult, setPingResult] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [autostartLoading, setAutostartLoading] = useState(true);
  const [backupTarget, setBackupTarget] = useState<ProviderTarget>("claude_code");
  const [configBackups, setConfigBackups] = useState<ConfigBackup[]>([]);
  const [backupPreview, setBackupPreview] = useState<string | null>(null);
  const [localization, setLocalization] = useState<DesktopLocalizationStatus | null>(null);
  const [localizationBusy, setLocalizationBusy] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [p, d] = await Promise.all([getPaths(), getDbInfo()]);
      setPaths(p);
      setDb(d);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshLocalization = useCallback(async () => {
    try {
      setLocalization(await getDesktopLocalizationStatus());
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refresh();
    void refreshLocalization();
    void (async () => {
      try {
        setAutostartEnabled(await getAutostartEnabled());
      } catch (e) {
        void message.error(e instanceof Error ? e.message : String(e));
      } finally {
        setAutostartLoading(false);
      }
    })();
  }, [refresh, refreshLocalization]);

  useEffect(() => {
    if (!focusLocalization) return;
    const frame = window.requestAnimationFrame(() => {
      document
        .getElementById("desktop-localization-card")
        ?.scrollIntoView({ block: "start" });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusLocalization]);

  const selectLocalizationPack = useCallback(async () => {
    setLocalizationBusy(true);
    try {
      const path = await selectDesktopLocalizationPack();
      if (!path) return;
      const result = await validateDesktopLocalizationPack(path);
      void message.success(result.message);
      await refreshLocalization();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setLocalizationBusy(false);
    }
  }, [refreshLocalization]);

  const installLocalization = useCallback(async () => {
    if (!localization?.packPath) return;
    setLocalizationBusy(true);
    try {
      const result = await installDesktopLocalization(localization.packPath);
      void message.success(result.message);
      await refreshLocalization();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
      await refreshLocalization();
    } finally {
      setLocalizationBusy(false);
    }
  }, [localization?.packPath, refreshLocalization]);

  const restoreLocalization = useCallback(async () => {
    setLocalizationBusy(true);
    try {
      const result = await restoreDesktopLocalization();
      void message.success(result.message);
      await refreshLocalization();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
      await refreshLocalization();
    } finally {
      setLocalizationBusy(false);
    }
  }, [refreshLocalization]);

  const onPing = useCallback(async () => {
    setRunning(true);
    try {
      const res = await ping();
      setPingResult(res);
      void message.success(`ping → ${res}`);
    } catch (e) {
      setPingResult(null);
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }, []);

  const onBackup = useCallback(async () => {
    setRunning(true);
    try {
      const res = await backupNow();
      void message.success(`${t("env.backupDone")}: ${res}`);
      void refresh();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }, [refresh, t]);

  const onAutostartChange = useCallback(async (enabled: boolean) => {
    setAutostartLoading(true);
    try {
      await setAutostartEnabled(enabled);
      setAutostartEnabled(enabled);
      void message.success(t("env.autostartUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setAutostartLoading(false);
    }
  }, [t]);

  const loadConfigBackups = useCallback(async (target = backupTarget) => {
    setRunning(true);
    try { setConfigBackups(await listConfigBackups(target)); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [backupTarget]);

  const previewBackup = useCallback(async (backup: ConfigBackup) => {
    try { setBackupPreview(await previewConfigBackup(backupTarget, backup.name)); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
  }, [backupTarget]);

  const restoreBackup = useCallback(async (backup: ConfigBackup) => {
    setRunning(true);
    try {
      await restoreConfigBackup(backupTarget, backup.name);
      void message.success(t("env.restoreDone"));
      await loadConfigBackups();
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [backupTarget, loadConfigBackups, t]);

  const claudeRows: PathRow[] = paths
    ? [
        { key: "claudeConfigDir", value: paths.claudeConfigDir },
        { key: "claudeSettingsPath", value: paths.claudeSettingsPath },
        { key: "claudeJsonPath", value: paths.claudeJsonPath },
      ]
    : [];

  const desktopRows: PathRow[] = paths
    ? [
        { key: "claudeDesktopBase", value: paths.claudeDesktopBase },
        { key: "claudeDesktopThreepBase", value: paths.claudeDesktopThreepBase },
        { key: "claudeDesktopConfigLibrary", value: paths.claudeDesktopConfigLibrary },
        { key: "claudeDesktopMetaPath", value: paths.claudeDesktopMetaPath },
        { key: "claudeDesktopNormalConfigPath", value: paths.claudeDesktopNormalConfigPath },
        { key: "claudeDesktopThreepConfigPath", value: paths.claudeDesktopThreepConfigPath },
      ]
    : [];

  return (
    <Spin spinning={loading}>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Alert
          type="info"
          showIcon
          message={t("env.title")}
          description={t("env.description")}
        />

        <Space>
          <Button icon={<ApiOutlined />} loading={running} onClick={onPing}>
            {t("env.ping")}
          </Button>
          <Button icon={<SafetyCertificateOutlined />} loading={running} onClick={onBackup}>
            {t("env.runBackup")}
          </Button>
          <Button icon={<ReloadOutlined />} onClick={() => void refresh()}>
            {t("env.refresh")}
          </Button>
          {pingResult && <Tag color="green">ping: {pingResult}</Tag>}
        </Space>

        {error && <Alert type="error" showIcon message={error} />}

        {paths && (
          <Card size="small" title={t("env.sections.home")}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("env.fields.home")}>
                <PathValue value={paths.home} />
              </Descriptions.Item>
            </Descriptions>
          </Card>
        )}

        {paths && (
          <Card size="small" title={t("env.sections.claude")}>
            <Descriptions column={1} size="small" bordered>
              {claudeRows.map((r) => (
                <Descriptions.Item key={r.key} label={t(`env.fields.${r.key}`)}>
                  <PathValue value={r.value} />
                </Descriptions.Item>
              ))}
            </Descriptions>
          </Card>
        )}

        {paths && (
          <Card size="small" title={t("env.sections.claudeDesktop")}>
            <Descriptions column={1} size="small" bordered>
              {desktopRows.map((r) => (
                <Descriptions.Item key={r.key} label={t(`env.fields.${r.key}`)}>
                  <PathValue value={r.value} />
                </Descriptions.Item>
              ))}
            </Descriptions>
          </Card>
        )}

        <Card
          id="desktop-localization-card"
          size="small"
          title={t("env.localization.title")}
          style={{ scrollMarginBlock: 24 }}
          extra={
            <Button
              size="small"
              icon={<ReloadOutlined />}
              disabled={localizationBusy}
              onClick={() => void refreshLocalization()}
            >
              {t("common.refresh")}
            </Button>
          }
        >
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Alert
              type="info"
              showIcon
              message={t("env.localization.safeMode")}
              description={t("env.localization.safeModeDescription")}
            />
            {localization?.multipleInstalls && (
              <Alert
                type="warning"
                showIcon
                message={t("env.localization.multipleInstalls")}
              />
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
              <Descriptions.Item label={t("env.localization.version")}>
                {localization?.claudeVersion ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.installPath")}>
                <PathValue value={localization?.installPath ?? null} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.locale")}>
                {localization?.configuredLocale ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("env.localization.packPath")}>
                <PathValue value={localization?.packPath ?? null} />
              </Descriptions.Item>
            </Descriptions>
            <Space wrap>
              <Button
                icon={<FolderOpenOutlined />}
                loading={localizationBusy}
                onClick={() => void selectLocalizationPack()}
              >
                {t("env.localization.selectPack")}
              </Button>
              <Popconfirm
                title={t("env.localization.confirmInstall")}
                description={t("env.localization.confirmInstallDescription")}
                onConfirm={() => void installLocalization()}
              >
                <Button
                  type="primary"
                  loading={localizationBusy}
                  disabled={
                    !localization?.platformSupported ||
                    !localization.installDetected ||
                    !localization.packValid
                  }
                >
                  {t("env.localization.install")}
                </Button>
              </Popconfirm>
              <Popconfirm
                title={t("env.localization.confirmRestore")}
                onConfirm={() => void restoreLocalization()}
              >
                <Button
                  danger
                  loading={localizationBusy}
                  disabled={!localization?.backupAvailable}
                >
                  {t("env.localization.restore")}
                </Button>
              </Popconfirm>
            </Space>
          </Space>
        </Card>

        <Card size="small" title={t("env.sections.system")}>
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label={t("env.fields.autostart")}>
              <Switch checked={autostartEnabled} loading={autostartLoading} onChange={(enabled) => void onAutostartChange(enabled)} />
            </Descriptions.Item>
          </Descriptions>
        </Card>

        {paths && (
          <Card size="small" title={t("env.sections.app")}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("env.fields.appConfigDir")}>
                <PathValue value={paths.appConfigDir} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.fields.appDbPath")}>
                <PathValue value={paths.appDbPath} />
              </Descriptions.Item>
              <Descriptions.Item label={t("env.fields.backupDir")}>
                <PathValue value={paths.backupDir} />
              </Descriptions.Item>
            </Descriptions>
          </Card>
        )}

        <Card size="small" title={t("env.sections.recovery")} extra={<Space>
          <Button size="small" onClick={() => { setBackupTarget("claude_code"); void loadConfigBackups("claude_code"); }}>{t("providers.claudeCode")}</Button>
          <Button size="small" onClick={() => { setBackupTarget("claude_desktop"); void loadConfigBackups("claude_desktop"); }}>{t("providers.claudeDesktop")}</Button>
        </Space>}>
          <List
            size="small"
            dataSource={configBackups}
            locale={{ emptyText: t("env.noConfigBackups") }}
            renderItem={(backup) => <List.Item actions={[
              <Button key="preview" size="small" onClick={() => void previewBackup(backup)}>{t("env.previewBackup")}</Button>,
              <Popconfirm key="restore" title={t("env.confirmRestore")} onConfirm={() => void restoreBackup(backup)}><Button size="small" danger>{t("env.restoreBackup")}</Button></Popconfirm>,
            ]}>{backup.name}</List.Item>}
          />
        </Card>

        {db && (
          <Card size="small" title={<><DatabaseOutlined /> {t("env.sections.db")}</>}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label={t("env.fields.schemaVersion")}>
                <Tag color="blue">v{db.schemaVersion}</Tag>
              </Descriptions.Item>
              <Descriptions.Item label={t("env.fields.providerCount")}>
                {db.providerCount}
              </Descriptions.Item>
            </Descriptions>
          </Card>
        )}
      </Space>
      <Modal open={backupPreview !== null} footer={null} onCancel={() => setBackupPreview(null)} title={t("env.previewBackup")} width={720}>
        <pre style={{ maxHeight: 480, overflow: "auto", whiteSpace: "pre-wrap" }}>{backupPreview}</pre>
      </Modal>
    </Spin>
  );
}
