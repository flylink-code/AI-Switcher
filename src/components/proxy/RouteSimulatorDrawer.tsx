import { useMemo, useState } from "react";
import {
  Button,
  Checkbox,
  Descriptions,
  Drawer,
  Input,
  InputNumber,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import { useTranslation } from "react-i18next";
import { simulateGatewayRoute } from "@/services/providers";
import type {
  ProviderTarget,
  SimulateGatewayRouteResult,
} from "@/types/backend";

const { Text } = Typography;
const { TextArea } = Input;

const TARGETS: ProviderTarget[] = [
  "claude_code",
  "claude_desktop",
  "codex",
  "opencode",
  "pi",
  "dsh",
  "cline",
];

export function RouteSimulatorDrawer({
  open,
  onClose,
  modelOptions,
  profileId,
}: {
  open: boolean;
  onClose: () => void;
  modelOptions: Array<{ value: string; label: string }>;
  profileId?: string | null;
}) {
  const { t } = useTranslation();
  const [requestedModel, setRequestedModel] = useState("auto");
  const [target, setTarget] = useState<ProviderTarget>("claude_code");
  const [tokenCount, setTokenCount] = useState<number | null>(null);
  const [hasThinking, setHasThinking] = useState(false);
  const [hasVision, setHasVision] = useState(false);
  const [hasWebSearch, setHasWebSearch] = useState(false);
  const [isSubagent, setIsSubagent] = useState(false);
  const [isImageGen, setIsImageGen] = useState(false);
  const [toolNames, setToolNames] = useState("");
  const [bodyJson, setBodyJson] = useState("");
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<SimulateGatewayRouteResult | null>(null);

  const run = async () => {
    setRunning(true);
    try {
      const next = await simulateGatewayRoute({
        requestedModel,
        target,
        tokenCount,
        hasThinking,
        hasVision,
        hasWebSearch,
        isSubagent,
        isImageGen,
        toolNames: toolNames
          .split(/[,，\s]+/)
          .map((item) => item.trim())
          .filter(Boolean),
        bodyJson: bodyJson.trim() || null,
        path: isImageGen ? "/v1/images/generations" : "/v1/messages",
        profileId: profileId ?? null,
      });
      setResult(next);
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setRunning(false);
    }
  };

  const catalogHint = useMemo(
    () =>
      t("gateway.simulateHint", {
        defaultValue: "只做路由决策，不发上游请求。粘贴真实请求体可覆盖勾选信号并显示实时估算 token。",
      }),
    [t],
  );

  return (
    <Drawer
      title={t("gateway.simulateTitle", { defaultValue: "路由试跑" })}
      open={open}
      onClose={onClose}
      width={560}
      extra={
        <Button type="primary" loading={running} onClick={() => void run()}>
          {t("gateway.simulateRun", { defaultValue: "试跑" })}
        </Button>
      }
    >
      <Space direction="vertical" size={12} style={{ width: "100%" }}>
        <Text type="secondary">{catalogHint}</Text>
        <Space wrap>
          <Select
            showSearch
            style={{ minWidth: 220 }}
            value={requestedModel}
            options={[{ value: "auto", label: "auto" }, ...modelOptions]}
            onChange={setRequestedModel}
          />
          <Select
            style={{ minWidth: 160 }}
            value={target}
            options={TARGETS.map((item) => ({
              value: item,
              label: t(`workspace.${item}`, { defaultValue: item }),
            }))}
            onChange={(value) => setTarget(value)}
          />
          <InputNumber
            min={0}
            placeholder={t("gateway.simulateTokens", { defaultValue: "估算 token" })}
            value={tokenCount ?? undefined}
            onChange={(value) => setTokenCount(value)}
          />
        </Space>
        <Space wrap>
          <Checkbox checked={hasThinking} onChange={(event) => setHasThinking(event.target.checked)}>
            {t("gateway.simulateThinking", { defaultValue: "含思考" })}
          </Checkbox>
          <Checkbox checked={hasVision} onChange={(event) => setHasVision(event.target.checked)}>
            {t("gateway.simulateVision", { defaultValue: "含图" })}
          </Checkbox>
          <Checkbox checked={hasWebSearch} onChange={(event) => setHasWebSearch(event.target.checked)}>
            {t("gateway.simulateSearch", { defaultValue: "联网" })}
          </Checkbox>
          <Checkbox checked={isSubagent} onChange={(event) => setIsSubagent(event.target.checked)}>
            {t("gateway.simulateSubagent", { defaultValue: "子代理" })}
          </Checkbox>
          <Checkbox checked={isImageGen} onChange={(event) => setIsImageGen(event.target.checked)}>
            {t("gateway.simulateImage", { defaultValue: "图像生成" })}
          </Checkbox>
        </Space>
        <Input
          placeholder={t("gateway.simulateTools", { defaultValue: "工具名，逗号分隔" })}
          value={toolNames}
          onChange={(event) => setToolNames(event.target.value)}
        />
        <TextArea
          rows={6}
          placeholder={t("gateway.simulateBody", { defaultValue: "可选：粘贴请求 JSON" })}
          value={bodyJson}
          onChange={(event) => setBodyJson(event.target.value)}
        />
        {result ? (
          <>
            <Descriptions size="small" column={1} bordered>
              <Descriptions.Item label={t("gateway.simulateEstimated", { defaultValue: "估算 token" })}>
                {result.estimatedTokens}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.model", { defaultValue: "模型" })}>
                {result.upstreamModel ?? result.decision?.normalizedModel ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.upstream", { defaultValue: "上游" })}>
                {result.providerName ?? result.providerId ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.reason", { defaultValue: "依据" })}>
                {result.decision?.reason ?? "—"}
              </Descriptions.Item>
              <Descriptions.Item label={t("gateway.modeName", { defaultValue: "模式" })}>
                {result.decision?.modeId ?? "—"}
              </Descriptions.Item>
            </Descriptions>
            <Table
              size="small"
              rowKey={(row) => `${row.stage}-${row.id}`}
              pagination={false}
              dataSource={result.steps}
              columns={[
                {
                  title: t("gateway.simulateStage", { defaultValue: "阶段" }),
                  dataIndex: "stage",
                  width: 80,
                },
                { title: "ID", dataIndex: "id", ellipsis: true },
                {
                  title: t("gateway.simulateHit", { defaultValue: "命中" }),
                  dataIndex: "matched",
                  width: 72,
                  render: (matched: boolean) =>
                    matched ? (
                      <Tag color="green">{t("gateway.simulateYes", { defaultValue: "是" })}</Tag>
                    ) : (
                      <Tag>{t("gateway.simulateNo", { defaultValue: "否" })}</Tag>
                    ),
                },
                { title: t("gateway.simulateDetail", { defaultValue: "说明" }), dataIndex: "detail" },
              ]}
            />
          </>
        ) : null}
      </Space>
    </Drawer>
  );
}
