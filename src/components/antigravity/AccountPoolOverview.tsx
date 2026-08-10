import { Card, Space, Typography } from "antd";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import CheckCircleOutlined from "@ant-design/icons/es/icons/CheckCircleOutlined";
import SyncOutlined from "@ant-design/icons/es/icons/SyncOutlined";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import { useTranslation } from "react-i18next";
import type { AntigravityAccountPublic, AntigravityGatewayStatus } from "@/services/api";
import { Metric, Surface, StatusBadge } from "@/components/ui";

const { Text } = Typography;

interface AccountPoolOverviewProps {
  accounts: AntigravityAccountPublic[];
  status?: AntigravityGatewayStatus;
}

export function AccountPoolOverview({ accounts, status }: AccountPoolOverviewProps) {
  const { t } = useTranslation();

  const activeAccount = accounts.find((a) => a.isActive);
  const availableCount = accounts.filter((a) => !a.disabled && !a.quotaForbidden).length;
  const totalCount = accounts.length;

  return (
    <Card size="small" style={{ marginBottom: 16 }}>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))",
          gap: 16,
        }}
      >
        <Surface variant="inset" padding="sm">
          <Metric
            label={t("antigravity.activeAccount")}
            value={activeAccount ? activeAccount.email : t("common.none")}
            helpText={activeAccount ? t("antigravity.active") : t("antigravity.emptyAccounts")}
            icon={<UserOutlined style={{ color: "var(--ant-color-primary)" }} />}
          />
        </Surface>

        <Surface variant="inset" padding="sm">
          <Metric
            label={t("antigravity.availableAccounts")}
            value={`${availableCount} / ${totalCount}`}
            helpText={t("antigravity.accountsCount", { count: totalCount })}
            icon={<CheckCircleOutlined style={{ color: "#52c41a" }} />}
          />
        </Surface>

        <Surface variant="inset" padding="sm">
          <Metric
            label={t("antigravity.rotationStrategy")}
            value={t("antigravity.rotationActive")}
            helpText={t("antigravity.rotationHint")}
            icon={<SyncOutlined style={{ color: "#722ed1" }} />}
          />
        </Surface>

        <Surface variant="inset" padding="sm">
          <div style={{ display: "flex", flexDirection: "column", height: "100%", justifyContent: "space-between" }}>
            <Metric
              label={t("antigravity.gateway")}
              value={status?.running ? t("antigravity.running") : t("antigravity.stoppedState")}
              helpText={status?.baseUrl ?? `http://127.0.0.1:${status?.port ?? 15830}`}
              icon={<ApiOutlined style={{ color: status?.running ? "#52c41a" : "#faad14" }} />}
            />
            <div style={{ marginTop: 4 }}>
              <StatusBadge
                status={status?.running ? "running" : "stopped"}
                text={status?.running ? `Port: ${status.port}` : t("antigravity.stoppedState")}
              />
            </div>
          </div>
        </Surface>
      </div>
    </Card>
  );
}
