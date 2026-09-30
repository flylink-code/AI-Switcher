import { useMemo, useState } from "react";
import { Button, Card, Divider, Input, InputNumber, Modal, Select, Space, Switch, Tag, Typography, message } from "antd";
import PlayCircleOutlined from "@ant-design/icons/es/icons/PlayCircleOutlined";
import StopOutlined from "@ant-design/icons/es/icons/StopOutlined";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import { useTranslation } from "react-i18next";
import type {
  KiroExitLatencyResult,
  KiroExitProbeResult,
  KiroExitProxy,
  KiroExitProxyInput,
  KiroGatewayStatus,
  KiroOutboundMode,
} from "@/services/kiro";

const { Text } = Typography;

interface GatewayCardProps {
  status?: KiroGatewayStatus;
  onStartGateway: (port: number, apiKey: string, outboundMode: KiroOutboundMode, outboundUrl: string) => Promise<void>;
  onStopGateway: () => Promise<void>;
  onSaveOutbound: (mode: KiroOutboundMode, url: string) => Promise<void>;
  onSaveExit: (entries: KiroExitProxyInput[]) => Promise<void>;
  onProbeExit: (id: string, url: string) => Promise<KiroExitProbeResult>;
  onProbeLatency: (id: string, url: string) => Promise<KiroExitLatencyResult>;
  onRefresh: () => void;
  isStarting?: boolean;
  isStopping?: boolean;
  isSavingOutbound?: boolean;
  isSavingExit?: boolean;
}

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

const EXIT_NAME = "链式代理出口IP";

interface ExitRow extends ExitProxyFields {
  id: string;
  name: string;
  enabled: boolean;
}

function isOutboundMode(value: string | undefined): value is KiroOutboundMode {
  return value === "direct" || value === "system" || value === "custom";
}

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

function newExitId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `exit-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function newExitRow(): ExitRow {
  return { id: newExitId(), name: EXIT_NAME, enabled: false, ...EMPTY_EXIT_PROXY };
}

function rowFromSaved(entry: KiroExitProxy): ExitRow {
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

function rowsToExitInputs(rows: ExitRow[]): KiroExitProxyInput[] {
  return rows.filter(exitFieldsOk).map((row) => ({
    id: row.id,
    name: row.name.trim() || EXIT_NAME,
    enabled: row.enabled,
    proxyUrl: composeExitProxyUrl({ ...row, port: String(Number(row.port)) }),
  }));
}

export function GatewayCard({
  status,
  onStartGateway,
  onStopGateway,
  onSaveOutbound,
  onSaveExit,
  onProbeExit,
  onProbeLatency,
  onRefresh,
  isStarting = false,
  isStopping = false,
  isSavingOutbound = false,
  isSavingExit = false,
}: GatewayCardProps) {
  const { t } = useTranslation();
  const [portDraft, setPortDraft] = useState<number | null>(null);
  const [apiKeyDraft, setApiKeyDraft] = useState<string | null>(null);
  const [outboundModeDraft, setOutboundModeDraft] = useState<KiroOutboundMode | null>(null);
  const [outboundUrlDraft, setOutboundUrlDraft] = useState<string | null>(null);
  const [exitPending, setExitPending] = useState<ExitRow[] | null>(null);
  const [exitEditor, setExitEditor] = useState<ExitRow | null>(null);
  const [probingId, setProbingId] = useState<string | null>(null);
  const [latencyId, setLatencyId] = useState<string | null>(null);
  const [probeById, setProbeById] = useState<Record<string, KiroExitProbeResult>>({});
  const [latencyById, setLatencyById] = useState<Record<string, KiroExitLatencyResult>>({});

  const port = portDraft ?? status?.port ?? 15831;
  const apiKey = apiKeyDraft ?? status?.apiKey ?? "";
  const outboundMode = outboundModeDraft ?? (isOutboundMode(status?.outboundMode) ? status.outboundMode : "system");
  const outboundUrl = outboundUrlDraft ?? status?.outboundProxyUrl ?? "";
  const savedExitRows = useMemo(() => (status?.exitProxies ?? []).map(rowFromSaved), [status?.exitProxies]);
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
      message.warning(t("kiro.exitFieldsRequired"));
      return;
    }
    const url = composeExitProxyUrl({ ...row, port: String(Number(row.port)) });
    setProbingId(row.id);
    void onProbeExit(row.id, url)
      .then((result) => setProbeById((prev) => ({ ...prev, [row.id]: result })))
      .catch(() => undefined)
      .finally(() => setProbingId(null));
  };
  const latencyExitRow = (row: ExitRow) => {
    if (!exitFieldsOk(row)) {
      message.warning(t("kiro.exitFieldsRequired"));
      return;
    }
    const url = composeExitProxyUrl({ ...row, port: String(Number(row.port)) });
    setLatencyId(row.id);
    void onProbeLatency(row.id, url)
      .then((result) => setLatencyById((prev) => ({ ...prev, [row.id]: result })))
      .catch(() => undefined)
      .finally(() => setLatencyId(null));
  };
  const saveExitEditor = () => {
    if (!exitEditor) return;
    if (!exitFieldsOk(exitEditor)) {
      message.warning(t("kiro.exitFieldsRequired"));
      return;
    }
    const nextRow: ExitRow = {
      ...exitEditor,
      name: exitEditor.name.trim() || EXIT_NAME,
      port: String(Number(exitEditor.port)),
    };
    const exists = exitRows.some((row) => row.id === nextRow.id);
    const next = exists ? exitRows.map((row) => (row.id === nextRow.id ? nextRow : row)) : [...exitRows, nextRow];
    setExitEditor(null);
    persistExitRows(next);
  };

  return (
    <Card title={t("kiro.gateway")} size="small">
      <Space direction="vertical" style={{ width: "100%" }} size={12}>
        <Space wrap>
          <Tag color={status?.running ? "success" : "default"}>
            {status?.running ? t("kiro.running") : t("kiro.stoppedState")}
          </Tag>
          <Text type="secondary">{t("kiro.accountsCount", { count: status?.accountCount ?? 0 })}</Text>
          <Text code>{status?.baseUrl ?? `http://127.0.0.1:${port}`}</Text>
        </Space>

        <Space wrap>
          <InputNumber
            min={1024}
            max={65535}
            value={port}
            onChange={(value) => setPortDraft(typeof value === "number" ? value : null)}
            addonBefore={t("kiro.port")}
          />
          <Input.Password
            style={{ width: 280 }}
            value={apiKey}
            onChange={(event) => setApiKeyDraft(event.target.value)}
            placeholder="sk-..."
            addonBefore={t("kiro.apiKey")}
          />
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Text strong style={{ fontSize: 13 }}>{t("kiro.outbound")}</Text>
          <Space wrap>
            <Select<KiroOutboundMode>
              style={{ minWidth: 200 }}
              value={outboundMode}
              onChange={setOutboundModeDraft}
              options={[
                { value: "custom", label: t("kiro.outboundCustom") },
                { value: "direct", label: t("kiro.outboundDirect") },
                { value: "system", label: t("kiro.outboundSystem") },
              ]}
            />
            <Input
              style={{ width: 280 }}
              disabled={outboundMode !== "custom"}
              value={outboundUrl}
              onChange={(event) => setOutboundUrlDraft(event.target.value)}
              placeholder="socks5://127.0.0.1:17891"
              addonBefore={t("kiro.outboundProxy")}
            />
            <Button loading={isSavingOutbound} onClick={() => void onSaveOutbound(outboundMode, outboundUrl)}>
              {t("kiro.outboundSave")}
            </Button>
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {t("kiro.outboundEffective", {
              value: status?.effectiveOutboundProxy || t("kiro.outboundNone"),
            })}
          </Text>
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Text strong style={{ fontSize: 13 }}>{t("kiro.exitSection")}</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>{t("kiro.exitHint")}</Text>
          {exitRows.length === 0 ? (
            <Text type="secondary" style={{ fontSize: 12 }}>{t("kiro.exitEmpty")}</Text>
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
                <Switch checked={row.enabled} disabled={isSavingExit} onChange={(checked) => setExitEnabled(row.id, checked)} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Text strong style={{ fontSize: 13 }}>{row.name || EXIT_NAME}</Text>
                  <div>
                    <Text type="secondary" style={{ fontSize: 12 }}>{exitEndpointLabel(row)}</Text>
                  </div>
                  {probeMessage ? (
                    <Text type={probeOk ? "success" : "danger"} style={{ fontSize: 12 }}>{probeMessage}</Text>
                  ) : null}
                  {latencyMessage ? (
                    <div>
                      <Text type={latencyOk ? "success" : "danger"} style={{ fontSize: 12 }}>{latencyMessage}</Text>
                    </div>
                  ) : null}
                </div>
                <Space size={6}>
                  <Button size="small" loading={probingId === row.id} onClick={() => probeExitRow(row)}>
                    {t("kiro.exitProbe")}
                  </Button>
                  <Button size="small" loading={latencyId === row.id} onClick={() => latencyExitRow(row)}>
                    {t("kiro.exitLatency")}
                  </Button>
                  <Button size="small" onClick={() => setExitEditor({ ...row })}>{t("kiro.exitEdit")}</Button>
                  <Button
                    size="small"
                    danger
                    disabled={isSavingExit}
                    onClick={() => persistExitRows(exitRows.filter((item) => item.id !== row.id))}
                  >
                    {t("kiro.exitDelete")}
                  </Button>
                </Space>
              </div>
            );
          })}
          <Button onClick={() => setExitEditor(newExitRow())}>{t("kiro.exitAdd")}</Button>
          <Modal
            title={exitEditor && exitRows.some((row) => row.id === exitEditor.id) ? t("kiro.exitEditTitle") : t("kiro.exitAdd")}
            open={exitEditor !== null}
            confirmLoading={isSavingExit}
            okText={t("kiro.exitSave")}
            onCancel={() => setExitEditor(null)}
            onOk={saveExitEditor}
            destroyOnHidden
          >
            {exitEditor ? (
              <div style={{ display: "grid", gridTemplateColumns: "72px 1fr", gap: 12, alignItems: "center", paddingTop: 8 }}>
                <Text>{t("kiro.exitName")}</Text>
                <Input value={exitEditor.name} onChange={(event) => setExitEditor({ ...exitEditor, name: event.target.value })} />
                <Text>{t("kiro.exitScheme")}</Text>
                <Select<ExitProxyScheme>
                  value={exitEditor.scheme}
                  onChange={(value) => setExitEditor({ ...exitEditor, scheme: value })}
                  options={[{ value: "socks5", label: "SOCKS5" }, { value: "http", label: "HTTP" }]}
                />
                <Text>{t("kiro.exitHost")}</Text>
                <Input value={exitEditor.host} placeholder="gw.example.com" onChange={(event) => setExitEditor({ ...exitEditor, host: event.target.value })} />
                <Text>{t("kiro.exitPort")}</Text>
                <Input
                  value={exitEditor.port}
                  placeholder="1080"
                  onChange={(event) => setExitEditor({ ...exitEditor, port: event.target.value.replace(/\D/g, "") })}
                />
                <Text>{t("kiro.exitUser")}</Text>
                <Input
                  value={exitEditor.username}
                  placeholder={t("kiro.exitUserOptional")}
                  onChange={(event) => setExitEditor({ ...exitEditor, username: event.target.value })}
                />
                <Text>{t("kiro.exitPass")}</Text>
                <Input.Password value={exitEditor.password} onChange={(event) => setExitEditor({ ...exitEditor, password: event.target.value })} />
              </div>
            ) : null}
          </Modal>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {status?.exitProxies?.some((item) => item.enabled) && status.exitChainLabel
              ? t("kiro.exitChain", { value: status.exitChainLabel })
              : t("kiro.exitOff")}
          </Text>
          {status?.exitError ? <Text type="danger" style={{ fontSize: 12 }}>{status.exitError}</Text> : null}
        </Space>

        <Divider style={{ margin: "4px 0" }} />

        <Space wrap style={{ marginTop: 4 }}>
          <Button
            type="primary"
            icon={<PlayCircleOutlined />}
            loading={isStarting}
            onClick={() => void onStartGateway(port, apiKey, outboundMode, outboundUrl)}
          >
            {t("kiro.start")}
          </Button>
          <Button icon={<StopOutlined />} loading={isStopping} disabled={!status?.running} onClick={() => void onStopGateway()}>
            {t("kiro.stop")}
          </Button>
          <Button icon={<ReloadOutlined />} onClick={onRefresh}>{t("common.refresh")}</Button>
          <Button
            icon={<CopyOutlined />}
            onClick={() => {
              void navigator.clipboard.writeText(apiKey).then(() => {
                message.success(t("kiro.copied"));
              });
            }}
          >
            {t("kiro.copyKey")}
          </Button>
        </Space>
      </Space>
    </Card>
  );
}
