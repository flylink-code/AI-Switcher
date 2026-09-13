import { useState } from "react";
import {
  Button,
  Card,
  Input,
  InputNumber,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
} from "antd";
import ArrowDownOutlined from "@ant-design/icons/es/icons/ArrowDownOutlined";
import ArrowUpOutlined from "@ant-design/icons/es/icons/ArrowUpOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { deleteRouteRule, upsertRouteRule } from "@/services/providers";
import type { RouteRule } from "@/types/backend";

const { Text } = Typography;

const LEFT_VALUES = [
  "token_count",
  "thinking",
  "web_search",
  "vision",
  "tool",
  "path",
  "target_app",
] as const;

type LeftValue = (typeof LEFT_VALUES)[number];

const OPERATORS_BY_LEFT: Record<LeftValue, string[]> = {
  token_count: [">", ">=", "<", "<=", "=="],
  thinking: ["==", "!="],
  web_search: ["==", "!="],
  vision: ["==", "!="],
  tool: ["contains", "=="],
  path: ["starts-with", "contains", "=="],
  target_app: ["==", "!="],
};

type ConditionShape = { left: string; operator: string; right: string | number | boolean };

type RewriteRow = { path: string; op: string; value?: string };

function isProtectedHeader(name: string): boolean {
  const lower = name.trim().toLowerCase();
  return (
    ["authorization", "api-key", "x-api-key", "cookie", "host", "content-length", "connection"].includes(lower)
    || lower.startsWith("x-auth-")
    || lower.startsWith("x-aisw-")
  );
}

function parseCondition(raw: string): { value: ConditionShape; advanced: boolean } {
  try {
    const parsed = JSON.parse(raw || "{}") as Record<string, unknown>;
    const left = typeof parsed.left === "string" ? parsed.left : "token_count";
    const operator = typeof parsed.operator === "string" ? parsed.operator : ">=";
    const right = parsed.right as string | number | boolean | undefined;
    const known = LEFT_VALUES.includes(left as LeftValue);
    const extra = Object.keys(parsed).some((key) => key !== "left" && key !== "operator" && key !== "right");
    return {
      value: {
        left: known ? left : "token_count",
        operator,
        right: right ?? "",
      },
      advanced: extra || !known,
    };
  } catch {
    return { value: { left: "token_count", operator: ">=", right: 60000 }, advanced: true };
  }
}

function parseRewrites(raw: string): RewriteRow[] {
  try {
    const parsed = JSON.parse(raw || "[]") as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.map((item) => {
      const row = (item ?? {}) as Record<string, unknown>;
      return {
        path: typeof row.path === "string" ? row.path : "request.headers.",
        op: typeof row.op === "string" ? row.op : "set",
        value: typeof row.value === "string" ? row.value : row.value != null ? JSON.stringify(row.value) : "",
      };
    });
  } catch {
    return [];
  }
}

function thinkingEffort(json: string): string {
  try {
    const parsed = JSON.parse(json || "{}") as { reasoningEffort?: string; mode?: string };
    return parsed.mode === "disabled" ? "off" : (parsed.reasoningEffort ?? "off");
  } catch {
    return "off";
  }
}

function thinkingJson(effort: string): string {
  return JSON.stringify(
    effort === "off" ? { mode: "disabled" } : { mode: "effort", reasoningEffort: effort },
  );
}

export function RouteRulesCard({
  rules,
  loading,
  modelOptions,
  profileId,
}: {
  rules: RouteRule[];
  loading: boolean;
  modelOptions: Array<{ value: string; label: string }>;
  profileId: string;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [advancedIds, setAdvancedIds] = useState<Record<string, boolean>>({});
  const [rewriteOpen, setRewriteOpen] = useState<Record<string, boolean>>({});

  const persist = async (rule: RouteRule) => {
    await upsertRouteRule(rule);
    await queryClient.invalidateQueries({ queryKey: ["route-rules", profileId] });
  };

  const move = async (row: RouteRule, direction: -1 | 1) => {
    const ordered = [...rules].sort((a, b) => a.sortIndex - b.sortIndex);
    const index = ordered.findIndex((item) => item.id === row.id);
    const swap = ordered[index + direction];
    if (!swap) return;
    await persist({ ...row, sortIndex: swap.sortIndex });
    await persist({ ...swap, sortIndex: row.sortIndex });
  };

  return (
    <Card
      size="small"
      title={t("gateway.routeRules", { defaultValue: "条件规则" })}
      extra={
        <Button
          size="small"
          icon={<PlusOutlined />}
          onClick={() => {
            const rule: RouteRule = {
              id: `rule_${crypto.randomUUID().replace(/-/g, "").slice(0, 12)}`,
              profileId,
              enabled: true,
              sortIndex: rules.length,
              ruleType: "model-prefix",
              conditionJson: JSON.stringify({ left: "token_count", operator: ">=", right: 60000 }),
              pattern: "",
              targetModel: "",
              thinkingConfigJson: "{}",
              rewritesJson: "[]",
            };
            void persist(rule);
          }}
        >
          {t("gateway.addRule", { defaultValue: "添加规则" })}
        </Button>
      }
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12 }}>
        {t("gateway.rulesHelpIntro", {
          defaultValue:
            "规则优先于模式、排在显式模型之后。model-prefix 按客户端模型名前缀匹配；condition 用 token/工具/是否含图等左值。不做 JS 脚本。",
        })}
      </Text>
      <Table
        size="small"
        rowKey="id"
        pagination={false}
        loading={loading}
        dataSource={[...rules].sort((a, b) => a.sortIndex - b.sortIndex)}
        columns={[
          {
            title: "#",
            width: 72,
            render: (_: unknown, row: RouteRule, index: number) => (
              <Space size={0}>
                <Button
                  type="text"
                  size="small"
                  icon={<ArrowUpOutlined />}
                  disabled={index === 0}
                  onClick={() => void move(row, -1)}
                />
                <Button
                  type="text"
                  size="small"
                  icon={<ArrowDownOutlined />}
                  disabled={index === rules.length - 1}
                  onClick={() => void move(row, 1)}
                />
              </Space>
            ),
          },
          {
            title: t("gateway.ruleType", { defaultValue: "类型" }),
            dataIndex: "ruleType",
            render: (value: string, row: RouteRule) => (
              <Select
                value={value}
                options={[
                  { value: "model-prefix", label: "model-prefix" },
                  { value: "condition", label: "condition" },
                ]}
                onChange={(ruleType) => void persist({ ...row, ruleType })}
              />
            ),
          },
          {
            title: t("gateway.pattern", { defaultValue: "匹配" }),
            render: (_: unknown, row: RouteRule) =>
              row.ruleType === "condition" ? (
                <ConditionEditor
                  row={row}
                  advanced={advancedIds[row.id] ?? parseCondition(row.conditionJson).advanced}
                  onAdvanced={(next) => setAdvancedIds((current) => ({ ...current, [row.id]: next }))}
                  onPersist={persist}
                />
              ) : (
                <Input
                  defaultValue={row.pattern}
                  onBlur={(event) => {
                    const value = event.target.value;
                    if (value !== row.pattern) void persist({ ...row, pattern: value });
                  }}
                />
              ),
          },
          {
            title: t("gateway.targetModel", { defaultValue: "目标模型" }),
            render: (_: unknown, row: RouteRule) => (
              <Select
                showSearch
                allowClear
                style={{ minWidth: 180 }}
                value={row.targetModel || undefined}
                options={modelOptions}
                onChange={(value) => void persist({ ...row, targetModel: value ?? "" })}
              />
            ),
          },
          {
            title: t("gateway.thinking", { defaultValue: "挡位" }),
            render: (_: unknown, row: RouteRule) => (
              <Select
                style={{ width: 100 }}
                value={thinkingEffort(row.thinkingConfigJson)}
                options={[
                  { value: "off", label: t("gateway.thinkingOff", { defaultValue: "关闭" }) },
                  { value: "low", label: t("gateway.thinkingLow", { defaultValue: "低" }) },
                  { value: "medium", label: t("gateway.thinkingMedium", { defaultValue: "中" }) },
                  { value: "high", label: t("gateway.thinkingHigh", { defaultValue: "高" }) },
                ]}
                onChange={(value) => void persist({ ...row, thinkingConfigJson: thinkingJson(value) })}
              />
            ),
          },
          {
            title: t("gateway.rewrites", { defaultValue: "改写" }),
            render: (_: unknown, row: RouteRule) => (
              <Button
                size="small"
                type="link"
                onClick={() => setRewriteOpen((current) => ({ ...current, [row.id]: !current[row.id] }))}
              >
                {t("gateway.editRewrites", { defaultValue: "编辑" })}
                {parseRewrites(row.rewritesJson).length > 0 ? (
                  <Tag style={{ marginLeft: 4 }}>{parseRewrites(row.rewritesJson).length}</Tag>
                ) : null}
              </Button>
            ),
          },
          {
            title: t("gateway.enabled", { defaultValue: "启用" }),
            render: (_: unknown, row: RouteRule) => (
              <Switch checked={row.enabled} onChange={(enabled) => void persist({ ...row, enabled })} />
            ),
          },
          {
            title: "",
            render: (_: unknown, row: RouteRule) => (
              <Button
                size="small"
                danger
                onClick={() => {
                  void deleteRouteRule(row.id).then(() => {
                    void queryClient.invalidateQueries({ queryKey: ["route-rules", profileId] });
                  });
                }}
              >
                {t("common.delete", { defaultValue: "删除" })}
              </Button>
            ),
          },
        ]}
        expandable={{
          expandedRowKeys: Object.keys(rewriteOpen).filter((id) => rewriteOpen[id]),
          showExpandColumn: false,
          expandedRowRender: (row: RouteRule) => (
            <RewriteEditor row={row} onPersist={persist} />
          ),
        }}
      />
    </Card>
  );
}

function ConditionEditor({
  row,
  advanced,
  onAdvanced,
  onPersist,
}: {
  row: RouteRule;
  advanced: boolean;
  onAdvanced: (next: boolean) => void;
  onPersist: (rule: RouteRule) => Promise<void>;
}) {
  const { t } = useTranslation();
  const parsed = parseCondition(row.conditionJson);
  const left = (LEFT_VALUES.includes(parsed.value.left as LeftValue)
    ? parsed.value.left
    : "token_count") as LeftValue;
  const operators = OPERATORS_BY_LEFT[left];
  const operator = operators.includes(parsed.value.operator) ? parsed.value.operator : operators[0];

  if (advanced) {
    return (
      <Space direction="vertical" size={4} style={{ width: "100%" }}>
        <Input
          defaultValue={row.conditionJson}
          onBlur={(event) => {
            const value = event.target.value;
            if (value !== row.conditionJson) void onPersist({ ...row, conditionJson: value });
          }}
        />
        <Button type="link" size="small" style={{ padding: 0 }} onClick={() => onAdvanced(false)}>
          {t("gateway.conditionBuilder", { defaultValue: "结构化编辑" })}
        </Button>
      </Space>
    );
  }

  const save = (next: ConditionShape) => {
    void onPersist({ ...row, conditionJson: JSON.stringify(next) });
  };

  return (
    <Space wrap size={4}>
      <Select
        style={{ width: 130 }}
        value={left}
        options={LEFT_VALUES.map((item) => ({ value: item, label: item }))}
        onChange={(value) => {
          const nextOps = OPERATORS_BY_LEFT[value];
          save({
            left: value,
            operator: nextOps[0],
            right: value === "token_count" ? 60000 : value === "thinking" || value === "web_search" || value === "vision" ? true : "",
          });
        }}
      />
      <Select
        style={{ width: 110 }}
        value={operator}
        options={operators.map((item) => ({ value: item, label: item }))}
        onChange={(value) => save({ ...parsed.value, left, operator: value })}
      />
      {left === "token_count" ? (
        <InputNumber
          value={Number(parsed.value.right) || 0}
          onChange={(value) => save({ left, operator, right: value ?? 0 })}
        />
      ) : left === "thinking" || left === "web_search" || left === "vision" ? (
        <Select
          style={{ width: 90 }}
          value={String(parsed.value.right) === "false" ? "false" : "true"}
          options={[
            { value: "true", label: "true" },
            { value: "false", label: "false" },
          ]}
          onChange={(value) => save({ left, operator, right: value === "true" })}
        />
      ) : (
        <Input
          style={{ width: 140 }}
          defaultValue={String(parsed.value.right ?? "")}
          onBlur={(event) => save({ left, operator, right: event.target.value })}
        />
      )}
      <Button type="link" size="small" style={{ padding: 0 }} onClick={() => onAdvanced(true)}>
        {t("gateway.conditionJson", { defaultValue: "原始 JSON" })}
      </Button>
    </Space>
  );
}

function RewriteEditor({
  row,
  onPersist,
}: {
  row: RouteRule;
  onPersist: (rule: RouteRule) => Promise<void>;
}) {
  const { t } = useTranslation();
  const rows = parseRewrites(row.rewritesJson);
  const save = (next: RewriteRow[]) => {
    const payload = next.map((item) => {
      const entry: Record<string, unknown> = { path: item.path, op: item.op };
      if (item.op !== "delete") entry.value = item.value ?? "";
      return entry;
    });
    void onPersist({ ...row, rewritesJson: JSON.stringify(payload) });
  };
  return (
    <Space direction="vertical" size={8} style={{ width: "100%" }}>
      <Text type="secondary">
        {t("gateway.rewritesHint", {
          defaultValue: "仅 request.headers.* / request.body.* 的 set/delete。鉴权头会被拦截。",
        })}
      </Text>
      {rows.map((item, index) => {
        const headerName = item.path.replace(/^request\.headers\./i, "");
        const blocked = item.path.toLowerCase().startsWith("request.headers.") && isProtectedHeader(headerName);
        return (
          <Space key={`${item.path}-${index}`} wrap>
            <Select
              style={{ width: 170 }}
              value={item.path.startsWith("request.body.") ? "request.body." : "request.headers."}
              options={[
                { value: "request.headers.", label: "request.headers.*" },
                { value: "request.body.", label: "request.body.*" },
              ]}
              onChange={(prefix) => {
                const next = [...rows];
                const rest = item.path.replace(/^request\.(headers|body)\./, "");
                next[index] = { ...item, path: `${prefix}${rest}` };
                save(next);
              }}
            />
            <Input
              style={{ width: 140 }}
              defaultValue={item.path.replace(/^request\.(headers|body)\./, "")}
              onBlur={(event) => {
                const prefix = item.path.startsWith("request.body.") ? "request.body." : "request.headers.";
                const next = [...rows];
                next[index] = { ...item, path: `${prefix}${event.target.value.trim()}` };
                save(next);
              }}
            />
            <Select
              style={{ width: 90 }}
              value={item.op}
              options={[
                { value: "set", label: "set" },
                { value: "delete", label: "delete" },
              ]}
              onChange={(op) => {
                const next = [...rows];
                next[index] = { ...item, op };
                save(next);
              }}
            />
            {item.op !== "delete" ? (
              <Input
                style={{ width: 140 }}
                defaultValue={item.value}
                onBlur={(event) => {
                  const next = [...rows];
                  next[index] = { ...item, value: event.target.value };
                  save(next);
                }}
              />
            ) : null}
            {blocked ? <Tag color="warning">{t("gateway.rewriteProtected", { defaultValue: "受保护" })}</Tag> : null}
            <Button
              size="small"
              danger
              onClick={() => {
                const next = rows.filter((_, i) => i !== index);
                save(next);
              }}
            >
              {t("common.delete", { defaultValue: "删除" })}
            </Button>
          </Space>
        );
      })}
      <Button
        size="small"
        onClick={() => save([...rows, { path: "request.headers.x-target-provider", op: "set", value: "" }])}
      >
        {t("gateway.addRewrite", { defaultValue: "添加改写" })}
      </Button>
    </Space>
  );
}
