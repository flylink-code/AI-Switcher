import { Typography } from "antd";
import { useTranslation } from "react-i18next";
import { StatusBadge, Inline } from "@/components/ui";
import type { KiroAccountPublic, KiroGatewayStatus } from "@/services/kiro";

const { Text } = Typography;

interface AccountPoolOverviewProps {
  accounts: KiroAccountPublic[];
  status?: KiroGatewayStatus;
}

export function AccountPoolOverview({ accounts, status }: AccountPoolOverviewProps) {
  const { t } = useTranslation();
  const availableCount = accounts.filter((account) => !account.disabled).length;
  const port = status?.port ?? 15831;

  return (
    <Inline gap="md" align="center" wrap>
      <StatusBadge
        status={status?.running ? "running" : "stopped"}
        label={
          status?.running
            ? `${t("kiro.gateway")} ${t("kiro.running")} · 127.0.0.1:${port}`
            : `${t("kiro.gateway")} ${t("kiro.stoppedState")}`
        }
      />
      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("kiro.availableAccounts", { available: availableCount, total: accounts.length })}
      </Text>
      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("kiro.rotationStrategy")}: {t("kiro.rotationRoundRobin")}
      </Text>
    </Inline>
  );
}
