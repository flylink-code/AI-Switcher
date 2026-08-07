import { Button, Card, Space, Tag, Typography, message } from "antd";
import ReloadOutlined from "@ant-design/icons/es/icons/ReloadOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useNavigatePage } from "@/lib/navigation";
import {
  listAntigravityAccounts,
  refreshAntigravityQuotas,
  type AntigravityAccountPublic,
} from "@/services/api";
import {
  QuotaMiniBar,
  accountQuotaSummary,
  formatTierLabel,
  tierTagColor,
} from "@/components/AntigravityQuotaBars";

const { Text } = Typography;

function errMsg(error: unknown): string {
  if (typeof error === "string" && error.trim()) return error;
  if (error instanceof Error && error.message.trim()) return error.message;
  if (error && typeof error === "object" && "message" in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === "string" && message.trim()) return message;
  }
  return String(error ?? "未知错误");
}

/** Compact Antigravity account strip shown under workbench charts when gateway is running. */
export function WorkbenchAntigravityPanel() {
  const { t } = useTranslation();
  const navigate = useNavigatePage();
  const queryClient = useQueryClient();

  const accountsQuery = useQuery({
    queryKey: ["antigravity-accounts"],
    queryFn: listAntigravityAccounts,
    refetchInterval: 30_000,
  });

  const refreshMutation = useMutation({
    mutationFn: refreshAntigravityQuotas,
    onSuccess: async () => {
      message.success(t("antigravity.quotaRefreshed"));
      await queryClient.invalidateQueries({ queryKey: ["antigravity-accounts"] });
    },
    onError: (error: unknown) => message.error(errMsg(error), 10),
  });

  const accounts = accountsQuery.data ?? [];

  return (
    <Card
      size="small"
      className="page-surface workbench-ag-panel"
      title={
        <Space>
          <span>{t("workbench.antigravityAccounts")}</span>
          <Tag color="purple">{t("antigravity.accountsCount", { count: accounts.length })}</Tag>
        </Space>
      }
      extra={
        <Space>
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={refreshMutation.isPending}
            onClick={() => refreshMutation.mutate()}
          >
            {t("antigravity.refreshQuota")}
          </Button>
          <Button size="small" type="link" onClick={() => navigate("antigravity")}>
            {t("workbench.antigravityManage")}
          </Button>
        </Space>
      }
    >
      {accounts.length === 0 ? (
        <Text type="secondary">{t("workbench.antigravityNoAccounts")}</Text>
      ) : (
        <div className="workbench-ag-account-grid">
          {accounts.map((account) => (
            <AccountCompactCard key={account.id} account={account} />
          ))}
        </div>
      )}
    </Card>
  );
}

function AccountCompactCard({ account }: { account: AntigravityAccountPublic }) {
  const { t } = useTranslation();
  const { fiveHour, weekly } = accountQuotaSummary(account);
  const tier = formatTierLabel(account.subscriptionTier);
  const cooling =
    account.cooldownUntil != null && account.cooldownUntil * 1000 > Date.now();

  return (
    <div className="workbench-ag-account-card" data-disabled={account.disabled || undefined}>
      <div className="workbench-ag-account-head">
        <Text strong ellipsis style={{ maxWidth: "100%" }}>
          {account.email}
        </Text>
        <Space size={4} wrap>
          {tier && <Tag color={tierTagColor(account.subscriptionTier)}>{tier}</Tag>}
          {account.isActive && <Tag color="green">{t("antigravity.active")}</Tag>}
          {account.disabled && <Tag color="red">{t("antigravity.disabled")}</Tag>}
          {cooling && <Tag color="orange">{t("antigravity.cooling")}</Tag>}
          {account.quotaForbidden && <Tag color="red">{t("antigravity.forbidden")}</Tag>}
        </Space>
      </div>
      <QuotaMiniBar label={t("antigravity.quota5h")} percent={fiveHour} />
      <QuotaMiniBar label={t("antigravity.quota7d")} percent={weekly} />
    </div>
  );
}
