import { useCallback, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Descriptions,
  Input,
  List,
  Modal,
  Popconfirm,
  Select,
  Skeleton,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import DatabaseOutlined from "@ant-design/icons/es/icons/DatabaseOutlined";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { OnboardingTip } from "@/components/OnboardingTip";
import type {
  AutostartMode,
  CloseBehavior,
  ConfigBackup,
  DoctorReport,
  VisibilityRepairResult,
  CodexWebSearchMode,
  CodexWebSearchSnapshot,
  LibraryArchivePreview,
  ProviderTarget,
  SyncPreview,
  SyncTarget,
  SyncTargetKind,
} from "@/types/backend";
import {
  backupNow,
  exportLibraryBackup,
  findLatestLibraryArchive,
  previewLibraryBackup,
  restoreLibraryBackup,
  runEnvironmentDoctor,
  repairDoctorCheck,
  repairEnvironmentVisibility,
  getCodexWebSearchMode,
  setCodexWebSearchMode,
  deleteSyncTarget,
  discoverWslDistributions,
  migrateDataRoot,
  listConfigBackups,
  ping,
  previewConfigBackup,
  restoreConfigBackup,
  listSyncTargets,
  previewSync,
  pushSyncArchive,
  saveSyncTarget,
  setAutostartConfig,
  setCloseBehavior,
  restartApp,
} from "@/services/api";
import {
  defaultRemoteRootForUser,
  joinSshEndpoint,
  nextRemoteRootForUser,
  splitSshEndpoint,
} from "@/utils/syncRemoteRoot";
import {
  autostartOptions,
  closeBehaviorOptions,
  environmentOptions,
} from "@/lib/appQueries";

const { Text } = Typography;

interface PathRow {
  key: string;
  value: string | null;
}

/** Prefer opening the dialog already inside a (often hidden) archive folder. */
function preferredArchiveDirectory(paths: { home: string; appConfigDir: string } | null): string | undefined {
  if (!paths) return undefined;
  const home = paths.home.replace(/[\\/]+$/, "");
  // GTK/Linux often hides dotfolders when browsing the parent; defaultPath opens inside them.
  return `${home}/.ai-switcher/incoming`;
}

function PathValue({ value }: { value: string | null }) {
  const { t } = useTranslation();
  if (value === null || value === "") {
    return <Tag>{t("env.notDetected")}</Tag>;
  }
  return <Text copyable code style={{ wordBreak: "break-all" }}>{value}</Text>;
}

export default function EnvironmentPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const environmentQuery = useQuery(environmentOptions);
  const autostartQuery = useQuery(autostartOptions);
  const closeBehaviorQuery = useQuery(closeBehaviorOptions);
  const syncTargetsQuery = useQuery({ queryKey: ["sync-targets"], queryFn: listSyncTargets });
  const paths = environmentQuery.data?.paths ?? null;
  const db = environmentQuery.data?.db ?? null;
  const dataRoot = environmentQuery.data?.dataRoot ?? null;
  const error = environmentQuery.error
    ? environmentQuery.error instanceof Error
      ? environmentQuery.error.message
      : String(environmentQuery.error)
    : null;
  const [pingResult, setPingResult] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [doctorRunning, setDoctorRunning] = useState(false);
  const [doctorReport, setDoctorReport] = useState<DoctorReport | null>(null);
  const [doctorRepairing, setDoctorRepairing] = useState<string | null>(null);
  const [visibilityRepairing, setVisibilityRepairing] = useState(false);
  const [webSearch, setWebSearch] = useState<CodexWebSearchSnapshot | null>(null);
  const [webSearchSaving, setWebSearchSaving] = useState(false);
  const [autostartChanging, setAutostartChanging] = useState(false);
  const [closeBehaviorChanging, setCloseBehaviorChanging] = useState(false);
  const [backupTarget, setBackupTarget] = useState<ProviderTarget>("claude_code");
  const [configBackups, setConfigBackups] = useState<ConfigBackup[]>([]);
  const [configBackupDirectory, setConfigBackupDirectory] = useState<string | null>(null);
  const [backupPreview, setBackupPreview] = useState<string | null>(null);
  const [dataRootPath, setDataRootPath] = useState("");
  const [libraryArchivePath, setLibraryArchivePath] = useState("");
  const [libraryArchivePreview, setLibraryArchivePreview] = useState<LibraryArchivePreview | null>(null);
  const [migratingDataRoot, setMigratingDataRoot] = useState(false);
  const [syncModalOpen, setSyncModalOpen] = useState(false);
  const [syncKind, setSyncKind] = useState<SyncTargetKind>("wsl");
  const [syncName, setSyncName] = useState("");
  const [syncDistribution, setSyncDistribution] = useState<string | undefined>();
  const [syncHost, setSyncHost] = useState("");
  const [syncUser, setSyncUser] = useState("");
  const [syncRoot, setSyncRoot] = useState(defaultRemoteRootForUser("user"));
  const [wslDistributions, setWslDistributions] = useState<string[]>([]);
  const [syncPreview, setSyncPreview] = useState<SyncPreview | null>(null);
  const [syncIncludeApiKeys, setSyncIncludeApiKeys] = useState(false);
  const [pathsModalOpen, setPathsModalOpen] = useState(false);
  const [syncPassword, setSyncPassword] = useState("");
  const [syncPasswordOpen, setSyncPasswordOpen] = useState(false);

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

  const onRunDoctor = useCallback(async () => {
    setDoctorRunning(true);
    try {
      setDoctorReport(await runEnvironmentDoctor());
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setDoctorRunning(false);
    }
  }, []);

  const onRepairVisibility = useCallback(async () => {
    setVisibilityRepairing(true);
    try {
      const result: VisibilityRepairResult = await repairEnvironmentVisibility();
      void message.success(result.message || t("env.visibilityRepairDone"));
      setDoctorReport(await runEnvironmentDoctor());
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setVisibilityRepairing(false);
    }
  }, [t]);

  const onRepairDoctorCheck = useCallback(async (id: string) => {
    setDoctorRepairing(id);
    try {
      const result = await repairDoctorCheck(id);
      void message.success(result.message);
      setDoctorReport(await runEnvironmentDoctor());
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setDoctorRepairing(null);
    }
  }, []);

  const loadWebSearch = useCallback(async () => {
    try {
      setWebSearch(await getCodexWebSearchMode());
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const onChangeWebSearch = useCallback(async (mode: CodexWebSearchMode) => {
    setWebSearchSaving(true);
    try {
      setWebSearch(await setCodexWebSearchMode(mode));
      void message.success(t("env.webSearchSaved"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setWebSearchSaving(false);
    }
  }, [t]);

  const onBackup = useCallback(async () => {
    setRunning(true);
    try {
      const res = await backupNow();
      void message.success(`${t("env.backupDone")}: ${res}`);
      await environmentQuery.refetch();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }, [environmentQuery, t]);

  const onExportLibrary = useCallback(async () => {
    setRunning(true);
    try {
      const home = paths?.home ?? undefined;
      const preferred =
        paths?.appConfigDir
        ?? (home ? `${home}/.claude-switcher` : undefined);
      const destinationDir = await open({
        directory: true,
        multiple: false,
        title: t("env.selectExportDirectory"),
        // Open inside the (often hidden) app data dir so Linux GTK does not hide it.
        defaultPath: preferred,
      });
      if (typeof destinationDir !== "string" || !destinationDir.trim()) {
        return;
      }
      const archive = await exportLibraryBackup(destinationDir);
      void message.success(t("env.libraryBackupDone", { path: archive.archivePath, entries: archive.entries }));
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [paths, t]);

  const pickDataRootDirectory = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("env.selectDataRootDirectory"),
      defaultPath: paths?.home ?? paths?.appConfigDir ?? undefined,
    });
    if (typeof selected === "string") setDataRootPath(selected);
  }, [paths, t]);

  const pickLibraryArchiveFile = useCallback(async () => {
    const selected = await open({
      directory: false,
      multiple: false,
      title: t("env.selectLibraryArchive"),
      filters: [{ name: "ZIP", extensions: ["zip"] }],
      defaultPath: preferredArchiveDirectory(paths),
    });
    if (typeof selected === "string") setLibraryArchivePath(selected);
  }, [paths, t]);

  const pickLibraryArchiveDirectory = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("env.selectLibraryArchiveDirectory"),
      defaultPath: preferredArchiveDirectory(paths),
    });
    if (typeof selected !== "string" || !selected.trim()) return;
    setRunning(true);
    try {
      const archivePath = await findLatestLibraryArchive(selected);
      setLibraryArchivePath(archivePath);
      void message.success(t("env.libraryArchivePicked", { path: archivePath }));
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [paths, t]);

  const useKnownArchiveDirectory = useCallback(async (directory: string) => {
    setRunning(true);
    try {
      const archivePath = await findLatestLibraryArchive(directory);
      setLibraryArchivePath(archivePath);
      void message.success(t("env.libraryArchivePicked", { path: archivePath }));
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [t]);

  const onPreviewLibraryArchive = useCallback(async () => {
    if (!libraryArchivePath.trim()) return;
    setRunning(true);
    try {
      setLibraryArchivePreview(await previewLibraryBackup(libraryArchivePath.trim()));
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [libraryArchivePath]);

  const onImportLibraryArchive = useCallback(async () => {
    if (!libraryArchivePath.trim()) return;
    setRunning(true);
    try {
      const result = await restoreLibraryBackup(libraryArchivePath.trim());
      void message.success(
        result.credentialsImported
          ? t("env.libraryImportedWithKeys")
          : t("env.libraryImported"),
      );
      setLibraryArchivePreview(null);
      if (result.restartRequired) {
        Modal.confirm({
          title: t("env.dataRootRestartTitle"),
          content: result.credentialsImported
            ? t("env.confirmImportLibraryRestartWithKeys")
            : t("env.confirmImportLibraryDescription"),
          okText: t("env.restartNow"),
          cancelText: t("common.cancel"),
          onOk: async () => {
            await restartApp();
          },
        });
      }
      await environmentQuery.refetch();
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [environmentQuery, libraryArchivePath, t]);

  const onAutostartChange = useCallback(async (mode: AutostartMode) => {
    setAutostartChanging(true);
    try {
      await setAutostartConfig(mode);
      const next = await queryClient.fetchQuery(autostartOptions);
      if (mode !== "off" && !next.enabled) {
        void message.error(
          next.taskManagerDisabled
            ? t("env.autostartTaskManagerDisabled")
            : t("env.autostartNotRegistered"),
        );
        return;
      }
      void message.success(t("env.autostartUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
      await queryClient.invalidateQueries({ queryKey: autostartOptions.queryKey });
    } finally {
      setAutostartChanging(false);
    }
  }, [queryClient, t]);

  const onCloseBehaviorChange = useCallback(async (behavior: CloseBehavior) => {
    setCloseBehaviorChanging(true);
    try {
      await setCloseBehavior(behavior);
      queryClient.setQueryData(closeBehaviorOptions.queryKey, behavior);
      void message.success(t("env.closeBehaviorUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setCloseBehaviorChanging(false);
    }
  }, [queryClient, t]);

  const loadConfigBackups = useCallback(async (target = backupTarget, directory = configBackupDirectory) => {
    setRunning(true);
    try { setConfigBackups(await listConfigBackups(target, directory)); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [backupTarget, configBackupDirectory]);

  const pickConfigBackupDirectory = useCallback(async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("env.selectConfigBackupDirectory"),
    });
    if (typeof selected !== "string" || !selected.trim()) return;
    setConfigBackupDirectory(selected);
    await loadConfigBackups(backupTarget, selected);
  }, [backupTarget, loadConfigBackups, t]);

  const useDefaultConfigBackupDirectory = useCallback(async () => {
    setConfigBackupDirectory(null);
    await loadConfigBackups(backupTarget, null);
  }, [backupTarget, loadConfigBackups]);

  const previewBackup = useCallback(async (backup: ConfigBackup) => {
    try { setBackupPreview(await previewConfigBackup(backupTarget, backup.name, configBackupDirectory)); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
  }, [backupTarget, configBackupDirectory]);

  const restoreBackup = useCallback(async (backup: ConfigBackup) => {
    setRunning(true);
    try {
      await restoreConfigBackup(backupTarget, backup.name, configBackupDirectory);
      void message.success(t("env.restoreDone"));
      await loadConfigBackups();
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [backupTarget, configBackupDirectory, loadConfigBackups, t]);

  const migrateLibrary = useCallback(async () => {
    if (!dataRootPath.trim()) return;
    setMigratingDataRoot(true);
    try {
      const result = await migrateDataRoot(dataRootPath);
      void message.success(t("env.dataRootMigrated"));
      if (result.restartRequired) {
        Modal.info({ title: t("env.dataRootRestartTitle"), content: t("env.dataRootRestartDescription") });
      }
      await environmentQuery.refetch();
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setMigratingDataRoot(false);
    }
  }, [dataRootPath, environmentQuery, t]);

  const discoverWsl = useCallback(async () => {
    setRunning(true);
    try {
      const distributions = await discoverWslDistributions();
      setWslDistributions(distributions);
      setSyncDistribution((current) => current ?? distributions[0]);
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, []);

  const saveSync = useCallback(async () => {
    setRunning(true);
    try {
      const sshHost =
        syncKind === "ssh" ? joinSshEndpoint(syncUser, syncHost) || null : null;
      await saveSyncTarget({
        id: "", name: syncName, kind: syncKind,
        wslDistribution: syncKind === "wsl" ? syncDistribution ?? null : null,
        sshHost,
        sshPort: null, remoteRoot: syncRoot, pathMappings: [],
        items: ["provider_presets", "mcp", "prompts", "skills", "session_archives"], lastSyncedAt: null,
      });
      setSyncModalOpen(false); setSyncName(""); setSyncHost(""); setSyncUser("");
      setSyncRoot(defaultRemoteRootForUser("user"));
      await syncTargetsQuery.refetch();
      void message.success(t("env.syncSaved"));
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [syncDistribution, syncHost, syncKind, syncName, syncRoot, syncTargetsQuery, syncUser, t]);

  const openSyncPreview = useCallback(async (target: SyncTarget) => {
    setRunning(true);
    try {
      setSyncIncludeApiKeys(false);
      setSyncPreview(await previewSync(target.id));
    }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, []);

  const removeSync = useCallback(async (target: SyncTarget) => {
    setRunning(true);
    try { await deleteSyncTarget(target.id); await syncTargetsQuery.refetch(); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [syncTargetsQuery]);

  const pushSync = useCallback(async (password: string | null) => {
    if (!syncPreview) return;
    setRunning(true);
    try {
      const result = await pushSyncArchive(syncPreview.target.id, password, syncIncludeApiKeys);
      void message.success(t("env.syncPushed", { path: result.remotePath }));
      setSyncPreview(null);
      setSyncPassword("");
      setSyncPasswordOpen(false);
      await syncTargetsQuery.refetch();
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [syncIncludeApiKeys, syncPreview, syncTargetsQuery, t]);

  const requestPushSync = useCallback(() => {
    if (!syncPreview) return;
    if (syncPreview.target.kind === "ssh") {
      setSyncPassword("");
      setSyncPasswordOpen(true);
      return;
    }
    void pushSync(null);
  }, [pushSync, syncPreview]);

  const onSyncUserChange = useCallback((value: string) => {
    setSyncUser((previous) => {
      setSyncRoot((root) => nextRemoteRootForUser(root, previous, value));
      return value;
    });
  }, []);

  const onSyncHostChange = useCallback((value: string) => {
    if (value.includes("@")) {
      const parsed = splitSshEndpoint(value);
      setSyncHost(parsed.host);
      if (parsed.user) onSyncUserChange(parsed.user);
      return;
    }
    setSyncHost(value);
  }, [onSyncUserChange]);

  const claudeRows: PathRow[] = paths
    ? [
        { key: "claudeConfigDir", value: paths.claudeConfigDir },
        { key: "claudeSettingsPath", value: paths.claudeSettingsPath },
        { key: "claudeJsonPath", value: paths.claudeJsonPath },
        { key: "claudeAgentsPath", value: paths.claudeAgentsPath },
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

  const codexRows: PathRow[] = paths
    ? [
        { key: "codexConfigDir", value: paths.codexConfigDir },
        { key: "codexConfigPath", value: paths.codexConfigPath },
        { key: "codexAuthPath", value: paths.codexAuthPath },
        { key: "codexSkillsDir", value: paths.codexSkillsDir },
        { key: "codexPluginsCacheDir", value: paths.codexPluginsCacheDir },
        { key: "codexSessionsDir", value: paths.codexSessionsDir },
        { key: "codexAgentsPath", value: paths.codexAgentsPath },
      ]
    : [];

  return (
    <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <OnboardingTip
          tipKey="environment"
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
          <Button loading={running} onClick={() => void onExportLibrary()}>
            {t("env.exportLibrary")}
          </Button>
          <Button
            icon={<FolderOpenOutlined />}
            disabled={!paths}
            onClick={() => setPathsModalOpen(true)}
          >
            {t("env.viewPaths")}
          </Button>
          <Button
            icon={<ReloadOutlined spin={environmentQuery.isFetching} />}
            onClick={() => void environmentQuery.refetch()}
          >
            {t("env.refresh")}
          </Button>
          {pingResult && <Tag color="green">ping: {pingResult}</Tag>}
        </Space>

        {error && <Alert type="error" showIcon message={error} />}
        {environmentQuery.isPending && (
          <Card size="small">
            <Skeleton active paragraph={{ rows: 6 }} />
          </Card>
        )}

        <Card
          size="small"
          title={t("env.doctorTitle")}
          extra={
            <Space size={8}>
              <Button size="small" loading={visibilityRepairing} onClick={() => void onRepairVisibility()}>
                {t("env.visibilityRepair")}
              </Button>
              <Button size="small" loading={doctorRunning} onClick={() => void onRunDoctor()}>
                {doctorRunning ? t("env.doctorRunning") : t("env.doctorRun")}
              </Button>
            </Space>
          }
        >
          <Text type="secondary">{t("env.doctorDescription")}</Text>
          {doctorReport ? (
            <List
              size="small"
              style={{ marginTop: 8 }}
              dataSource={doctorReport.checks}
              renderItem={(check) => (
                <List.Item
                  extra={
                    <Space size={8}>
                      {!check.ok && check.repairAction ? (
                        <Button
                          size="small"
                          loading={doctorRepairing === check.repairAction}
                          onClick={() => void onRepairDoctorCheck(check.repairAction!)}
                        >
                          {t("env.doctorRepair")}
                        </Button>
                      ) : null}
                      <Tag color={check.ok ? "green" : "red"}>
                        {t(check.ok ? "env.doctorPassed" : "env.doctorFailed")}
                      </Tag>
                    </Space>
                  }
                >
                  <List.Item.Meta
                    title={check.label}
                    description={<Text type="secondary" style={{ whiteSpace: "pre-wrap" }}>{check.detail}</Text>}
                  />
                </List.Item>
              )}
            />
          ) : (
            <Text type="secondary" style={{ display: "block", marginTop: 8 }}>
              {t("env.doctorEmpty")}
            </Text>
          )}
        </Card>

        <Card
          size="small"
          title={t("env.webSearchTitle")}
          extra={
            <Button size="small" onClick={() => void loadWebSearch()}>
              {t("env.refresh")}
            </Button>
          }
        >
          <Text type="secondary">{t("env.webSearchDescription")}</Text>
          <Space style={{ marginTop: 12 }} wrap>
            <Select<CodexWebSearchMode>
              style={{ minWidth: 180 }}
              loading={webSearchSaving}
              placeholder={t("env.webSearchPlaceholder")}
              value={webSearch?.mode}
              onFocus={() => {
                if (!webSearch) void loadWebSearch();
              }}
              onChange={(mode) => void onChangeWebSearch(mode)}
              options={[
                { value: "disabled", label: t("env.webSearchDisabled") },
                { value: "cached", label: t("env.webSearchCached") },
                { value: "indexed", label: t("env.webSearchIndexed") },
                { value: "live", label: t("env.webSearchLive") },
              ]}
            />
            {webSearch ? (
              <Text type="secondary">
                {webSearch.setInConfig
                  ? t("env.webSearchConfigPath", { path: webSearch.configPath })
                  : t("env.webSearchUnset")}
              </Text>
            ) : null}
          </Space>
        </Card>

        <Card size="small" title={t("env.sections.system")}>
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label={t("env.fields.autostart")}>
              <Select<AutostartMode>
                value={autostartQuery.data?.mode ?? "off"}
                loading={autostartQuery.isPending || autostartChanging}
                disabled={autostartChanging}
                style={{ width: 220 }}
                options={[
                  { value: "off", label: t("env.autostartModes.off") },
                  { value: "silent", label: t("env.autostartModes.silent") },
                  { value: "window", label: t("env.autostartModes.window") },
                ]}
                onChange={(mode) => void onAutostartChange(mode)}
              />
            </Descriptions.Item>
            <Descriptions.Item label={t("env.fields.closeBehavior")}>
              <Select<CloseBehavior>
                value={closeBehaviorQuery.data ?? "ask"}
                loading={closeBehaviorQuery.isPending || closeBehaviorChanging}
                disabled={closeBehaviorChanging}
                style={{ width: 220 }}
                options={[
                  { value: "ask", label: t("env.closeBehaviors.ask") },
                  { value: "tray", label: t("env.closeBehaviors.tray") },
                  { value: "quit", label: t("env.closeBehaviors.quit") },
                ]}
                onChange={(behavior) => void onCloseBehaviorChange(behavior)}
              />
            </Descriptions.Item>
            <Descriptions.Item label={t("env.autostartRegistryCommand")}>
              {autostartQuery.data?.command ? (
                <Typography.Text copyable code style={{ whiteSpace: "pre-wrap" }}>
                  {autostartQuery.data.command}
                </Typography.Text>
              ) : autostartQuery.data?.enabled ? (
                <Text type="secondary">{t("env.autostartEnabledNoCommand")}</Text>
              ) : (
                <Text type="secondary">{t("env.autostartNotInRegistry")}</Text>
              )}
            </Descriptions.Item>
          </Descriptions>
          {autostartQuery.data?.taskManagerDisabled ? (
            <Alert
              style={{ marginTop: 12 }}
              type="warning"
              showIcon
              message={t("env.autostartTaskManagerDisabled")}
            />
          ) : null}
        </Card>

        {paths && (
          <Card size="small" title={t("env.sections.app")}>
            <Space direction="vertical" style={{ width: "100%" }}>
              <Text type="secondary">{t("env.dataRootDescription")}</Text>
              <Descriptions column={1} size="small" bordered>
                <Descriptions.Item label={t("env.dataRootActive")}><PathValue value={dataRoot?.activePath ?? paths.appConfigDir} /></Descriptions.Item>
              </Descriptions>
              <Space.Compact style={{ width: "100%" }}>
                <Input value={dataRootPath} onChange={(event) => setDataRootPath(event.target.value)} placeholder={t("env.dataRootPlaceholder")} />
                <Button onClick={() => void pickDataRootDirectory()}>{t("env.chooseDirectory")}</Button>
                <Popconfirm title={t("env.dataRootConfirm")} description={t("env.dataRootConfirmDescription")} onConfirm={() => void migrateLibrary()} disabled={!dataRootPath.trim()}>
                  <Button loading={migratingDataRoot} disabled={!dataRootPath.trim()}>{t("env.dataRootMove")}</Button>
                </Popconfirm>
              </Space.Compact>
            </Space>
          </Card>
        )}

        <Card size="small" title={t("env.sections.recovery")} extra={<Space>
          <Button size="small" onClick={() => { setBackupTarget("claude_code"); void loadConfigBackups("claude_code"); }}>{t("providers.claudeCode")}</Button>
          <Button size="small" onClick={() => { setBackupTarget("claude_desktop"); void loadConfigBackups("claude_desktop"); }}>{t("providers.claudeDesktop")}</Button>
          <Button size="small" onClick={() => { setBackupTarget("codex"); void loadConfigBackups("codex"); }}>Codex</Button>
        </Space>}>
          <Space wrap style={{ marginBottom: 8 }}>
            <Button size="small" onClick={() => void pickConfigBackupDirectory()}>{t("env.chooseBackupDirectory")}</Button>
            {configBackupDirectory && (
              <Button size="small" onClick={() => void useDefaultConfigBackupDirectory()}>{t("env.useDefaultBackupDirectory")}</Button>
            )}
          </Space>
          {configBackupDirectory && (
            <Text type="secondary" style={{ display: "block", marginBottom: 8 }}>
              {t("env.configBackupDirectory", { path: configBackupDirectory })}
            </Text>
          )}
          <List
            size="small"
            dataSource={configBackups}
            locale={{ emptyText: t("env.noConfigBackups") }}
            renderItem={(backup) => <List.Item actions={[
              <Button key="preview" size="small" onClick={() => void previewBackup(backup)}>{t("env.previewBackup")}</Button>,
              <Popconfirm key="restore" title={t("env.confirmRestore")} onConfirm={() => void restoreBackup(backup)}><Button size="small" danger>{t("env.restoreBackup")}</Button></Popconfirm>,
            ]}>{backup.name}</List.Item>}
          />
          <Space direction="vertical" size={8} style={{ width: "100%", marginTop: 12 }}>
            <Space wrap>
              <Button onClick={() => void pickLibraryArchiveFile()}>{t("env.chooseArchiveFile")}</Button>
              <Button onClick={() => void pickLibraryArchiveDirectory()}>{t("env.chooseArchiveDirectory")}</Button>
              {paths?.appConfigDir && (
                <Button onClick={() => void useKnownArchiveDirectory(`${paths.appConfigDir}/backups`)}>
                  {t("env.useBackupsDirectory")}
                </Button>
              )}
              {paths?.home && (
                <Button onClick={() => void useKnownArchiveDirectory(`${paths.home}/.ai-switcher/incoming`)}>
                  {t("env.useSyncIncomingDirectory")}
                </Button>
              )}
            </Space>
            <Space.Compact style={{ width: "100%" }}>
              <Input value={libraryArchivePath} onChange={(event) => setLibraryArchivePath(event.target.value)} placeholder={t("env.libraryArchivePlaceholder")} />
              <Button loading={running} disabled={!libraryArchivePath.trim()} onClick={() => void onPreviewLibraryArchive()}>{t("env.verifyLibraryArchive")}</Button>
              <Popconfirm
                title={t("env.confirmImportLibrary")}
                description={t("env.confirmImportLibraryDescription")}
                disabled={!libraryArchivePath.trim()}
                onConfirm={() => void onImportLibraryArchive()}
              >
                <Button danger loading={running} disabled={!libraryArchivePath.trim()}>{t("env.importLibraryArchive")}</Button>
              </Popconfirm>
            </Space.Compact>
            <Text type="secondary">{t("env.libraryArchiveDescription")}</Text>
          </Space>
        </Card>

        <Card size="small" title={t("env.sections.sync")} extra={<Button size="small" onClick={() => { setSyncModalOpen(true); void discoverWsl(); }}>{t("env.addSyncTarget")}</Button>}>
          <OnboardingTip tipKey="environment_sync" message={t("env.syncDescription")} style={{ marginBottom: 12 }} />
          <List
            size="small"
            loading={syncTargetsQuery.isPending}
            dataSource={syncTargetsQuery.data ?? []}
            locale={{ emptyText: t("env.noSyncTargets") }}
            renderItem={(target) => <List.Item actions={[
              <Button key="preview" size="small" onClick={() => void openSyncPreview(target)}>{t("env.previewSync")}</Button>,
              <Popconfirm key="delete" title={t("env.confirmDeleteSync")} onConfirm={() => void removeSync(target)}><Button danger size="small">{t("usage.delete")}</Button></Popconfirm>,
            ]}><Space><Tag>{target.kind.toUpperCase()}</Tag><Text>{target.name}</Text><Text type="secondary">{target.remoteRoot}</Text></Space></List.Item>}
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
      <Modal open={backupPreview !== null} footer={null} onCancel={() => setBackupPreview(null)} title={t("env.previewBackup")} width={720}>
        <pre style={{ maxHeight: 480, overflow: "auto", whiteSpace: "pre-wrap" }}>{backupPreview}</pre>
      </Modal>
      <Modal
        open={libraryArchivePreview !== null}
        onCancel={() => setLibraryArchivePreview(null)}
        title={t("env.libraryArchiveVerified")}
        footer={
          <Space>
            <Button onClick={() => setLibraryArchivePreview(null)}>{t("common.cancel")}</Button>
            <Popconfirm
              title={t("env.confirmImportLibrary")}
              description={t("env.confirmImportLibraryDescription")}
              onConfirm={() => void onImportLibraryArchive()}
            >
              <Button type="primary" danger loading={running}>{t("env.importLibraryArchive")}</Button>
            </Popconfirm>
          </Space>
        }
      >
        {libraryArchivePreview && <Descriptions column={1} size="small" bordered>
          <Descriptions.Item label={t("env.libraryArchivePath")}><PathValue value={libraryArchivePreview.archivePath} /></Descriptions.Item>
          <Descriptions.Item label={t("env.libraryArchiveEntries")}>{libraryArchivePreview.entries}</Descriptions.Item>
          <Descriptions.Item label={t("env.libraryArchiveBytes")}>{libraryArchivePreview.totalBytes.toLocaleString()}</Descriptions.Item>
          <Descriptions.Item label={t("env.libraryArchiveSchema")}>v{libraryArchivePreview.schemaVersion}</Descriptions.Item>
        </Descriptions>}
      </Modal>
      <Modal open={syncModalOpen} title={t("env.addSyncTarget")} confirmLoading={running} onOk={() => void saveSync()} onCancel={() => setSyncModalOpen(false)}>
        <Space direction="vertical" style={{ width: "100%" }}>
          <Select value={syncKind} onChange={setSyncKind} options={[{ value: "wsl", label: "WSL" }, { value: "ssh", label: "SSH" }]} />
          <Input value={syncName} onChange={(event) => setSyncName(event.target.value)} placeholder={t("env.syncNamePlaceholder")} />
          {syncKind === "wsl" ? (
            <Select value={syncDistribution} onChange={setSyncDistribution} options={wslDistributions.map((value) => ({ value, label: value }))} placeholder={t("env.syncWslPlaceholder")} />
          ) : (
            <>
              <Input value={syncUser} onChange={(event) => onSyncUserChange(event.target.value)} placeholder={t("env.syncUserPlaceholder")} />
              <Input value={syncHost} onChange={(event) => onSyncHostChange(event.target.value)} placeholder={t("env.syncHostPlaceholder")} />
            </>
          )}
          <Input value={syncRoot} onChange={(event) => setSyncRoot(event.target.value)} placeholder={t("env.syncRootPlaceholder")} />
        </Space>
      </Modal>
      <Modal
        open={syncPreview !== null}
        footer={syncPreview ? (
          <Space>
            <Button onClick={() => setSyncPreview(null)}>{t("common.cancel")}</Button>
            <Popconfirm title={t("env.confirmPushSync")} description={t("env.confirmPushSyncDescription")} onConfirm={() => requestPushSync()}>
              <Button type="primary" loading={running}>{t("env.pushSync")}</Button>
            </Popconfirm>
          </Space>
        ) : null}
        onCancel={() => setSyncPreview(null)}
        title={t("env.previewSync")}
        width={720}
      >
        {syncPreview && <Space direction="vertical" style={{ width: "100%" }}>
          {syncPreview.warnings.map((warning) => <Alert key={warning} type="warning" showIcon message={warning} />)}
          <Checkbox checked={syncIncludeApiKeys} onChange={(event) => setSyncIncludeApiKeys(event.target.checked)}>
            {t("env.syncIncludeApiKeys")}
          </Checkbox>
          {syncIncludeApiKeys && (
            <Alert type="error" showIcon message={t("env.syncIncludeApiKeysWarning")} />
          )}
          <List dataSource={syncPreview.changes} locale={{ emptyText: t("env.noSyncChanges") }} renderItem={(change) => <List.Item><Text>{change.sourcePath} → {change.remotePath}</Text></List.Item>} />
        </Space>}
      </Modal>
      <Modal
        open={pathsModalOpen}
        onCancel={() => setPathsModalOpen(false)}
        footer={<Button onClick={() => setPathsModalOpen(false)}>{t("common.cancel")}</Button>}
        title={t("env.pathsModalTitle")}
        width={760}
      >
        {paths && (
          <Space direction="vertical" size="middle" style={{ width: "100%" }}>
            <Card size="small" title={t("env.sections.home")}>
              <Descriptions column={1} size="small" bordered>
                <Descriptions.Item label={t("env.fields.home")}>
                  <PathValue value={paths.home} />
                </Descriptions.Item>
              </Descriptions>
            </Card>
            <Card size="small" title={t("env.sections.claude")}>
              <Descriptions column={1} size="small" bordered>
                {claudeRows.map((r) => (
                  <Descriptions.Item key={r.key} label={t(`env.fields.${r.key}`)}>
                    <PathValue value={r.value} />
                  </Descriptions.Item>
                ))}
              </Descriptions>
            </Card>
            <Card size="small" title={t("env.sections.claudeDesktop")}>
              <Descriptions column={1} size="small" bordered>
                {desktopRows.map((r) => (
                  <Descriptions.Item key={r.key} label={t(`env.fields.${r.key}`)}>
                    <PathValue value={r.value} />
                  </Descriptions.Item>
                ))}
              </Descriptions>
            </Card>
            <Card size="small" title={t("env.sections.codex")}>
              <Descriptions column={1} size="small" bordered>
                {codexRows.map((r) => (
                  <Descriptions.Item key={r.key} label={t(`env.fields.${r.key}`)}>
                    <PathValue value={r.value} />
                  </Descriptions.Item>
                ))}
              </Descriptions>
            </Card>
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
                <Descriptions.Item label={t("env.dataRootActive")}>
                  <PathValue value={dataRoot?.activePath ?? paths.appConfigDir} />
                </Descriptions.Item>
              </Descriptions>
            </Card>
          </Space>
        )}
      </Modal>
      <Modal
        open={syncPasswordOpen}
        title={t("env.syncPasswordTitle")}
        confirmLoading={running}
        okText={t("env.pushSync")}
        onOk={() => void pushSync(syncPassword.trim() ? syncPassword : null)}
        onCancel={() => { setSyncPasswordOpen(false); setSyncPassword(""); }}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Alert type="info" showIcon message={t("env.syncPasswordHint")} />
          <Input.Password
            value={syncPassword}
            onChange={(event) => setSyncPassword(event.target.value)}
            placeholder={t("env.syncPasswordPlaceholder")}
            autoComplete="new-password"
          />
        </Space>
      </Modal>
    </Space>
  );
}
