import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Checkbox,
  Divider,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useTranslation } from "react-i18next";
import type {
  AntigravityGatewayStatus,
  AntigravityCatalogModel,
  AntigravityLimiterSettings,
  AntigravityFastPathSettings,
  AntigravityExitProxy,
  AntigravityExitProxyInput,
  AntigravityExitLatencyResult,
  AntigravityExitProbeResult,
} from "@/services/api";

const { Text, Paragraph } = Typography;

type ExitProxyScheme = "socks5" | "http";

interface ExitProxyFields {
  scheme: ExitProxyScheme;
  host: string;
  port: string;
  username: string;
  password: string;
}

const EMPTY_EXIT_PROXY: ExitProxyFields = {
  scheme: "socks5",
  host: "",
  port: "",
  username: "",
  password: "",
};

function parseExitProxyUrl(value: string): ExitProxyFields {
  const trimmed = value.trim();
  if (!trimmed) return EMPTY_EXIT_PROXY;
  try {
    const url = new URL(trimmed);
    const rawScheme = url.protocol.replace(/:$/, "").toLowerCase();
    const scheme: ExitProxyScheme = rawScheme === "http" ? "http" : "socks5";
    return {
      scheme,
      host: url.hostname,
      port: url.port,
      username: decodeURIComponent(url.username),
      password: decodeURIComponent(url.password),
    };
  } catch {
    return EMPTY_EXIT_PROXY;
  }
}

function formatExitHost(host: string): string {
  if (host.includes(":") && !host.startsWith("[")) return `[${host}]`;
  return host;
}

function composeExitProxyUrl(fields: ExitProxyFields): string {
  const host = fields.host.trim();
  const port = fields.port.trim();
  if (!host || !port) return "";
  const auth =
    fields.username || fields.password
      ? `${encodeURIComponent(fields.username)}:${encodeURIComponent(fields.password)}@`
      : "";
  return `${fields.scheme}://${auth}${formatExitHost(host)}:${port}`;
}

const EXIT_NAME = "链式代理出口IP";

interface ExitRow extends ExitProxyFields {
  id: string;
  name: string;
  enabled: boolean;
}

function newExitId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `exit-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function newExitRow(): ExitRow {
  return { id: newExitId(), name: EXIT_NAME, enabled: false, ...EMPTY_EXIT_PROXY };
}

function rowFromSaved(entry: AntigravityExitProxy): ExitRow {
  return {
    id: entry.id,
    name: entry.name || EXIT_NAME,
    enabled: entry.enabled,
    ...parseExitProxyUrl(entry.proxyUrl),
  };
}

function exitFieldsOk(row: ExitRow): boolean {
  const portNumber = Number(row.port);
  return Boolean(row.host.trim()) && portNumber >= 1 && portNumber <= 65535;
}

function exitEndpointLabel(row: ExitRow): string {
  const host = row.host.trim();
  const port = row.port.trim();
  if (!host || !port) return "";
  const scheme = row.scheme === "http" ? "HTTP" : "SOCKS5";
  return `${scheme}  ${host}:${port}`;
}

function rowsToExitInputs(rows: ExitRow[]): AntigravityExitProxyInput[] {
  return rows.filter(exitFieldsOk).map((row) => ({
    id: row.id,
    name: row.name.trim() || EXIT_NAME,
    enabled: row.enabled,
    proxyUrl: composeExitProxyUrl({ ...row, port: String(Number(row.port)) }),
  }));
}

const DEFAULT_LIMITER: AntigravityLimiterSettings = {
  accountConcurrency: 4,
  subagentConcurrency: 2,
  minRequestIntervalMs: 300,
  ratePerMin: 30,
  tokenBurst: 8,
  acquireTimeoutSecs: 8,
};

const DEFAULT_FAST_PATH: AntigravityFastPathSettings = {
  quotaMock: true,
  titleSkip: true,
  prefixDetect: true,
  suggestionSkip: false,
  filepathMock: false,
  flashDegrade: true,
};

const FAST_PATH_TOGGLES: Array<{
  key: keyof AntigravityFastPathSettings;
  labelKey: string;
  labelDefault: string;
  hintKey: string;
  hintDefault: string;
}> = [
  {
    key: "quotaMock",
    labelKey: "antigravity.fastPathQuota",
    labelDefault: "额度探测",
    hintKey: "antigravity.fastPathQuotaHint",
    hintDefault: "启动时会发一条「查额度」请求。勾选后本机直接回复通过。",
  },
  {
    key: "titleSkip",
    labelKey: "antigravity.fastPathTitleSkip",
    labelDefault: "会话标题",
    hintKey: "antigravity.fastPathTitleSkipHint",
    hintDefault: "侧栏会话名本会调一次模型。勾选后本机回占位名 Conversation。",
  },
  {
    key: "prefixDetect",
    labelKey: "antigravity.fastPathPrefix",
    labelDefault: "命令前缀",
    hintKey: "antigravity.fastPathPrefixHint",
    hintDefault: "终端命令安全检查（注入、危险命令）。勾选后本机解析并回复。",
  },
  {
    key: "flashDegrade",
    labelKey: "antigravity.fastPathFlash",
    labelDefault: "后台改走 Flash",
    hintKey: "antigravity.fastPathFlashHint",
    hintDefault: "未能短路的后台任务（摘要、压缩等）改用便宜的 Gemini Flash，不走主会话模型。",
  },
  {
    key: "suggestionSkip",
    labelKey: "antigravity.fastPathSuggestion",
    labelDefault: "跳过建议",
    hintKey: "antigravity.fastPathSuggestionHint",
    hintDefault: "关掉 Claude Code 的「下一步建议」生成。默认关：误判会吞掉真实回复。",
  },
  {
    key: "filepathMock",
    labelKey: "antigravity.fastPathFilepath",
    labelDefault: "路径提取",
    hintKey: "antigravity.fastPathFilepathHint",
    hintDefault: "从命令输出里抽文件路径。默认关：误判会漏路径。",
  },
];

interface GatewayCardProps {
  status?: AntigravityGatewayStatus;
  models?: AntigravityCatalogModel[];
  onStartGateway: (port: number, apiKey?: string, outboundMode?: string, outboundUrl?: string) => Promise<void>;
  onStopGateway: () => Promise<void>;
  onSaveOutbound: (mode: "direct" | "system" | "custom", url: string) => Promise<void>;
  onSaveExit: (entries: AntigravityExitProxyInput[]) => Promise<void>;
  onProbeExit: (id: string, url: string) => Promise<AntigravityExitProbeResult>;
  onProbeLatency: (id: string, url: string) => Promise<AntigravityExitLatencyResult>;
  onSaveLimiter: (settings: AntigravityLimiterSettings) => Promise<void>;
  onSaveFastPath: (settings: AntigravityFastPathSettings) => Promise<void>;
  onRefresh: () => void;
  isStarting?: boolean;
  isStopping?: boolean;
  isSavingOutbound?: boolean;
  isSavingExit?: boolean;
  isSavingLimiter?: boolean;
  isSavingFastPath?: boolean;
}

export function GatewayCard({
  status,
  models,
  onStartGateway,
  onStopGateway,
  onSaveOutbound,
  onSaveExit,
  onProbeExit,
  onProbeLatency,
  onSaveLimiter,
  onSaveFastPath,
  onRefresh,
  isStarting = false,
  isStopping = false,
  isSavingOutbound = false,
  isSavingExit = false,
  isSavingLimiter = false,
  isSavingFastPath = false,
}: GatewayCardProps) {
  const { t } = useTranslation();

  const [portDraft, setPortDraft] = useState<number | null>(null);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [outboundModeDraft, setOutboundModeDraft] = useState<
    "direct" | "system" | "custom" | null
  >(null);
  const [outboundUrlDraft, setOutboundUrlDraft] = useState<string | null>(null);
  const [exitPending, setExitPending] = useState<ExitRow[] | null>(null);
  const [exitEditor, setExitEditor] = useState<ExitRow | null>(null);
  const [probingId, setProbingId] = useState<string | null>(null);
  const [latencyId, setLatencyId] = useState<string | null>(null);
  const [probeById, setProbeById] = useState<Record<string, AntigravityExitProbeResult>>({});
  const [latencyById, setLatencyById] = useState<Record<string, AntigravityExitLatencyResult>>({});
  const [limiterDraft, setLimiterDraft] = useState<AntigravityLimiterSettings | null>(null);
  const [fastPathDraft, setFastPathDraft] = useState<AntigravityFastPathSettings | null>(null);
  const [curlVisible, setCurlVisible] = useState(false);

  const port = portDraft ?? status?.port ?? 15830;
  const apiKey = apiKeyDraft ?? status?.apiKey ?? "";
  const outboundMode =
    outboundModeDraft ??
    (status?.outboundMode === "direct" || status?.outboundMode === "system"
      ? status.outboundMode
      : "custom");
  const outboundUrl =
    outboundUrlDraft ?? status?.outboundProxyUrl ?? "socks5://127.0.0.1:17891";
  const savedExitRows = useMemo(
    () => (status?.exitProxies ?? []).map(rowFromSaved),
    [status?.exitProxies],
  );
  const exitRows = exitPending ?? savedExitRows;
  const persistExitRows = (next: ExitRow[]) => {
    setExitPending(next);
    void onSaveExit(rowsToExitInputs(next))
      .then(() => setExitPending(null))
      .catch(() => setExitPending(null));
  };
  const setExitEnabled = (id: string, checked: boolean) => {
    persistExitRows(
      exitRows.map((row) => ({
        ...row,
        enabled: row.id === id ? checked : checked ? false : row.enabled,
      })),
    );
  };
  const probeExitRow = (row: ExitRow) => {
    if (!exitFieldsOk(row)) {
      message.warning(
        t("antigravity.exitFieldsRequired", {
          defaultValue: "主机和端口（1–65535）都要填写后才能保存或检测",
        }),
      );
      return;
    }
    const url = composeExitProxyUrl({ ...row, port: String(Number(row.port)) });
    setProbingId(row.id);
    void onProbeExit(row.id, url)
      .then((result) => {
        setProbeById((prev) => ({ ...prev, [row.id]: result }));
      })
      .catch(() => undefined)
      .finally(() => setProbingId(null));
  };
  const latencyExitRow = (row: ExitRow) => {
    if (!exitFieldsOk(row)) {
      message.warning(
        t("antigravity.exitFieldsRequired", {
          defaultValue: "主机和端口（1–65535）都要填写后才能保存或检测",
        }),
      );
      return;
    }
    const url = composeExitProxyUrl({ ...row, port: String(Number(row.port)) });
    setLatencyId(row.id);
    void onProbeLatency(row.id, url)
      .then((result) => {
        setLatencyById((prev) => ({ ...prev, [row.id]: result }));
      })
      .catch(() => undefined)
      .finally(() => setLatencyId(null));
  };
  const saveExitEditor = () => {
    if (!exitEditor) return;
    if (!exitFieldsOk(exitEditor)) {
      message.warning(
        t("antigravity.exitFieldsRequired", {
          defaultValue: "主机和端口（1–65535）都要填写后才能保存或检测",
        }),
      );
      return;
    }
    const nextRow: ExitRow = {
      ...exitEditor,
      name: exitEditor.name.trim() || EXIT_NAME,
      port: String(Number(exitEditor.port)),
    };
    const exists = exitRows.some((row) => row.id === nextRow.id);
    const next = exists
      ? exitRows.map((row) => (row.id === nextRow.id ? nextRow : row))
      : [...exitRows, nextRow];
    setExitEditor(null);
    persistExitRows(next);
  };
  const limiter = limiterDraft ?? status?.limiterSettings ?? DEFAULT_LIMITER;
  const fastPath = fastPathDraft ?? status?.fastPath ?? DEFAULT_FAST_PATH;

  const patchLimiter = (patch: Partial<AntigravityLimiterSettings>) => {
    setLimiterDraft({ ...limiter, ...patch });
  };
  const patchFastPath = (patch: Partial<AntigravityFastPathSettings>) => {
    setFastPathDraft({ ...fastPath, ...patch });
  };

  const sampleModel =
    models?.find((model) => model.id === "claude-sonnet-4-6")?.id ??
    models?.[0]?.id ??
    "claude-sonnet-4-6";

  const curlSnippet = useMemo(() => {
    const base = status?.baseUrl ?? `http://127.0.0.1:${port}`;
    const key = apiKey || "sk-ai-switcher-antigravity";
    return `curl -s ${base}/v1/messages \\\n  -H "x-api-key: ${key}" \\\n  -H "content-type: application/json" \\\n  -d '{"model":"${sampleModel}","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}'`;
  }, [apiKey, port, sampleModel, status?.baseUrl]);

  return (
    <Card title={t("antigravity.gateway")} size="small" style={{ marginBottom: 16 }}>
      <Space direction="vertical" style={{ width: "100%" }} size={12}>
        <Space wrap>
          <Tag color={status?.running ? "success" : "default"}>
            {status?.running ? t("antigravity.running") : t("antigravity.stoppedState")}
          </Tag>
          <Text type="secondary">
            {t("antigravity.accountsCount", { count: status?.accountCount ?? 0 })}
          </Text>
          <Text code>{status?.baseUrl ?? `http://127.0.0.1:${port}`}</Text>
        </Space>

        <Space wrap>
          <InputNumber
            min={1024}
            max={65535}
            value={port}
            onChange={(value) => setPortDraft(typeof value === "number" ? value : null)}
            addonBefore={t("antigravity.port")}
          />
          <Input.Password
            style={{ width: 280 }}
            value={apiKey}
            onChange={(event) => setApiKeyDraft(event.target.value)}
            placeholder="sk-..."
            addonBefore="API Key"
          />
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Text strong style={{ fontSize: 13 }}>{t("antigravity.outboundSection", { defaultValue: "出站代理" })}</Text>
          <Space wrap>
            <Select
              style={{ minWidth: 200 }}
              value={outboundMode}
              onChange={(value) => setOutboundModeDraft(value)}
              options={[
                { value: "custom", label: t("antigravity.outboundCustom") },
                { value: "direct", label: t("antigravity.outboundDirect") },
                { value: "system", label: t("antigravity.outboundSystem") },
              ]}
            />
            <Input
              style={{ width: 280 }}
              disabled={outboundMode !== "custom"}
              value={outboundUrl}
              onChange={(event) => setOutboundUrlDraft(event.target.value)}
              placeholder="socks5://127.0.0.1:17891"
              addonBefore={t("antigravity.outboundProxy")}
            />
            <Button
              loading={isSavingOutbound}
              onClick={() => onSaveOutbound(outboundMode, outboundUrl)}
            >
              {t("antigravity.outboundSave")}
            </Button>
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.outboundEffective", {
              value: status?.effectiveOutboundProxy || t("antigravity.outboundNone"),
            })}
          </Text>
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Text strong style={{ fontSize: 13 }}>
            {t("antigravity.exitSection", { defaultValue: "链式代理出口IP" })}
          </Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.exitHint", {
              defaultValue:
                "每条单独列出。添加或编辑时再填写协议、主机和账号。检测不必先启用。启用一条后，请求先走出站代理，再走这条链式代理出口IP。同时只能启用一条。",
            })}
          </Text>
          {exitRows.length === 0 ? (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("antigravity.exitEmpty", { defaultValue: "还没有链式代理出口IP" })}
            </Text>
          ) : null}
          {exitRows.map((row) => {
            const savedProbe = status?.exitProxies?.find((item) => item.id === row.id);
            const liveProbe = probeById[row.id];
            const probeOk = liveProbe ? liveProbe.ok : savedProbe?.probeOk;
            const probeMessage = liveProbe ? liveProbe.message : savedProbe?.probeMessage;
            const liveLatency = latencyById[row.id];
            const latencyOk = liveLatency ? liveLatency.ok : savedProbe?.latencyOk;
            const latencyMessage = liveLatency ? liveLatency.message : savedProbe?.latencyMessage;
            return (
              <div
                key={row.id}
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 12,
                  width: "100%",
                  padding: "8px 10px",
                  border: "1px solid var(--ant-color-border-secondary, #f0f0f0)",
                  borderRadius: 8,
                }}
              >
                <Switch
                  checked={row.enabled}
                  disabled={isSavingExit}
                  onChange={(checked) => setExitEnabled(row.id, checked)}
                />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Text strong style={{ fontSize: 13 }}>
                    {row.name || EXIT_NAME}
                  </Text>
                  <div>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {exitEndpointLabel(row)}
                    </Text>
                  </div>
                  {probeMessage ? (
                    <Text type={probeOk ? "success" : "danger"} style={{ fontSize: 12 }}>
                      {probeMessage}
                    </Text>
                  ) : null}
                  {latencyMessage ? (
                    <div>
                      <Text type={latencyOk ? "success" : "danger"} style={{ fontSize: 12 }}>
                        {latencyMessage}
                      </Text>
                    </div>
                  ) : null}
                </div>
                <Space size={6}>
                  <Button
                    size="small"
                    autoInsertSpace={false}
                    loading={probingId === row.id}
                    onClick={() => probeExitRow(row)}
                  >
                    {t("antigravity.exitProbe", { defaultValue: "检测出口 IP" })}
                  </Button>
                  <Button
                    size="small"
                    autoInsertSpace={false}
                    loading={latencyId === row.id}
                    onClick={() => latencyExitRow(row)}
                  >
                    {t("antigravity.exitLatency", { defaultValue: "延迟测试" })}
                  </Button>
                  <Button size="small" autoInsertSpace={false} onClick={() => setExitEditor({ ...row })}>
                    {t("antigravity.exitEdit", { defaultValue: "编辑" })}
                  </Button>
                  <Button
                    size="small"
                    autoInsertSpace={false}
                    danger
                    disabled={isSavingExit}
                    onClick={() => persistExitRows(exitRows.filter((item) => item.id !== row.id))}
                  >
                    {t("antigravity.exitDelete", { defaultValue: "删除" })}
                  </Button>
                </Space>
              </div>
            );
          })}
          <Button onClick={() => setExitEditor(newExitRow())}>
            {t("antigravity.exitAdd", { defaultValue: "添加链式代理出口IP" })}
          </Button>
          <Modal
            title={
              exitEditor && exitRows.some((row) => row.id === exitEditor.id)
                ? t("antigravity.exitEditTitle", { defaultValue: "编辑链式代理出口IP" })
                : t("antigravity.exitAdd", { defaultValue: "添加链式代理出口IP" })
            }
            open={exitEditor !== null}
            confirmLoading={isSavingExit}
            okText={t("antigravity.exitSave", { defaultValue: "保存链式代理出口IP" })}
            onCancel={() => setExitEditor(null)}
            onOk={saveExitEditor}
            destroyOnClose
          >
            {exitEditor ? (
              <div
                style={{
                  display: "grid",
                  gridTemplateColumns: "72px 1fr",
                  gap: 12,
                  alignItems: "center",
                  paddingTop: 8,
                }}
              >
                <Text>{t("antigravity.exitName", { defaultValue: "名称" })}</Text>
                <Input
                  value={exitEditor.name}
                  onChange={(event) =>
                    setExitEditor({ ...exitEditor, name: event.target.value })
                  }
                />
                <Text>{t("antigravity.exitScheme", { defaultValue: "协议" })}</Text>
                <Select<ExitProxyScheme>
                  value={exitEditor.scheme}
                  onChange={(value) => setExitEditor({ ...exitEditor, scheme: value })}
                  options={[
                    { value: "socks5", label: "SOCKS5" },
                    { value: "http", label: "HTTP" },
                  ]}
                />
                <Text>{t("antigravity.exitHost", { defaultValue: "主机" })}</Text>
                <Input
                  value={exitEditor.host}
                  placeholder="gw.example.com"
                  onChange={(event) =>
                    setExitEditor({ ...exitEditor, host: event.target.value })
                  }
                />
                <Text>{t("antigravity.exitPort", { defaultValue: "端口" })}</Text>
                <Input
                  value={exitEditor.port}
                  placeholder="1080"
                  onChange={(event) =>
                    setExitEditor({
                      ...exitEditor,
                      port: event.target.value.replace(/\D/g, ""),
                    })
                  }
                />
                <Text>{t("antigravity.exitUser", { defaultValue: "账号" })}</Text>
                <Input
                  value={exitEditor.username}
                  placeholder={t("antigravity.exitUserOptional", { defaultValue: "可空" })}
                  onChange={(event) =>
                    setExitEditor({ ...exitEditor, username: event.target.value })
                  }
                />
                <Text>{t("antigravity.exitPass", { defaultValue: "密码" })}</Text>
                <Input.Password
                  value={exitEditor.password}
                  onChange={(event) =>
                    setExitEditor({ ...exitEditor, password: event.target.value })
                  }
                />
              </div>
            ) : null}
          </Modal>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {status?.exitProxies?.some((item) => item.enabled) && status.exitChainLabel
              ? t("antigravity.exitChain", {
                  defaultValue: "链路：{{value}}",
                  value: status.exitChainLabel,
                })
              : t("antigravity.exitOff", { defaultValue: "未启用（只走出站代理）" })}
          </Text>
          {status?.exitError ? (
            <Text type="danger" style={{ fontSize: 12 }}>
              {status.exitError}
            </Text>
          ) : null}
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Text strong style={{ fontSize: 13 }}>
            {t("antigravity.limiterSection", { defaultValue: "并发与限速" })}
          </Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.limiterHint", {
              defaultValue:
                "按账号限制并发与请求速率，减轻 Cloud Code 429。保存后立即生效（进行中的流式请求不受影响）。",
            })}
          </Text>
          <Space wrap>
            <InputNumber
              min={1}
              max={16}
              value={limiter.accountConcurrency}
              onChange={(value) =>
                patchLimiter({ accountConcurrency: typeof value === "number" ? value : 4 })
              }
              addonBefore={t("antigravity.limiterAccountConcurrency", { defaultValue: "账号并发" })}
            />
            <InputNumber
              min={1}
              max={8}
              value={limiter.subagentConcurrency}
              onChange={(value) =>
                patchLimiter({ subagentConcurrency: typeof value === "number" ? value : 2 })
              }
              addonBefore={t("antigravity.limiterSubagentConcurrency", { defaultValue: "子代理并发" })}
            />
            <InputNumber
              min={0}
              max={5000}
              step={50}
              value={limiter.minRequestIntervalMs}
              onChange={(value) =>
                patchLimiter({ minRequestIntervalMs: typeof value === "number" ? value : 300 })
              }
              addonBefore={t("antigravity.limiterMinInterval", { defaultValue: "最小间隔 ms" })}
            />
            <InputNumber
              min={0}
              max={120}
              value={limiter.ratePerMin}
              onChange={(value) =>
                patchLimiter({ ratePerMin: typeof value === "number" ? value : 30 })
              }
              addonBefore={t("antigravity.limiterRatePerMin", { defaultValue: "RPM 上限" })}
            />
            <InputNumber
              min={1}
              max={32}
              value={limiter.tokenBurst}
              onChange={(value) =>
                patchLimiter({ tokenBurst: typeof value === "number" ? value : 8 })
              }
              addonBefore={t("antigravity.limiterTokenBurst", { defaultValue: "突发令牌" })}
            />
            <InputNumber
              min={1}
              max={120}
              value={limiter.acquireTimeoutSecs}
              onChange={(value) =>
                patchLimiter({ acquireTimeoutSecs: typeof value === "number" ? value : 8 })
              }
              addonBefore={t("antigravity.limiterAcquireTimeout", { defaultValue: "等待超时 s" })}
            />
            <Button loading={isSavingLimiter} onClick={() => onSaveLimiter(limiter)}>
              {t("antigravity.limiterSave", { defaultValue: "保存并发/限速" })}
            </Button>
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.limiterRateOffHint", {
              defaultValue: "RPM 上限设为 0 可关闭令牌桶（仍保留并发闸门与 429 退避）。",
            })}
          </Text>
          <Divider style={{ margin: "8px 0" }} />
          <Text strong style={{ fontSize: 13 }}>
            {t("antigravity.fastPathTitle", { defaultValue: "后台请求短路" })}
          </Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("antigravity.fastPathHint", {
              defaultValue:
                "Claude Code 会额外发一些短请求（探测额度、生成会话标题、扫命令前缀等）。勾选后由本机直接回复，不占用 Cloud Code 额度；关掉则照常发给上游。",
            })}
          </Text>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))",
              gap: "10px 20px",
              width: "100%",
            }}
          >
            {FAST_PATH_TOGGLES.map((item) => (
              <Checkbox
                key={item.key}
                checked={fastPath[item.key]}
                onChange={(event) =>
                  patchFastPath({ [item.key]: event.target.checked })
                }
                style={{ alignItems: "flex-start", marginInlineStart: 0 }}
              >
                <div>
                  <div>{t(item.labelKey, { defaultValue: item.labelDefault })}</div>
                  <Text type="secondary" style={{ fontSize: 12, whiteSpace: "normal" }}>
                    {t(item.hintKey, { defaultValue: item.hintDefault })}
                  </Text>
                </div>
              </Checkbox>
            ))}
          </div>
          <Button
            loading={isSavingFastPath}
            onClick={() => onSaveFastPath(fastPath)}
            style={{ alignSelf: "flex-start" }}
          >
            {t("antigravity.fastPathSave", { defaultValue: "保存短路设置" })}
          </Button>
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space wrap style={{ marginTop: 4 }}>
          <Button
            type="primary"
            icon={<PlayCircleOutlined />}
            loading={isStarting}
            onClick={() => onStartGateway(port, apiKey, outboundMode, outboundUrl)}
          >
            {t("antigravity.start")}
          </Button>
          <Button
            icon={<StopOutlined />}
            loading={isStopping}
            onClick={() => onStopGateway()}
          >
            {t("antigravity.stop")}
          </Button>
          <Button icon={<ReloadOutlined />} onClick={onRefresh}>
            {t("common.refresh")}
          </Button>
          <Button
            icon={<CopyOutlined />}
            onClick={() => {
              void navigator.clipboard.writeText(curlSnippet).then(() => {
                message.success(t("antigravity.copied"));
              });
            }}
          >
            {t("antigravity.copyCurl")}
          </Button>
        </Space>

        {curlVisible ? (
          <Paragraph style={{ marginBottom: 0 }}>
            <pre style={{ margin: 0, padding: 8, borderRadius: 6, background: "var(--ant-color-bg-layout, #f5f5f5)", whiteSpace: "pre-wrap", fontSize: 12 }}>
              {curlSnippet}
            </pre>
            <Button type="link" size="small" style={{ padding: 0, marginTop: 4 }} onClick={() => setCurlVisible(false)}>
              {t("antigravity.hideTestCommand", { defaultValue: "收起测试命令" })}
            </Button>
          </Paragraph>
        ) : (
          <Button type="link" size="small" style={{ padding: 0 }} onClick={() => setCurlVisible(true)}>
            {t("antigravity.viewTestCommand", { defaultValue: "查看测试命令" })}
          </Button>
        )}
      </Space>
    </Card>
  );
}
