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
} from "antd";
import {
  ReloadOutlined,
  DatabaseOutlined,
  ApiOutlined,
  SafetyCertificateOutlined,
} from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import type { PathsInfo, DbInfo } from "@/types/backend";
import { backupNow, getDbInfo, getPaths, ping } from "@/services/api";

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
  const [paths, setPaths] = useState<PathsInfo | null>(null);
  const [db, setDb] = useState<DbInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pingResult, setPingResult] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

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

  useEffect(() => {
    void refresh();
  }, [refresh]);

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
        { key: "claudeDesktopConfigLibrary", value: paths.claudeDesktopConfigLibrary },
        { key: "claudeDesktopMetaPath", value: paths.claudeDesktopMetaPath },
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
    </Spin>
  );
}
