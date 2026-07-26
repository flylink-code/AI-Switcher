import { useCallback, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Descriptions,
  List,
  Modal,
  Popconfirm,
  Skeleton,
  Space,
  Switch,
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
  PathsInfo,
  DbInfo,
  ConfigBackup,
  ProviderTarget,
} from "@/types/backend";
import {
  backupNow,
  getAutostartEnabled,
  getDbInfo,
  getPaths,
  listConfigBackups,
  ping,
  previewConfigBackup,
  restoreConfigBackup,
  setAutostartEnabled,
} from "@/services/api";

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
  const environmentQuery = useQuery({
    queryKey: ["environment", "paths-db"],
    queryFn: async (): Promise<{ paths: PathsInfo; db: DbInfo }> => {
      const [paths, db] = await Promise.all([getPaths(), getDbInfo()]);
      return { paths, db };
    },
    staleTime: 5 * 60_000,
  });
  const autostartQuery = useQuery({
    queryKey: ["environment", "autostart"],
    queryFn: getAutostartEnabled,
    staleTime: 60_000,
  });
  const paths = environmentQuery.data?.paths ?? null;
  const db = environmentQuery.data?.db ?? null;
  const error = environmentQuery.error
    ? environmentQuery.error instanceof Error
      ? environmentQuery.error.message
      : String(environmentQuery.error)
    : null;
  const [pingResult, setPingResult] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [autostartChanging, setAutostartChanging] = useState(false);
  const [backupTarget, setBackupTarget] = useState<ProviderTarget>("claude_code");
  const [configBackups, setConfigBackups] = useState<ConfigBackup[]>([]);
  const [backupPreview, setBackupPreview] = useState<string | null>(null);

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

  const onAutostartChange = useCallback(async (enabled: boolean) => {
    setAutostartChanging(true);
    try {
      await setAutostartEnabled(enabled);
      queryClient.setQueryData(["environment", "autostart"], enabled);
      void message.success(t("env.autostartUpdated"));
    } catch (e) {
      void message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setAutostartChanging(false);
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
              <Switch
                checked={autostartQuery.data ?? false}
                loading={autostartQuery.isPending || autostartChanging}
                onChange={(enabled) => void onAutostartChange(enabled)}
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
      <Modal open={backupPreview !== null} footer={null} onCancel={() => setBackupPreview(null)} title={t("env.previewBackup")} width={720}>
        <pre style={{ maxHeight: 480, overflow: "auto", whiteSpace: "pre-wrap" }}>{backupPreview}</pre>
      </Modal>
    </Space>
  );
}
