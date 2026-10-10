import { useEffect, useState } from "react";
import { Button, Card, Input, InputNumber, Select, Space, Switch, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { SettingsRow } from "@/components/settings";
import {
  catalogModelSelectProps,
  type CatalogModelSelectOption,
} from "./CatalogModelOptionContent";
import {
  getSmartGatewayBudget,
  getSmartGatewayHealthProbeSecs,
  getSmartGatewayInboundLimits,
  getSmartGatewaySubagentInheritUpstream,
  setSmartGatewayBudget,
  setSmartGatewayHealthProbeSecs,
  setSmartGatewayInboundLimits,
  setSmartGatewaySubagentInheritUpstream,
} from "@/services/providers";
import type { SmartGatewayInboundLimits } from "@/types/backend";

const { Text } = Typography;

export function GatewayLimitsCard({
  modelOptions,
}: {
  modelOptions: CatalogModelSelectOption[];
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const inboundQuery = useQuery({ queryKey: ["smart-gateway-inbound"], queryFn: getSmartGatewayInboundLimits });
  const budgetQuery = useQuery({ queryKey: ["smart-gateway-budget"], queryFn: getSmartGatewayBudget });
  const probeQuery = useQuery({ queryKey: ["smart-gateway-health-probe"], queryFn: getSmartGatewayHealthProbeSecs });
  const inheritQuery = useQuery({ queryKey: ["smart-gateway-subagent-inherit-upstream"], queryFn: getSmartGatewaySubagentInheritUpstream });
  const [inbound, setInbound] = useState<SmartGatewayInboundLimits | null>(null);
  const [budgetUsd, setBudgetUsd] = useState(0);
  const [action, setAction] = useState("warn");
  const [fallbackModel, setFallbackModel] = useState("");
  const [probeSecs, setProbeSecs] = useState(300);
  const [inheritUpstream, setInheritUpstream] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => { if (inboundQuery.data) setInbound(inboundQuery.data); }, [inboundQuery.data]);
  useEffect(() => {
    if (budgetQuery.data) {
      setBudgetUsd(budgetQuery.data.dailyBudgetUsd);
      setAction(budgetQuery.data.action);
      setFallbackModel(budgetQuery.data.fallbackModel);
    }
  }, [budgetQuery.data]);
  useEffect(() => { if (typeof probeQuery.data === "number") setProbeSecs(probeQuery.data); }, [probeQuery.data]);
  useEffect(() => { if (typeof inheritQuery.data === "boolean") setInheritUpstream(inheritQuery.data); }, [inheritQuery.data]);

  const save = async () => {
    if (!inbound) return;
    setSaving(true);
    try {
      await setSmartGatewayInboundLimits(inbound);
      await setSmartGatewayBudget({ dailyBudgetUsd: budgetUsd, action, fallbackModel });
      await setSmartGatewayHealthProbeSecs(probeSecs);
      await setSmartGatewaySubagentInheritUpstream(inheritUpstream);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["smart-gateway-inbound"] }),
        queryClient.invalidateQueries({ queryKey: ["smart-gateway-budget"] }),
        queryClient.invalidateQueries({ queryKey: ["smart-gateway-health-probe"] }),
        queryClient.invalidateQueries({ queryKey: ["smart-gateway-subagent-inherit-upstream"] }),
      ]);
      void message.success(t("gateway.limitsSaved", { defaultValue: "限额已保存" }));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  const patchInbound = (patch: Partial<SmartGatewayInboundLimits>) => {
    setInbound((current) => (current ? { ...current, ...patch } : current));
  };

  return (
    <Card size="small" title={t("gateway.limitsTitle", { defaultValue: "限额与配额" })} extra={<Button size="small" type="primary" loading={saving} disabled={!inbound} onClick={() => void save()}>{t("common.save", { defaultValue: "保存" })}</Button>}>
      <Space direction="vertical" size={8} style={{ width: "100%" }}>
        <Text type="secondary">{t("gateway.limitsHint", { defaultValue: "默认全关，不改变现有行为。超限回 429 + Retry-After。日预算按去重后的当日花费估算。" })}</Text>
        <SettingsRow title={t("gateway.limitConcurrency", { defaultValue: "入口并发" })} description={t("gateway.limitConcurrencyHint", { defaultValue: "0 = 不限制，最多 64" })} control={<InputNumber min={0} max={64} value={inbound?.maxConcurrency ?? 0} onChange={(value) => patchInbound({ maxConcurrency: value ?? 0 })} />} />
        <SettingsRow title={t("gateway.limitInterval", { defaultValue: "最小间隔 (ms)" })} description={t("gateway.limitIntervalHint", { defaultValue: "0 = 不限制" })} control={<InputNumber min={0} max={10000} value={inbound?.minIntervalMs ?? 0} onChange={(value) => patchInbound({ minIntervalMs: value ?? 0 })} />} />
        <SettingsRow title={t("gateway.limitRpm", { defaultValue: "RPM" })} description={t("gateway.limitRpmHint", { defaultValue: "0 = 不限制；突发令牌默认 8" })} control={<Space><InputNumber min={0} max={600} value={inbound?.rpm ?? 0} onChange={(value) => patchInbound({ rpm: value ?? 0 })} /><InputNumber min={1} max={64} value={inbound?.burst ?? 8} onChange={(value) => patchInbound({ burst: value ?? 8 })} /></Space>} />
        <SettingsRow title={t("gateway.limitTimeout", { defaultValue: "等待超时 (s)" })} control={<InputNumber min={0} max={120} value={inbound?.acquireTimeoutSecs ?? 8} onChange={(value) => patchInbound({ acquireTimeoutSecs: value ?? 8 })} />} />
        <SettingsRow title={t("gateway.dailyBudget", { defaultValue: "日花费上限 (USD)" })} description={t("gateway.dailyBudgetHint", { spent: (budgetQuery.data?.todaySpendUsd ?? 0).toFixed(4), defaultValue: "今日已花 ${{spent}}。0 = 不限制。" })} control={<InputNumber min={0} step={0.5} value={budgetUsd} onChange={(value) => setBudgetUsd(value ?? 0)} />} />
        <SettingsRow title={t("gateway.budgetAction", { defaultValue: "超限动作" })} control={<Space><Select style={{ width: 140 }} value={action} options={[{ value: "warn", label: t("gateway.budgetWarn", { defaultValue: "仅告警" }) }, { value: "reject", label: t("gateway.budgetReject", { defaultValue: "拒绝" }) }, { value: "fallback", label: t("gateway.budgetFallback", { defaultValue: "降级模型" }) }]} onChange={setAction} />{action === "fallback" ? <Select {...catalogModelSelectProps} allowClear style={{ minWidth: 220 }} value={fallbackModel || undefined} options={modelOptions} placeholder={t("gateway.budgetFallbackModel", { defaultValue: "便宜模型" })} onChange={(value) => setFallbackModel(value ?? "")} /> : <Input style={{ display: "none" }} />}</Space>} />
        <SettingsRow title={t("gateway.healthProbe", { defaultValue: "健康探测间隔 (s)" })} description={t("gateway.healthProbeHint", { defaultValue: "默认 300；0 = 关闭主动探测。最短 30 秒。" })} control={<InputNumber min={0} max={3600} value={probeSecs} onChange={(value) => setProbeSecs(value ?? 0)} />} />
        <SettingsRow title={t("gateway.subagentInheritUpstream", { defaultValue: "子代理沿用父会话上游" })} description={t("gateway.subagentInheritUpstreamHint", { defaultValue: "仅接受 x-cs-parent-session-id；不猜测父会话，默认关闭。" })} control={<Switch checked={inheritUpstream} onChange={setInheritUpstream} />} />
      </Space>
    </Card>
  );
}
