import { Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { AntigravityAccountPublic, AntigravityGatewayStatus } from "@/services/api";
import { StatusBadge, Inline } from "@/components/ui";

const { Text } = Typography;

interface AccountPoolOverviewProps {
  accounts: AntigravityAccountPublic[];
  status?: AntigravityGatewayStatus;
}

/** Compact runtime summary replacing the previous four-card overview. */
export function AccountPoolOverview({ accounts, status }: AccountPoolOverviewProps) {
  const { t } = useTranslation();

  const activeAccount = accounts.find((a) => a.isActive);
  const availableCount = accounts.filter((a) => !a.disabled && !a.quotaForbidden).length;
  const totalCount = accounts.length;

  return (
    <Inline gap="md" align="center" wrap>
      <StatusBadge
        status={status?.running ? "running" : "stopped"}
        label={
          status?.running
            ? `${t("antigravity.gateway")} ${t("antigravity.running")} · 127.0.0.1:${status.port}`
            : `${t("antigravity.gateway")} ${t("antigravity.stoppedState")}`
        }
      />
      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("antigravity.availableAccounts")} {availableCount} / {totalCount}
      </Text>
      {activeAccount && (
        <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
          {t("antigravity.activeAccount")}: {activeAccount.email}
        </Text>
      )}
      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("antigravity.rotationStrategy")}: {t("antigravity.rotationActive")}
      </Text>
    </Inline>
  );
}
