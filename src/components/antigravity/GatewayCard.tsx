import { useMemo, useState } from "react";
import {
  Button,
  Card,
  Checkbox,
  Divider,
  Input,
  InputNumber,
  Select,
  Space,
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
} from "@/services/api";

const { Text, Paragraph } = Typography;

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
  onSaveLimiter: (settings: AntigravityLimiterSettings) => Promise<void>;
  onSaveFastPath: (settings: AntigravityFastPathSettings) => Promise<void>;
  onRefresh: () => void;
  isStarting?: boolean;
  isStopping?: boolean;
  isSavingOutbound?: boolean;
  isSavingLimiter?: boolean;
  isSavingFastPath?: boolean;
}

export function GatewayCard({
  status,
  models,
  onStartGateway,
  onStopGateway,
  onSaveOutbound,
  onSaveLimiter,
  onSaveFastPath,
  onRefresh,
  isStarting = false,
  isStopping = false,
  isSavingOutbound = false,
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
