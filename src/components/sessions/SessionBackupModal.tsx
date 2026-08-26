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
  Space,
  Spin,
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
  getSessionBackupDir,
  listSessionBackups,
  resetSessionBackupDir,
  restoreSessionBackup,
  setSessionBackupDir,
} from "@/services/api";
import type { SessionBackupArchiveInfo, SessionProvider } from "@/types/backend";

const { Text } = Typography;

interface SessionBackupModalProps {
  open: boolean;
  provider: SessionProvider;
  onClose: () => void;
  onRestored?: () => void;
}

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

  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const dir = await getSessionBackupDir();
      setBackupDir(dir);
      const list = await listSessionBackups(provider, dir);
      setBackups(list);
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
