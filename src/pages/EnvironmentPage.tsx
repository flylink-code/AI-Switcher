import { useCallback, useState } from "react";
import {
  Alert,
  Button,
  Card,
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
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SafetyCertificateOutlined from "@ant-design/icons/es/icons/SafetyCertificateOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type {
  AutostartMode,
  CloseBehavior,
  ConfigBackup,
  LibraryArchivePreview,
  ProviderTarget,
  SyncPreview,
  SyncTarget,
  SyncTargetKind,
} from "@/types/backend";
import {
  backupNow,
  exportLibraryBackup,
  previewLibraryBackup,
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
} from "@/services/api";
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
  const [autostartChanging, setAutostartChanging] = useState(false);
  const [closeBehaviorChanging, setCloseBehaviorChanging] = useState(false);
  const [backupTarget, setBackupTarget] = useState<ProviderTarget>("claude_code");
  const [configBackups, setConfigBackups] = useState<ConfigBackup[]>([]);
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
  const [syncRoot, setSyncRoot] = useState("/home/user/.ai-switcher");
  const [wslDistributions, setWslDistributions] = useState<string[]>([]);
  const [syncPreview, setSyncPreview] = useState<SyncPreview | null>(null);

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
      const archive = await exportLibraryBackup();
      void message.success(t("env.libraryBackupDone", { path: archive.archivePath, entries: archive.entries }));
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

  const onAutostartChange = useCallback(async (mode: AutostartMode) => {
    setAutostartChanging(true);
    try {
      await setAutostartConfig(mode);
      queryClient.setQueryData(["environment", "autostart"], {
        enabled: mode !== "off",
        mode,
      });
      void message.success(t("env.autostartUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
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
      await saveSyncTarget({
        id: "", name: syncName, kind: syncKind,
        wslDistribution: syncKind === "wsl" ? syncDistribution ?? null : null,
        sshHost: syncKind === "ssh" ? syncHost : null,
        sshPort: null, remoteRoot: syncRoot, pathMappings: [],
        items: ["provider_presets", "mcp", "prompts", "skills", "session_archives"], lastSyncedAt: null,
      });
      setSyncModalOpen(false); setSyncName(""); setSyncHost("");
      await syncTargetsQuery.refetch();
      void message.success(t("env.syncSaved"));
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [syncDistribution, syncHost, syncKind, syncName, syncRoot, syncTargetsQuery, t]);

  const openSyncPreview = useCallback(async (target: SyncTarget) => {
    setRunning(true);
    try { setSyncPreview(await previewSync(target.id)); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, []);

  const removeSync = useCallback(async (target: SyncTarget) => {
    setRunning(true);
    try { await deleteSyncTarget(target.id); await syncTargetsQuery.refetch(); }
    catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [syncTargetsQuery]);

  const pushSync = useCallback(async () => {
    if (!syncPreview) return;
    setRunning(true);
    try {
      const result = await pushSyncArchive(syncPreview.target.id);
      void message.success(t("env.syncPushed", { path: result.remotePath }));
      setSyncPreview(null);
      await syncTargetsQuery.refetch();
    } catch (e) { void message.error(e instanceof Error ? e.message : String(e)); }
    finally { setRunning(false); }
  }, [syncPreview, syncTargetsQuery, t]);

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
          <Button loading={running} onClick={() => void onExportLibrary()}>
            {t("env.exportLibrary")}
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
            <Space direction="vertical" style={{ width: "100%", marginTop: 16 }}>
              <Text type="secondary">{t("env.dataRootDescription")}</Text>
              <Descriptions column={1} size="small" bordered>
                <Descriptions.Item label={t("env.dataRootActive")}><PathValue value={dataRoot?.activePath ?? paths.appConfigDir} /></Descriptions.Item>
              </Descriptions>
              <Space.Compact style={{ width: "100%" }}>
                <Input value={dataRootPath} onChange={(event) => setDataRootPath(event.target.value)} placeholder={t("env.dataRootPlaceholder")} />
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
          <Space.Compact style={{ width: "100%" }}>
            <Input value={libraryArchivePath} onChange={(event) => setLibraryArchivePath(event.target.value)} placeholder={t("env.libraryArchivePlaceholder")} />
            <Button loading={running} disabled={!libraryArchivePath.trim()} onClick={() => void onPreviewLibraryArchive()}>{t("env.verifyLibraryArchive")}</Button>
          </Space.Compact>
          <Text type="secondary">{t("env.libraryArchiveDescription")}</Text>
        </Card>

        <Card size="small" title={t("env.sections.sync")} extra={<Button size="small" onClick={() => { setSyncModalOpen(true); void discoverWsl(); }}>{t("env.addSyncTarget")}</Button>}>
          <Alert type="info" showIcon message={t("env.syncDescription")} style={{ marginBottom: 12 }} />
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
      <Modal open={libraryArchivePreview !== null} footer={null} onCancel={() => setLibraryArchivePreview(null)} title={t("env.libraryArchiveVerified")}>
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
          {syncKind === "wsl" ? <Select value={syncDistribution} onChange={setSyncDistribution} options={wslDistributions.map((value) => ({ value, label: value }))} placeholder={t("env.syncWslPlaceholder")} /> : <Input value={syncHost} onChange={(event) => setSyncHost(event.target.value)} placeholder={t("env.syncHostPlaceholder")} />}
          <Input value={syncRoot} onChange={(event) => setSyncRoot(event.target.value)} placeholder={t("env.syncRootPlaceholder")} />
        </Space>
      </Modal>
      <Modal open={syncPreview !== null} footer={syncPreview ? <Space><Button onClick={() => setSyncPreview(null)}>{t("common.cancel")}</Button><Popconfirm title={t("env.confirmPushSync")} description={t("env.confirmPushSyncDescription")} onConfirm={() => void pushSync()}><Button type="primary" loading={running}>{t("env.pushSync")}</Button></Popconfirm></Space> : null} onCancel={() => setSyncPreview(null)} title={t("env.previewSync")} width={720}>
        {syncPreview && <Space direction="vertical" style={{ width: "100%" }}>
          {syncPreview.warnings.map((warning) => <Alert key={warning} type="warning" showIcon message={warning} />)}
          <List dataSource={syncPreview.changes} locale={{ emptyText: t("env.noSyncChanges") }} renderItem={(change) => <List.Item><Text>{change.sourcePath} → {change.remotePath}</Text></List.Item>} />
        </Space>}
      </Modal>
    </Space>
  );
}
