import { Alert, Tag, Typography } from "antd";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  getAntigravityPoolWarning,
  getAntigravityRecommendedAccount,
  type AntigravityAccountPublic,
  type AntigravityGatewayStatus,
} from "@/services/api";
import { StatusBadge, Inline } from "@/components/ui";

const { Text } = Typography;

interface AccountPoolOverviewProps {
  accounts: AntigravityAccountPublic[];
  status?: AntigravityGatewayStatus;
}

/** Compact runtime summary replacing the previous four-card overview. */
export function AccountPoolOverview({ accounts, status }: AccountPoolOverviewProps) {
  const { t } = useTranslation();

  const poolFingerprint = accounts
    .map(
      (account) =>
        `${account.id}:${account.remainingQuota ?? ""}:${account.quotaForbidden ? 1 : 0}:${account.cooldownUntil ?? ""}`,
    )
    .join("|");

  const warningQuery = useQuery({
    queryKey: ["antigravity-pool-warning", poolFingerprint],
    queryFn: getAntigravityPoolWarning,
  });

  const recommendedQuery = useQuery({
    queryKey: ["antigravity-recommended-account", poolFingerprint],
    queryFn: getAntigravityRecommendedAccount,
  });

  const activeAccount = accounts.find((a) => a.isActive);
  const availableCount = accounts.filter((a) => !a.disabled && !a.quotaForbidden).length;
  const totalCount = accounts.length;
  const warning = warningQuery.data;
  const recommended = recommendedQuery.data;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
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
        {recommended && recommended.id !== activeAccount?.id && (
          <Tag color="cyan" style={{ fontSize: "var(--font-size-xs)", margin: 0 }}>
            {t("antigravity.recommendedFirst")}: {recommended.email}
          </Tag>
        )}
        <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
          {t("antigravity.rotationStrategy")}: {t("antigravity.rotationWeighted")}
        </Text>
      </Inline>
      {warning?.hasWarning && (
        <Alert
          type={warning.warningLevel === "exhausted" ? "error" : "warning"}
          showIcon
          banner
          message={warning.message}
          style={{ borderRadius: 6, padding: "4px 12px", fontSize: 13 }}
        />
      )}
    </div>
  );
}
