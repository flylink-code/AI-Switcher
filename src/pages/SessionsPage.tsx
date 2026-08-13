import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  Pagination,
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
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import {
  backupSessions,
  exportSessions,
  loadSessionMessages,
  exportSession as exportSessionArchive,
  importSession as importSessionArchive,
  listTrashedSessions,
  restoreTrashedSession,
  scanSessions,
  searchSessionContents,
  syncCodexSessionProviders,
  trashSession as trashSessionArchive,
} from "@/services/api";
import type {
  SessionMessage,
  SessionArchiveInfo,
  SessionMeta,
  SessionProvider,
  SessionScanResult,
} from "@/types/backend";
import { WorkspaceTargetSegmented } from "@/components/WorkspaceTargetSegmented";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

type DirectoryFilter = "all" | "yes" | "no";
type TimeFilter = "all" | "day" | "week" | "month";
type SortMode = "recent" | "oldest" | "directory";

function sessionProviderLabel(provider: SessionProvider): string {
  switch (provider) {
    case "claude_code":
      return "Claude Code";
    case "codex":
      return "Codex";
    case "opencode":
      return "OpenCode";
    case "pi":
      return "Pi";
    default: {
      const _exhaustive: never = provider;
      return _exhaustive;
    }
  }
}

function sessionProviderColor(provider: SessionProvider): string {
  switch (provider) {
    case "claude_code":
      return "purple";
    case "codex":
      return "green";
    case "opencode":
      return "cyan";
    case "pi":
      return "blue";
    default: {
      const _exhaustive: never = provider;
      return _exhaustive;
    }
  }
}

const PAGE_SIZE = 50;
const EMPTY_RESULT: SessionScanResult = { sessions: [], providers: [], total: 0, offset: 0 };
/** Matches backend `RUNTIME_RECOVERED_EVENT` after post-update relaunch recovery. */
const RUNTIME_RECOVERED_EVENT = "runtime-recovered";
const CODEX_EMPTY_RETRY_DELAYS_MS = [2_000, 5_000, 12_000] as const;

export default function SessionsPage() {
  const { t, i18n } = useTranslation();
  const [toast, toastContext] = message.useMessage();
  const { token } = theme.useToken();
  const [result, setResult] = useState<SessionScanResult>(EMPTY_RESULT);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [contentSearch, setContentSearch] = useState(false);
  const provider = usePagePreferencesStore((state) => state.sessionsProvider);
  const setSessionsProvider = usePagePreferencesStore((state) => state.setSessionsProvider);
  const [directory, setDirectory] = useState<DirectoryFilter>("all");
  const [time, setTime] = useState<TimeFilter>("all");
  const [sort, setSort] = useState<SortMode>("recent");
  const [page, setPage] = useState(1);
  useEffect(() => {
    // Codex rows usually have projectDir after SQLite enrichment; Claude Code filters
    // like "no directory" must not silently empty the Codex list after switching.
    setDirectory("all");
    setSort((current) =>
      (provider === "pi" || provider === "opencode") && current === "directory" ? "recent" : current,
    );
  }, [provider]);
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
  const [repairingCodex, setRepairingCodex] = useState(false);
  const emptyRetryRef = useRef(0);
  // Several post-update recovery paths can request the same list.  Keep only
  // the newest response so a delayed empty scan cannot overwrite fresh rows.
  const browseRequestRef = useRef(0);

  const needsFullScan =
    directory !== "all" ||
    time !== "all" ||
    sort !== "recent" ||
    Boolean(query.trim());
  // Always page on the backend. Path-only listing is cheap; avoid unbounded scans.
  const pageForFetch = needsFullScan || contentSearch ? 1 : page;

  const loadBrowse = useCallback(async () => {
    const requestId = ++browseRequestRef.current;
    setLoading(true);
    setError(null);
    try {
      const offset = needsFullScan ? 0 : (pageForFetch - 1) * PAGE_SIZE;
      // Cap even "full" filter scans — filtering is applied client-side on returned rows
      // for directory/time/query when we fetch a larger first page.
      const limit = needsFullScan ? Math.max(PAGE_SIZE * 20, 500) : PAGE_SIZE;
      const next = await scanSessions(provider, offset, limit);
      if (requestId !== browseRequestRef.current) return;
      setResult(next);
      setSelected((current) =>
        current
          ? next.sessions.find((session) => session.sourcePath === current.sourcePath) ?? null
          : null,
      );
      setSelectedPaths((current) => {
        const valid = new Set(next.sessions.map((item) => item.sourcePath));
        return new Set([...current].filter((path) => valid.has(path)));
      });
    } catch (reason) {
      if (requestId !== browseRequestRef.current) return;
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (requestId === browseRequestRef.current) {
        setLoading(false);
      }
    }
  }, [needsFullScan, pageForFetch, provider]);

  const refresh = useCallback(async () => {
    setContentSearch(false);
    await loadBrowse();
  }, [loadBrowse]);

  useEffect(() => {
    if (contentSearch) return;
    void loadBrowse();
  }, [contentSearch, loadBrowse]);

  useEffect(() => {
    return () => {
      // Ignore a late IPC response after the page has been unmounted.
      browseRequestRef.current += 1;
    };
  }, []);

  useEffect(() => {
    setPage(1);
  }, [provider, directory, time, sort, query, contentSearch]);

  useEffect(() => {
    emptyRetryRef.current = 0;
  }, [provider]);

  // After NSIS `/R`, the first Codex scan can race locks / incomplete FS settle.
  // Retry a couple of times (aligned with relaunch recovery + usage sync).
  useEffect(() => {
    if (provider !== "codex" || contentSearch || loading || error) return;
    if (result.total > 0) {
      emptyRetryRef.current = 0;
      return;
    }
    if (emptyRetryRef.current >= CODEX_EMPTY_RETRY_DELAYS_MS.length) return;
    const delay = CODEX_EMPTY_RETRY_DELAYS_MS[emptyRetryRef.current];
    emptyRetryRef.current += 1;
    const timer = window.setTimeout(() => {
      void loadBrowse();
    }, delay);
    return () => window.clearTimeout(timer);
  }, [provider, contentSearch, loading, error, result.total, loadBrowse]);

  // Backend emits this after post-update proxy + Codex session-provider recovery.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen(RUNTIME_RECOVERED_EVENT, () => {
      if (cancelled) return;
      emptyRetryRef.current = 0;
      void refresh();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
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
        provider,
      );
      setResult(next);
      setSelected(null);
      setMessages([]);
      setContentSearch(true);
      setPage(1);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setLoading(false);
    }
  };

  const filteredSessions = useMemo(() => {
    const now = Date.now();
    const cutoffs: Record<TimeFilter, number> = {
      all: 0,
      day: now - 24 * 60 * 60 * 1000,
      week: now - 7 * 24 * 60 * 60 * 1000,
      month: now - 30 * 24 * 60 * 60 * 1000,
    };
    const normalizedQuery = query.trim().toLocaleLowerCase();
    const sessions = result.sessions.filter((session) => {
      if (session.provider !== provider) return false;
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
      const pinDelta = Number(Boolean(right.pinned)) - Number(Boolean(left.pinned));
      if (pinDelta !== 0 && sort === "recent") {
        return pinDelta;
      }
      const leftTime = left.lastActiveAt ?? left.createdAt ?? 0;
      const rightTime = right.lastActiveAt ?? right.createdAt ?? 0;
      return sort === "oldest" ? leftTime - rightTime : rightTime - leftTime;
    });
  }, [contentSearch, directory, provider, query, result.sessions, sort, time]);

  const listTotal = needsFullScan || contentSearch ? filteredSessions.length : result.total;
  const visibleSessions = useMemo(() => {
    if (!(needsFullScan || contentSearch)) return filteredSessions;
    const start = (page - 1) * PAGE_SIZE;
    return filteredSessions.slice(start, start + PAGE_SIZE);
  }, [contentSearch, filteredSessions, needsFullScan, page]);

  const batchableSessions = useMemo(
    // OpenCode 会话是 opencode.db 行，后端不支持归档/导出/回收站。
    () => visibleSessions.filter((session) => session.provider !== "opencode"),
    [visibleSessions],
  );
  const selectedSessions = batchableSessions.filter((session) => selectedPaths.has(session.sourcePath));
  const selectedProvider = selectedSessions[0]?.provider;
  const mixedSelection = false;
  const archiveProvider = provider;
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
      if (!selectedProvider || mixedSelection) return;
      const result = await backupSessions(selectedProvider, [...selectedPaths]);
      void toast.success(t("sessions.batchBackedUp", { count: result.archives.length }));
    } catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };

  const repairCodexSessions = async () => {
    setRepairingCodex(true);
    try {
      const result = await syncCodexSessionProviders();
      if (result.status === "warning") {
        void toast.warning(result.message);
      } else if (result.changedSessionFiles > 0 || result.sqliteRowsUpdated > 0) {
        void toast.success(
          t("sessions.codexRepairSummary", {
            files: result.changedSessionFiles,
            rows: result.sqliteRowsUpdated,
          }),
        );
      } else {
        void toast.success(t("sessions.codexRepairUpToDate"));
      }
      if (result.skippedLockedFiles.length > 0) {
        void toast.warning(
          t("sessions.codexRepairSkipped", { count: result.skippedLockedFiles.length }),
        );
      }
      await refresh();
    } catch (reason) {
      void toast.error(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setRepairingCodex(false);
    }
  };

  const selectExportDirectory = async (): Promise<string | null> => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: t("sessions.selectExportDirectory"),
    });
    return typeof selected === "string" ? selected : null;
  };

  const selectImportArchive = async () => {
    const selected = await open({
      directory: false,
      multiple: false,
      title: t("sessions.selectImportArchive"),
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (typeof selected === "string") setImportPath(selected);
  };

  const exportSelected = async () => {
    if (!selectedPaths.size) return;
    const destinationDir = await selectExportDirectory();
    if (!destinationDir) return;
    setSessionAction(true);
    try {
      if (!selectedProvider || mixedSelection) return;
      const archive = await exportSessions(selectedProvider, [...selectedPaths], destinationDir);
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
    const destinationDir = await selectExportDirectory();
    if (!destinationDir) return;
    setSessionAction(true);
    try { const archive = await exportSessionArchive(session.provider, session.sourcePath, destinationDir); void toast.success(t("sessions.exported", { path: archive.archivePath })); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const trashSession = async (session: SessionMeta) => {
    setSessionAction(true);
    try { await trashSessionArchive(session.provider, session.sourcePath); void toast.success(t("sessions.trashed")); await refresh(); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const importSession = async () => {
    if (!importPath.trim()) return;
    setSessionAction(true);
    try { await importSessionArchive(archiveProvider, importPath); setImportOpen(false); setImportPath(""); void toast.success(t("sessions.imported")); await refresh(); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const openTrash = async () => {
    setSessionAction(true);
    try { setTrashedArchives(await listTrashedSessions(archiveProvider)); setTrashOpen(true); }
    catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };
  const restoreArchive = async (archive: SessionArchiveInfo) => {
    setSessionAction(true);
    try {
      await restoreTrashedSession(archiveProvider, archive.archivePath);
      setTrashedArchives(await listTrashedSessions(archiveProvider));
      await refresh();
      void toast.success(t("sessions.restored"));
    } catch (reason) { void toast.error(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSessionAction(false); }
  };

  const locale = i18n.language === "en-US" ? "en-US" : "zh-CN";
  const degradedProviders = useMemo(
    () => result.providers.filter((item) => item.status === "degraded"),
    [result.providers],
  );
  const activeProviderStatus = useMemo(
    () => result.providers.find((item) => item.provider === provider),
    [provider, result.providers],
  );

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      {toastContext}

      {error && <Alert type="error" showIcon message={t("sessions.loadFailed")} description={error} />}
      {degradedProviders.map((item) => (
        <Alert
          key={item.provider}
          type="warning"
          showIcon
          message={t("sessions.degradedTitle")}
          description={item.detail || t("sessions.degradedHint")}
        />
      ))}
      {provider === "codex" && !loading && !error && listTotal === 0 ? (
        <Alert
          type="info"
          showIcon
          message={t("sessions.codexEmptyTitle")}
          description={
            <Space direction="vertical" size={8}>
              <span>
                {directory !== "all" || time !== "all" || query.trim()
                  ? t("sessions.codexEmptyFilteredHint")
                  : t("sessions.codexEmptyHint", {
                      path: activeProviderStatus?.rootPath || t("sessions.codexEmptyPathUnknown"),
                      detail: activeProviderStatus?.detail || "",
                    })}
              </span>
              {(directory !== "all" || time !== "all" || query.trim()) && (
                <Button
                  onClick={() => {
                    setDirectory("all");
                    setTime("all");
                    setQuery("");
                  }}
                >
                  {t("sessions.clearFilters")}
                </Button>
              )}
              <Tooltip title={t("sessions.codexRepairHint")}>
                <Button loading={repairingCodex} onClick={() => void repairCodexSessions()}>
                  {t("sessions.codexRepair")}
                </Button>
              </Tooltip>
            </Space>
          }
        />
      ) : null}

      <WorkspaceTargetSegmented<SessionProvider>
        value={provider}
        onChange={setSessionsProvider}
        t={t}
        targets={["claude_code", "codex", "opencode", "pi"]}
      />

      <Card size="small" className="page-surface">
        <div style={{ display: "flex", flexWrap: "wrap", gap: 12, alignItems: "center", justifyContent: "space-between" }}>
          <Space wrap size={8}>
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
              style={{ width: 260 }}
            />
            <Button icon={<SearchOutlined />} onClick={() => void runContentSearch()}>
              {t("sessions.searchContents")}
            </Button>
            {provider !== "pi" && provider !== "opencode" ? (
              <Select<DirectoryFilter>
                value={directory}
                onChange={setDirectory}
                style={{ width: 130 }}
                options={[
                  { value: "all", label: t("sessions.allDirectories") },
                  { value: "yes", label: t("sessions.hasDirectory") },
                  { value: "no", label: t("sessions.noDirectory") },
                ]}
              />
            ) : null}
            <Select<TimeFilter>
              value={time}
              onChange={setTime}
              style={{ width: 120 }}
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
              style={{ width: 130 }}
              options={[
                { value: "recent", label: t("sessions.sortRecent") },
                { value: "oldest", label: t("sessions.sortOldest") },
                ...(provider !== "pi" && provider !== "opencode"
                  ? [{ value: "directory" as const, label: t("sessions.sortDirectory") }]
                  : []),
              ]}
            />
          </Space>
          <Space wrap size={8}>
            <Button icon={<ReloadOutlined />} onClick={() => void refresh()}>
              {t("common.refresh")}
            </Button>
            <Button disabled={!batchableSessions.length} onClick={toggleAllVisible}>
              {allBatchableSelected ? t("sessions.clearSelection") : t("sessions.selectVisible")}
            </Button>
            <Button loading={sessionAction} disabled={!selectedPaths.size || mixedSelection} onClick={() => void backupSelected()}>
              {t("sessions.backupSelected", { count: selectedPaths.size })}
            </Button>
            <Button type="primary" loading={sessionAction} disabled={!selectedPaths.size || mixedSelection} onClick={() => void exportSelected()}>
              {t("sessions.exportSelected", { count: selectedPaths.size })}
            </Button>
            {provider !== "opencode" && (
              <>
                <Button onClick={() => setImportOpen(true)}>{t("sessions.import")}</Button>
                <Button loading={sessionAction} onClick={() => void openTrash()}>{t("sessions.trashBin")}</Button>
              </>
            )}
            {provider === "codex" ? (
              <Tooltip title={t("sessions.codexRepairHint")}>
                <Button loading={repairingCodex} onClick={() => void repairCodexSessions()}>
                  {t("sessions.codexRepair")}
                </Button>
              </Tooltip>
            ) : null}
          </Space>
        </div>
      </Card>

      <Spin spinning={loading}>
        <Row gutter={16} style={{ minHeight: 480 }}>
          <Col xs={24} lg={9}>
            <Card
              size="small"
              className="page-surface"
              title={t("sessions.listTitle", { count: listTotal })}
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
                    actions={session.provider === "opencode" ? undefined : [
                      <span
                        key="select"
                        style={{ opacity: selectedPaths.size > 0 ? 1 : 0.4, display: "inline-flex" }}
                      >
                        <Checkbox
                          checked={selectedPaths.has(session.sourcePath)}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) => toggleSelectedPath(session.sourcePath, event.target.checked)}
                          aria-label={t("sessions.selectSession", { title: session.title || session.sessionId })}
                        />
                      </span>,
                    ]}
                  >
                    <List.Item.Meta
                      title={
                        <Space>
                          <Tag color={sessionProviderColor(session.provider)}>
                            {sessionProviderLabel(session.provider)}
                          </Tag>
                          {session.pinned ? <Tag color="gold">{t("sessions.pinned")}</Tag> : null}
                          <Typography.Text ellipsis style={{ maxWidth: 200 }}>
                            {session.title || session.sessionId}
                          </Typography.Text>
                        </Space>
                      }
                      description={
                        <Space direction="vertical" size={2} style={{ width: "100%" }}>
                          <Typography.Text type="secondary" ellipsis>
                            {session.projectDir
                              || session.summary
                              || t("sessions.unknownDirectory")}
                          </Typography.Text>
                          <Typography.Text style={{ fontSize: 11, color: "var(--color-text-tertiary)" }}>
                            {formatTime(session.lastActiveAt ?? session.createdAt, locale)}
                          </Typography.Text>
                        </Space>
                      }
                    />
                  </List.Item>
                )}
              />
              {listTotal > PAGE_SIZE ? (
                <div style={{ padding: 12, display: "flex", justifyContent: "center" }}>
                  <Pagination
                    size="small"
                    current={page}
                    pageSize={PAGE_SIZE}
                    total={listTotal}
                    showSizeChanger={false}
                    onChange={setPage}
                  />
                </div>
              ) : null}
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
      <Modal title={t("sessions.import")} open={importOpen} confirmLoading={sessionAction} onOk={() => void importSession()} onCancel={() => { setImportOpen(false); setImportPath(""); }}>
        <Space.Compact style={{ width: "100%" }}>
          <Input readOnly value={importPath} placeholder={t("sessions.importPlaceholder")} onPressEnter={() => void importSession()} />
          <Button onClick={() => void selectImportArchive()}>{t("sessions.chooseArchive")}</Button>
        </Space.Compact>
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
      <Card className="page-surface" style={{ minHeight: 480 }}>
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
      className="page-surface"
      title={
        <Space>
          {session.pinned ? <Tag color="gold">{t("sessions.pinned")}</Tag> : null}
          <span>{session.title || session.sessionId}</span>
        </Space>
      }
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
        <Typography.Text type="secondary" copyable={Boolean(session.projectDir)} ellipsis>
          {session.projectDir || session.summary || t("sessions.unknownDirectory")}
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
  return value
    ? new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value))
    : "—";
}
