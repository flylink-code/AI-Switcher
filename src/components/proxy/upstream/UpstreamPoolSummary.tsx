import { useMemo } from "react";
import { Space, Tag, Typography } from "antd";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import CloseCircleOutlined from "@ant-design/icons/es/icons/CloseCircleOutlined";
import WarningOutlined from "@ant-design/icons/es/icons/WarningOutlined";
import QuestionCircleOutlined from "@ant-design/icons/es/icons/QuestionCircleOutlined";
import { useTranslation } from "react-i18next";
import type { GatewayUpstreamHealth, Provider } from "@/types/backend";

const { Text } = Typography;

export interface UpstreamPoolSummaryProps {
  upstreams: Provider[];
  healthList: GatewayUpstreamHealth[];
}

export function UpstreamPoolSummary({
  upstreams,
  healthList,
}: UpstreamPoolSummaryProps) {
  const { t } = useTranslation();

  const stats = useMemo(() => {
    const healthMap = new Map<string, GatewayUpstreamHealth>();
    for (const h of healthList) {
      healthMap.set(h.upstreamId, h);
    }

    let ok = 0;
    let cooling = 0;
    let failing = 0;
    let unknown = 0;

    for (const u of upstreams) {
      const h = healthMap.get(u.id);
      if (!h || h.status === "unknown") {
        unknown++;
      } else if (h.status === "ok") {
        ok++;
      } else if (h.status === "cooling" || h.status === "rate_limited") {
        cooling++;
      } else if (h.status === "auth_failed" || h.status === "failing") {
        failing++;
      } else {
        unknown++;
      }
    }

    return {
      total: upstreams.length,
      ok,
      cooling,
      failing,
      unknown,
    };
  }, [upstreams, healthList]);

  if (stats.total === 0) return null;

  return (
    <div
      style={{
        padding: "6px 12px",
        marginBottom: 12,
        borderRadius: 6,
        background: "var(--ant-color-fill-quaternary, rgba(0, 0, 0, 0.02))",
        border: "1px solid var(--ant-color-border-secondary, #f0f0f0)",
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        flexWrap: "wrap",
        gap: 8,
      }}
    >
      <Space size={12} wrap align="center">
        <Text strong style={{ fontSize: 13 }}>
          {t("proxy.upstreamPoolStatus", { defaultValue: "上游池健康概览" })}
        </Text>
        <Space size={6} wrap>
          <Tag color="default">
            {t("proxy.upstreamTotalCount", { count: stats.total, defaultValue: `共 ${stats.total} 个上游` })}
          </Tag>
          <Tag color="success" icon={<CheckCircleOutlined />}>
            {t("proxy.upstreamOkCount", { count: stats.ok, defaultValue: `正常 ${stats.ok}` })}
          </Tag>
          {stats.cooling > 0 ? (
            <Tag color="warning" icon={<WarningOutlined />}>
              {t("proxy.upstreamCoolingCount", { count: stats.cooling, defaultValue: `冷却中 ${stats.cooling}` })}
            </Tag>
          ) : null}
          {stats.failing > 0 ? (
            <Tag color="error" icon={<CloseCircleOutlined />}>
              {t("proxy.upstreamFailingCount", { count: stats.failing, defaultValue: `异常 ${stats.failing}` })}
            </Tag>
          ) : null}
          {stats.unknown > 0 ? (
            <Tag color="default" icon={<QuestionCircleOutlined />}>
              {t("proxy.upstreamUnknownCount", { count: stats.unknown, defaultValue: `未检测 ${stats.unknown}` })}
            </Tag>
          ) : null}
        </Space>
      </Space>
    </div>
  );
}
