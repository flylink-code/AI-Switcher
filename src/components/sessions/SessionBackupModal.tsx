import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Divider,
  Empty,
  Input,
  List,
  Modal,
  Popconfirm,
  Select,
  Space,
  Spin,
  Switch,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import HistoryOutlined from "@ant-design/icons/es/icons/HistoryOutlined";
import RedoOutlined from "@ant-design/icons/es/icons/RedoOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import UploadOutlined from "@ant-design/icons/es/icons/UploadOutlined";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import {
  backupAllSessions,
  getSessionAutoBackupSettings,
  getSessionBackupDir,
  getSessionMirrorDir,
  listSessionBackups,
  resetSessionBackupDir,
  restoreSessionBackup,
  restoreSessionMirror,
  setSessionAutoBackupSettings,
  setSessionBackupDir,
} from "@/services/api";
import type { SessionAutoBackupSettings, SessionBackupArchiveInfo, SessionProvider } from "@/types/backend";

const { Text } = Typography;

interface SessionBackupModalProps {
  open: boolean;
  provider: SessionProvider;
  onClose: () => void;
  onRestored?: () => void;
}

const DEFAULT_AUTO_SETTINGS: SessionAutoBackupSettings = {
  scheduleEnabled: false,
  intervalMinutes: 60,
  keepAuto: 8,
  mirrorEnabled: false,
  activeDays: 30,
};

function formatFileSize(bytes: number): string {
  if (bytes <= 0) return "0 B";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function formatBackupTime(timestamp: number, locale: string): string {
  if (!timestamp) return "—";
  try {
    return new Intl.DateTimeFormat(locale, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).format(new Date(timestamp));
  } catch {
    return new Date(timestamp).toLocaleString();
  }
}

function providerColor(p: SessionProvider): string {
  switch (p) {
    case "claude_code":
      return "purple";
    case "codex":
      return "green";
    case "opencode":
      return "cyan";
    case "pi":
      return "blue";
    case "dsh":
      return "geekblue";
    case "cline":
      return "orange";
    default: {
      const _exhaustive: never = p;
      return _exhaustive;
    }
  }
}

export function SessionBackupModal({
  open: isOpen,
  provider,
  onClose,
  onRestored,
}: SessionBackupModalProps) {
  const { t, i18n } = useTranslation();
  const [toast, toastContext] = message.useMessage();
  const locale = i18n.language === "en-US" ? "en-US" : "zh-CN";

  const [backupDir, setBackupDir] = useState<string>("");
  const [backups, setBackups] = useState<SessionBackupArchiveInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [backingUp, setBackingUp] = useState(false);
  const [restoringPath, setRestoringPath] = useState<string | null>(null);
  const [autoSettings, setAutoSettings] = useState<SessionAutoBackupSettings>(DEFAULT_AUTO_SETTINGS);
  const [savingAuto, setSavingAuto] = useState(false);
  const [restoringMirror, setRestoringMirror] = useState(false);

  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const dir = await getSessionBackupDir();
      setBackupDir(dir);
      const [list, auto] = await Promise.all([
        listSessionBackups(provider, dir),
        getSessionAutoBackupSettings(),
      ]);
      setBackups(list);
      setAutoSettings(auto);
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, [provider, toast]);

  useEffect(() => {
    if (isOpen) {
      void loadData();
    }
  }, [isOpen, loadData]);

  const handleSelectDirectory = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: t("sessions.backup.selectDirTitle"),
        defaultPath: backupDir || undefined,
      });
      if (typeof selected === "string" && selected.trim()) {
        const next = await setSessionBackupDir(selected.trim());
        setBackupDir(next);
        const list = await listSessionBackups(provider, next);
        setBackups(list);
        void toast.success(t("sessions.backup.dirUpdated"));
      }
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const handleResetDirectory = async () => {
    try {
      const next = await resetSessionBackupDir();
      setBackupDir(next);
      const list = await listSessionBackups(provider, next);
      setBackups(list);
      void toast.success(t("sessions.backup.dirReset"));
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const handleOpenFolder = async () => {
    if (!backupDir) return;
    try {
      await openPath(backupDir);
    } catch {
      try {
        await revealItemInDir(backupDir);
      } catch (reason) {
        void toast.error(reason instanceof Error ? reason.message : String(reason));
      }
    }
  };

  const persistAutoSettings = async (next: SessionAutoBackupSettings) => {
    setSavingAuto(true);
    try {
      const saved = await setSessionAutoBackupSettings(next);
      setAutoSettings(saved);
      void toast.success(t("sessions.backup.autoSaved"));
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSavingAuto(false);
    }
  };

  const handleOpenMirrorFolder = async () => {
    try {
      const dir = await getSessionMirrorDir(provider);
      try {
        await openPath(dir);
      } catch {
        await revealItemInDir(dir);
      }
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const handleRestoreMirror = async (overwrite: boolean) => {
    setRestoringMirror(true);
    try {
      const result = await restoreSessionMirror(provider, overwrite);
      void toast.success(
        t("sessions.backup.restoreSuccess", {
          restored: result.restoredCount,
          skipped: result.skippedCount,
        }),
      );
      onRestored?.();
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRestoringMirror(false);
    }
  };

  const handleBackupAll = async () => {
    setBackingUp(true);
    try {
      const result = await backupAllSessions(provider, backupDir);
      void toast.success(
        t("sessions.backup.backupAllSuccess", {
          count: result.sessionCount,
          path: result.archivePath,
        }),
      );
      const list = await listSessionBackups(provider, backupDir);
      setBackups(list);
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBackingUp(false);
    }
  };

  const handleRestore = async (archivePath: string) => {
    setRestoringPath(archivePath);
    try {
      const result = await restoreSessionBackup(provider, archivePath, false);
      void toast.success(
        t("sessions.backup.restoreSuccess", {
          restored: result.restoredCount,
          skipped: result.skippedCount,
        }),
      );
      onRestored?.();
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRestoringPath(null);
    }
  };

  const handleRestoreExternal = async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: false,
        title: t("sessions.backup.selectExternalArchive"),
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      if (typeof selected === "string" && selected.trim()) {
        await handleRestore(selected.trim());
      }
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const isSupportedProvider = provider !== "opencode" && provider !== "cline";

  return (
    <Modal
      open={isOpen}
      title={
        <Space>
          <HistoryOutlined />
          <span>{t("sessions.backup.modalTitle")}</span>
          <Tag color={providerColor(provider)}>{provider}</Tag>
        </Space>
      }
      width={720}
      footer={[
        <Button key="close" onClick={onClose}>
          {t("common.close")}
        </Button>,
      ]}
      onCancel={onClose}
      destroyOnHidden
    >
      {toastContext}
      <Space direction="vertical" size={16} style={{ width: "100%", marginTop: 8 }}>
        <Alert
          type="info"
          showIcon
          message={t("sessions.backup.tipTitle")}
          description={t("sessions.backup.tipDesc")}
        />

        {/* 备份目录设置卡片 */}
        <Card size="small" title={t("sessions.backup.dirConfigTitle")}>
          <Space direction="vertical" size={8} style={{ width: "100%" }}>
            <Text type="secondary" style={{ fontSize: 13 }}>
              {t("sessions.backup.currentDirLabel")}
            </Text>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <Input
                readOnly
                value={backupDir}
                placeholder={t("sessions.backup.dirPlaceholder")}
                style={{ flex: 1 }}
              />
              <Button icon={<FolderOpenOutlined />} onClick={() => void handleSelectDirectory()}>
                {t("sessions.backup.browseBtn")}
              </Button>
              <Button onClick={() => void handleOpenFolder()}>
                {t("sessions.backup.openFolderBtn")}
              </Button>
              <Tooltip title={t("sessions.backup.resetDirTooltip")}>
                <Button icon={<RedoOutlined />} onClick={() => void handleResetDirectory()}>
                  {t("sessions.backup.resetBtn")}
                </Button>
              </Tooltip>
            </div>
          </Space>
        </Card>

        <Card size="small" title={t("sessions.backup.autoTitle")}>
          <Space direction="vertical" size={12} style={{ width: "100%" }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("sessions.backup.autoHint")}
            </Text>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                gap: 12,
                flexWrap: "wrap",
              }}
            >
              <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                <Text strong>{t("sessions.backup.scheduleLabel")}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("sessions.backup.scheduleHint")}
                </Text>
              </Space>
              <Space>
                <Select
                  size="small"
                  disabled={!autoSettings.scheduleEnabled || savingAuto}
                  value={autoSettings.intervalMinutes}
                  style={{ minWidth: 120 }}
                  onChange={(value) =>
                    void persistAutoSettings({ ...autoSettings, intervalMinutes: Number(value) })
                  }
                  options={[
                    { value: 15, label: t("sessions.backup.interval15") },
                    { value: 60, label: t("sessions.backup.interval60") },
                    { value: 360, label: t("sessions.backup.interval360") },
                    { value: 1440, label: t("sessions.backup.interval1440") },
                  ]}
                />
                <Switch
                  checked={autoSettings.scheduleEnabled}
                  disabled={savingAuto || !isSupportedProvider}
                  onChange={(checked) =>
                    void persistAutoSettings({ ...autoSettings, scheduleEnabled: checked })
                  }
                />
              </Space>
            </div>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                gap: 12,
                flexWrap: "wrap",
              }}
            >
              <Space direction="vertical" size={0} style={{ minWidth: 0, flex: 1 }}>
                <Text strong>{t("sessions.backup.mirrorLabel")}</Text>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {t("sessions.backup.mirrorHint")}
                </Text>
              </Space>
              <Space>
                <Select
                  size="small"
                  disabled={!autoSettings.mirrorEnabled || savingAuto}
                  value={autoSettings.activeDays}
                  style={{ minWidth: 140 }}
                  onChange={(value) =>
                    void persistAutoSettings({ ...autoSettings, activeDays: Number(value) })
                  }
                  options={[
                    { value: 7, label: t("sessions.backup.active7") },
                    { value: 30, label: t("sessions.backup.active30") },
                    { value: 90, label: t("sessions.backup.active90") },
                    { value: 0, label: t("sessions.backup.activeAll") },
                  ]}
                />
                <Switch
                  checked={autoSettings.mirrorEnabled}
                  disabled={savingAuto || !isSupportedProvider}
                  onChange={(checked) =>
                    void persistAutoSettings({ ...autoSettings, mirrorEnabled: checked })
                  }
                />
              </Space>
            </div>
            <Space wrap>
              <Button
                icon={<FolderOpenOutlined />}
                disabled={!isSupportedProvider}
                onClick={() => void handleOpenMirrorFolder()}
              >
                {t("sessions.backup.openMirrorBtn")}
              </Button>
              <Popconfirm
                title={t("sessions.backup.restoreMirrorTitle")}
                description={t("sessions.backup.restoreMirrorMissingDesc")}
                okText={t("sessions.backup.restoreMirrorMissing")}
                cancelText={t("common.cancel")}
                disabled={!isSupportedProvider}
                onConfirm={() => void handleRestoreMirror(false)}
              >
                <Button loading={restoringMirror} disabled={!isSupportedProvider}>
                  {t("sessions.backup.restoreMirrorBtn")}
                </Button>
              </Popconfirm>
              <Popconfirm
                title={t("sessions.backup.restoreMirrorOverwriteTitle")}
                description={t("sessions.backup.restoreMirrorOverwriteDesc")}
                okText={t("sessions.backup.restoreMirrorOverwrite")}
                cancelText={t("common.cancel")}
                disabled={!isSupportedProvider}
                onConfirm={() => void handleRestoreMirror(true)}
              >
                <Button disabled={!isSupportedProvider}>{t("sessions.backup.restoreMirrorOverwriteBtn")}</Button>
              </Popconfirm>
            </Space>
          </Space>
        </Card>

        {/* 全量备份操作 */}
        <Card size="small" title={t("sessions.backup.actionTitle")}>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              flexWrap: "wrap",
              gap: 12,
            }}
          >
            <div>
              <Text strong>{t("sessions.backup.backupAllTitle", { provider })}</Text>
              <br />
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t("sessions.backup.backupAllDesc")}
              </Text>
            </div>
            <Space>
              <Button
                icon={<UploadOutlined />}
                onClick={() => void handleRestoreExternal()}
                disabled={!isSupportedProvider}
              >
                {t("sessions.backup.restoreExternalBtn")}
              </Button>
              <Button
                type="primary"
                loading={backingUp}
                disabled={!isSupportedProvider}
                onClick={() => void handleBackupAll()}
              >
                {t("sessions.backup.backupAllBtn")}
              </Button>
            </Space>
          </div>
        </Card>

        <Divider style={{ margin: "4px 0" }} />

        {/* 历史备份与恢复列表 */}
        <div>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "center",
              marginBottom: 8,
            }}
          >
            <Space>
              <Text strong>{t("sessions.backup.historyTitle")}</Text>
              <Tag>{backups.length}</Tag>
            </Space>
            <Button
              size="small"
              icon={<ReloadOutlined />}
              loading={loading}
              onClick={() => void loadData()}
            >
              {t("common.refresh")}
            </Button>
          </div>

          <Spin spinning={loading}>
            <List
              bordered
              size="small"
              style={{ maxHeight: 280, overflowY: "auto" }}
              dataSource={backups}
              locale={{
                emptyText: (
                  <Empty
                    image={Empty.PRESENTED_IMAGE_SIMPLE}
                    description={t("sessions.backup.noBackups")}
                  />
                ),
              }}
              renderItem={(item) => (
                <List.Item
                  actions={[
                    <Popconfirm
                      key="restore"
                      title={t("sessions.backup.restoreConfirmTitle")}
                      description={t("sessions.backup.restoreConfirmDesc", {
                        count: item.sessionCount,
                      })}
                      okText={t("sessions.backup.confirmRestore")}
                      cancelText={t("common.cancel")}
                      onConfirm={() => void handleRestore(item.archivePath)}
                      disabled={!isSupportedProvider}
                    >
                      <Button
                        type="link"
                        size="small"
                        loading={restoringPath === item.archivePath}
                        disabled={!isSupportedProvider}
                      >
                        {t("sessions.backup.restoreBtn")}
                      </Button>
                    </Popconfirm>,
                  ]}
                >
                  <List.Item.Meta
                    title={
                      <Space>
                        <Tag color={providerColor(item.provider)}>{item.provider}</Tag>
                        {item.isAuto ? <Tag>{t("sessions.backup.autoTag")}</Tag> : null}
                        <Text strong style={{ fontSize: 13 }}>
                          {item.filename}
                        </Text>
                      </Space>
                    }
                    description={
                      <Space size={16} style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>
                        <span>
                          {t("sessions.backup.sessionCount", { count: item.sessionCount })}
                        </span>
                        <span>{formatFileSize(item.fileSize)}</span>
                        <span>{formatBackupTime(item.createdAt, locale)}</span>
                      </Space>
                    }
                  />
                </List.Item>
              )}
            />
          </Spin>
        </div>
      </Space>
    </Modal>
  );
}
