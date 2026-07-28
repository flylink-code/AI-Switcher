import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  Empty,
  Input,
  List,
  Modal,
  Popconfirm,
  Row,
  Select,
  Space,
  Spin,
  Tag,
  Tooltip,
  Typography,
  message,
  theme,
} from "antd";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import DesktopOutlined from "@ant-design/icons/es/icons/DesktopOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import {
  backupClaudeCodeSessions,
  exportClaudeCodeSessions,
  loadSessionMessages,
  exportClaudeCodeSession,
  importClaudeCodeSession,
  listTrashedClaudeCodeSessions,
  restoreTrashedClaudeCodeSession,
  scanSessions,
  searchSessionContents,
  trashClaudeCodeSession,
} from "@/services/api";
import type {
  SessionMessage,
  SessionArchiveInfo,
  SessionMeta,
  SessionProvider,
  SessionScanResult,
} from "@/types/backend";

type ProviderFilter = "all" | SessionProvider;
type DirectoryFilter = "all" | "yes" | "no";
type TimeFilter = "all" | "day" | "week" | "month";
type SortMode = "recent" | "oldest" | "directory";

const EMPTY_RESULT: SessionScanResult = { sessions: [], providers: [] };

export default function SessionsPage() {
  const { t, i18n } = useTranslation();
  const [toast, toastContext] = message.useMessage();
  const { token } = theme.useToken();
  const [result, setResult] = useState<SessionScanResult>(EMPTY_RESULT);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [contentSearch, setContentSearch] = useState(false);
  const [provider, setProvider] = useState<ProviderFilter>("all");
  const [directory, setDirectory] = useState<DirectoryFilter>("all");
  const [time, setTime] = useState<TimeFilter>("all");
  const [sort, setSort] = useState<SortMode>("recent");
  const [selected, setSelected] = useState<SessionMeta | null>(null);
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [messagesLoading, setMessagesLoading] = useState(false);
  const [messageQuery, setMessageQuery] = useState("");
  const [importPath, setImportPath] = useState("");
  const [importOpen, setImportOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trashedArchives, setTrashedArchives] = useState<SessionArchiveInfo[]>([]);
  const [sessionAction, setSessionAction] = useState(false);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(() => new Set());

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    setContentSearch(false);
    try {
      const next = await scanSessions();
      setResult(next);
      setSelected((current) =>
        current
          ? next.sessions.find((session) => session.sourcePath === current.sourcePath) ?? null
          : null,
      );
      setSelectedPaths((current) => {
        const valid = new Set(next.sessions.filter((item) => item.provider === "claude_code").map((item) => item.sourcePath));
        return new Set([...current].filter((path) => valid.has(path)));
      });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runContentSearch = async () => {
    const trimmed = query.trim();
    if (!trimmed) {
      void toast.warning(t("sessions.searchRequired"));
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const next = await searchSessionContents(
        trimmed,
        provider === "all" ? undefined : provider,
      );
      setResult(next);
      setSelected(null);
      setMessages([]);
      setContentSearch(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  };

  const visibleSessions = useMemo(() => {
    const now = Date.now();
    const cutoffs: Record<TimeFilter, number> = {
      all: 0,
      day: now - 24 * 60 * 60 * 1000,
      week: now - 7 * 24 * 60 * 60 * 1000,
      month: now - 30 * 24 * 60 * 60 * 1000,
    };
    const normalizedQuery = query.trim().toLocaleLowerCase();
    const sessions = result.sessions.filter((session) => {
      if (provider !== "all" && session.provider !== provider) return false;
      if (directory === "yes" && !session.projectDir) return false;
      if (directory === "no" && session.projectDir) return false;
      const activeAt = session.lastActiveAt ?? session.createdAt ?? 0;
      if (time !== "all" && activeAt < cutoffs[time]) return false;
      if (contentSearch || !normalizedQuery) return true;
      return [session.sessionId, session.title, session.summary, session.projectDir]
        .filter(Boolean)
        .some((value) => value!.toLocaleLowerCase().includes(normalizedQuery));
    });
    return sessions.sort((left, right) => {
      if (sort === "directory") {
        return (left.projectDir ?? "").localeCompare(right.projectDir ?? "");
      }
      const leftTime = left.lastActiveAt ?? left.createdAt ?? 0;
      const rightTime = right.lastActiveAt ?? right.createdAt ?? 0;
      return sort === "oldest" ? leftTime - rightTime : rightTime - leftTime;
    });
  }, [contentSearch, directory, provider, query, result.sessions, sort, time]);

  const batchableSessions = useMemo(
    () => visibleSessions.filter((session) => session.provider === "claude_code"),
    [visibleSessions],
  );
  const allBatchableSelected = batchableSessions.length > 0 && batchableSessions.every((session) => selectedPaths.has(session.sourcePath));

  const toggleSelectedPath = (sourcePath: string, checked: boolean) => {
    setSelectedPaths((current) => {
      const next = new Set(current);
      if (checked) next.add(sourcePath); else next.delete(sourcePath);
      return next;
    });
  };

  const toggleAllVisible = () => {
    setSelectedPaths((current) => {
      const next = new Set(current);
      batchableSessions.forEach((session) => {
        if (allBatchableSelected) next.delete(session.sourcePath); else next.add(session.sourcePath);
      });
      return next;
    });
  };

  const backupSelected = async () => {
    if (!selectedPaths.size) return;
    setSessionAction(true);
    try {
      const result = await backupClaudeCodeSessions([...selectedPaths]);
      void toast.success(t("sessions.batchBackedUp", { count: result.archives.length }));
    } catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };

  const exportSelected = async () => {
    if (!selectedPaths.size) return;
    setSessionAction(true);
    try {
      const archive = await exportClaudeCodeSessions([...selectedPaths]);
      void toast.success(t("sessions.batchExported", { count: archive.sessionCount, path: archive.archivePath }));
    } catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };

  const selectSession = async (session: SessionMeta) => {
    setSelected(session);
    setMessages([]);
    setMessageQuery("");
    setMessagesLoading(true);
    try {
      setMessages(await loadSessionMessages(session.provider, session.sourcePath));
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setMessagesLoading(false);
    }
  };

  const copyText = async (value: string, successKey: string) => {
    try {
      await navigator.clipboard.writeText(value);
      void toast.success(t(successKey));
    } catch {
      void toast.error(t("sessions.copyFailed"));
    }
  };

  const exportSession = async (session: SessionMeta) => {
    setSessionAction(true);
    try { const archive = await exportClaudeCodeSession(session.sourcePath); void toast.success(t("sessions.exported", { path: archive.archivePath })); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const trashSession = async (session: SessionMeta) => {
    setSessionAction(true);
    try { await trashClaudeCodeSession(session.sourcePath); void toast.success(t("sessions.trashed")); await refresh(); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const importSession = async () => {
    if (!importPath.trim()) return;
    setSessionAction(true);
    try { await importClaudeCodeSession(importPath); setImportOpen(false); setImportPath(""); void toast.success(t("sessions.imported")); await refresh(); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const openTrash = async () => {
    setSessionAction(true);
    try { setTrashedArchives(await listTrashedClaudeCodeSessions()); setTrashOpen(true); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const restoreArchive = async (archive: SessionArchiveInfo) => {
    setSessionAction(true);
    try {
      await restoreTrashedClaudeCodeSession(archive.archivePath);
      setTrashedArchives(await listTrashedClaudeCodeSessions());
      await refresh();
      void toast.success(t("sessions.restored"));
    } catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };

  const desktopStatus = result.providers.find(
    (item) => item.provider === "claude_desktop",
  );
  const locale = i18n.language === "en-US" ? "en-US" : "zh-CN";

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      {toastContext}
      <div>
        <Typography.Title level={3} style={{ margin: 0 }}>
          {t("sessions.title")}
        </Typography.Title>
        <Typography.Text type="secondary">{t("sessions.subtitle")}</Typography.Text>
      </div>

      {desktopStatus && (
        <Alert
          type="info"
          showIcon
          message={t("sessions.desktopLimited")}
          description={t(`sessions.desktopStatus.${desktopStatus.status}`, {
            defaultValue: desktopStatus.detail,
          })}
          action={
            <Button
              size="small"
              icon={<DesktopOutlined />}
              onClick={() =>
                void openUrl("claude://claude.ai/new").catch(() => {
                  void toast.error(t("sessions.openDesktopFailed"));
                })
              }
            >
              {t("sessions.openDesktop")}
            </Button>
          }
        />
      )}

      {error && <Alert type="error" showIcon message={t("sessions.loadFailed")} description={error} />}

      <Card size="small">
        <Space wrap>
          <Input
            allowClear
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              if (contentSearch) setContentSearch(false);
            }}
            onPressEnter={() => void runContentSearch()}
            prefix={<SearchOutlined />}
            placeholder={t("sessions.searchPlaceholder")}
            style={{ width: 300 }}
          />
          <Button icon={<SearchOutlined />} onClick={() => void runContentSearch()}>
            {t("sessions.searchContents")}
          </Button>
          <Select<ProviderFilter>
            value={provider}
            onChange={setProvider}
            style={{ width: 150 }}
            options={[
              { value: "all", label: t("sessions.allProviders") },
              { value: "claude_code", label: "Claude Code" },
              { value: "claude_desktop", label: "Claude Desktop" },
            ]}
          />
          <Select<DirectoryFilter>
            value={directory}
            onChange={setDirectory}
            style={{ width: 150 }}
            options={[
              { value: "all", label: t("sessions.allDirectories") },
              { value: "yes", label: t("sessions.hasDirectory") },
              { value: "no", label: t("sessions.noDirectory") },
            ]}
          />
          <Select<TimeFilter>
            value={time}
            onChange={setTime}
            style={{ width: 140 }}
            options={[
              { value: "all", label: t("sessions.allTime") },
              { value: "day", label: t("sessions.lastDay") },
              { value: "week", label: t("sessions.lastWeek") },
              { value: "month", label: t("sessions.lastMonth") },
            ]}
          />
          <Select<SortMode>
            value={sort}
            onChange={setSort}
            style={{ width: 150 }}
            options={[
              { value: "recent", label: t("sessions.sortRecent") },
              { value: "oldest", label: t("sessions.sortOldest") },
              { value: "directory", label: t("sessions.sortDirectory") },
            ]}
          />
          <Button icon={<ReloadOutlined />} onClick={() => void refresh()}>
            {t("common.refresh")}
          </Button>
          <Button disabled={!batchableSessions.length} onClick={toggleAllVisible}>
            {allBatchableSelected ? t("sessions.clearSelection") : t("sessions.selectVisible")}
          </Button>
          <Button loading={sessionAction} disabled={!selectedPaths.size} onClick={() => void backupSelected()}>
            {t("sessions.backupSelected", { count: selectedPaths.size })}
          </Button>
          <Button type="primary" loading={sessionAction} disabled={!selectedPaths.size} onClick={() => void exportSelected()}>
            {t("sessions.exportSelected", { count: selectedPaths.size })}
          </Button>
          <Button onClick={() => setImportOpen(true)}>{t("sessions.import")}</Button>
          <Button loading={sessionAction} onClick={() => void openTrash()}>{t("sessions.trashBin")}</Button>
        </Space>
      </Card>

      <Spin spinning={loading}>
        <Row gutter={16} style={{ minHeight: 480 }}>
          <Col xs={24} lg={9}>
            <Card
              size="small"
              title={t("sessions.listTitle", { count: visibleSessions.length })}
              styles={{ body: { padding: 0, maxHeight: "62vh", overflow: "auto" } }}
            >
              <List
                dataSource={visibleSessions}
                locale={{ emptyText: <Empty description={t("sessions.empty")} /> }}
                renderItem={(session) => (
                  <List.Item
                    onClick={() => void selectSession(session)}
                    style={{
                      cursor: "pointer",
                      padding: 12,
                      background:
                        selected?.sourcePath === session.sourcePath
                          ? token.colorFillSecondary
                          : undefined,
                    }}
                    actions={session.provider === "claude_code" ? [
                      <Checkbox
                        key="select"
                        checked={selectedPaths.has(session.sourcePath)}
                        onClick={(event) => event.stopPropagation()}
                        onChange={(event) => toggleSelectedPath(session.sourcePath, event.target.checked)}
                        aria-label={t("sessions.selectSession", { title: session.title || session.sessionId })}
                      />,
                    ] : undefined}
                  >
                    <List.Item.Meta
                      title={
                        <Space>
                          <Tag color="purple">Claude Code</Tag>
                          <Typography.Text ellipsis style={{ maxWidth: 230 }}>
                            {session.title || session.sessionId}
                          </Typography.Text>
                        </Space>
                      }
                      description={
                        <Space direction="vertical" size={2} style={{ width: "100%" }}>
                          <Typography.Text type="secondary" ellipsis>
                            {session.projectDir || t("sessions.unknownDirectory")}
                          </Typography.Text>
                          <Typography.Text type="secondary">
                            {formatTime(session.lastActiveAt ?? session.createdAt, locale)}
                          </Typography.Text>
                        </Space>
                      }
                    />
                  </List.Item>
                )}
              />
            </Card>
          </Col>
          <Col xs={24} lg={15}>
            <SessionDetail
              session={selected}
              messages={messages}
              loading={messagesLoading}
              query={messageQuery}
              onQueryChange={setMessageQuery}
              onCopy={copyText}
              onExport={exportSession}
              onTrash={trashSession}
              actionPending={sessionAction}
              locale={locale}
            />
          </Col>
        </Row>
      </Spin>
      <Modal title={t("sessions.import")} open={importOpen} confirmLoading={sessionAction} onOk={() => void importSession()} onCancel={() => setImportOpen(false)}>
        <Input value={importPath} onChange={(event) => setImportPath(event.target.value)} placeholder={t("sessions.importPlaceholder")} onPressEnter={() => void importSession()} />
      </Modal>
      <Modal title={t("sessions.trashBin")} open={trashOpen} footer={null} onCancel={() => setTrashOpen(false)}>
        <List
          dataSource={trashedArchives}
          locale={{ emptyText: <Empty description={t("sessions.emptyTrash")} /> }}
          renderItem={(archive) => (
            <List.Item actions={[
              <Button key="restore" type="link" loading={sessionAction} onClick={() => void restoreArchive(archive)}>{t("sessions.restore")}</Button>,
            ]}>
              <List.Item.Meta
                title={archive.sessionId}
                description={formatTime(archive.createdAt, locale)}
              />
            </List.Item>
          )}
        />
      </Modal>
    </Space>
  );
}

function SessionDetail({
  session,
  messages,
  loading,
  query,
  onQueryChange,
  onCopy,
  onExport,
  onTrash,
  actionPending,
  locale,
}: {
  session: SessionMeta | null;
  messages: SessionMessage[];
  loading: boolean;
  query: string;
  onQueryChange: (value: string) => void;
  onCopy: (value: string, successKey: string) => Promise<void>;
  onExport: (session: SessionMeta) => Promise<void>;
  onTrash: (session: SessionMeta) => Promise<void>;
  actionPending: boolean;
  locale: string;
}) {
  const { t } = useTranslation();
  if (!session) {
    return (
      <Card style={{ minHeight: 480 }}>
        <Empty description={t("sessions.selectHint")} />
      </Card>
    );
  }

  const visibleMessages = query.trim()
    ? messages.filter((item) =>
        item.content.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
      )
    : messages;

  return (
    <Card
      size="small"
      title={session.title || session.sessionId}
      extra={
        <Space>
          <Tooltip title={session.resumeCommand ? undefined : t("sessions.resumeUnavailable")}>
            <Button
              size="small"
              icon={<CopyOutlined />}
              disabled={!session.resumeCommand}
              onClick={() =>
                session.resumeCommand &&
                void onCopy(session.resumeCommand, "sessions.commandCopied")
              }
            >
              {t("sessions.copyCommand")}
            </Button>
          </Tooltip>
          <Tooltip title={session.projectDir ? undefined : t("sessions.directoryUnavailable")}>
            <Button
              size="small"
              icon={<CopyOutlined />}
              disabled={!session.projectDir}
              onClick={() =>
                session.projectDir &&
                void onCopy(session.projectDir, "sessions.directoryCopied")
              }
            >
              {t("sessions.copyDirectory")}
            </Button>
          </Tooltip>
          <Button size="small" loading={actionPending} onClick={() => void onExport(session)}>{t("sessions.export")}</Button>
          <Popconfirm title={t("sessions.confirmTrash")} onConfirm={() => void onTrash(session)}>
            <Button size="small" danger loading={actionPending}>{t("sessions.trash")}</Button>
          </Popconfirm>
        </Space>
      }
      styles={{ body: { maxHeight: "62vh", overflow: "auto" } }}
    >
      <Space direction="vertical" size={8} style={{ width: "100%", marginBottom: 16 }}>
        <Typography.Text type="secondary">ID: {session.sessionId}</Typography.Text>
        <Typography.Text type="secondary" copyable ellipsis>
          {session.projectDir || t("sessions.unknownDirectory")}
        </Typography.Text>
        <Typography.Text type="secondary">
          {t("sessions.createdAt")}: {formatTime(session.createdAt, locale)}
        </Typography.Text>
        <Typography.Text type="secondary">
          {t("sessions.lastActiveAt")}: {formatTime(session.lastActiveAt, locale)}
        </Typography.Text>
        <Input
          allowClear
          value={query}
          onChange={(event) => onQueryChange(event.target.value)}
          prefix={<SearchOutlined />}
          placeholder={t("sessions.searchInSession")}
        />
      </Space>
      <Spin spinning={loading}>
        <List
          dataSource={visibleMessages}
          locale={{ emptyText: <Empty description={t("sessions.noMessages")} /> }}
          renderItem={(item) => (
            <List.Item style={{ display: "block" }}>
              <Space style={{ marginBottom: 6 }}>
                <Tag color={roleColor(item.role)}>
                  {t(`sessions.roles.${item.role}`, { defaultValue: item.role })}
                </Tag>
                {item.timestamp && (
                  <Typography.Text type="secondary">
                    {formatTime(item.timestamp, locale)}
                  </Typography.Text>
                )}
              </Space>
              <Typography.Paragraph
                style={{ whiteSpace: "pre-wrap", wordBreak: "break-word", marginBottom: 0 }}
              >
                {highlight(item.content, query)}
              </Typography.Paragraph>
            </List.Item>
          )}
        />
      </Spin>
    </Card>
  );
}

function highlight(content: string, query: string) {
  const trimmed = query.trim();
  if (!trimmed) return content;
  const escaped = trimmed.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return content.split(new RegExp(`(${escaped})`, "gi")).map((part, index) =>
    part.toLocaleLowerCase() === trimmed.toLocaleLowerCase() ? (
      <mark key={`${index}-${part}`}>{part}</mark>
    ) : (
      part
    ),
  );
}

function roleColor(role: string) {
  if (role === "user") return "blue";
  if (role === "assistant") return "purple";
  if (role === "tool") return "gold";
  return "default";
}

function formatTime(value: number | undefined, locale: string) {
  return value ? new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(value) : "—";
}
