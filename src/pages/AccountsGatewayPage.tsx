import { Segmented, Typography } from "antd";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import AntigravityPage from "@/pages/AntigravityPage";
import KiroPage from "@/pages/KiroPage";
import { ChatGptAccounts, CODEX_OAUTH_ACCOUNTS_KEY } from "@/components/chatgpt";
import { getAntigravityGatewayStatus, listAntigravityAccounts } from "@/services/antigravity";
import { getKiroGatewayStatus, listKiroAccounts } from "@/services/kiro";
import { listCodexOauthAccounts } from "@/services/providers";
import { useProviderAccountsStore } from "@/stores/providerAccountsStore";
import type { ProviderAccountKind } from "@/stores/providerAccountsStore";

const { Text } = Typography;

/**
 * Account-login gateways in one place: Antigravity / Kiro reverse proxies and
 * the ChatGPT (Codex OAuth) account pool. Each tab reuses the existing page.
 */
export default function AccountsGatewayPage() {
  const { t } = useTranslation();
  const tab = useProviderAccountsStore((state) => state.tab);
  const setTab = useProviderAccountsStore((state) => state.setTab);

  // Same query keys as the embedded pages, so the summary and the tab body
  // share one cache entry and one poll.
  const agStatus = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 5_000,
  });
  const agAccounts = useQuery({
    queryKey: ["antigravity-accounts"],
    queryFn: listAntigravityAccounts,
  });
  const kiroStatus = useQuery({
    queryKey: ["kiro-gateway"],
    queryFn: getKiroGatewayStatus,
    refetchInterval: 5_000,
  });
  const kiroAccounts = useQuery({
    queryKey: ["kiro-accounts"],
    queryFn: listKiroAccounts,
  });
  const chatgptAccounts = useQuery({
    queryKey: CODEX_OAUTH_ACCOUNTS_KEY,
    queryFn: listCodexOauthAccounts,
  });

  const summaries: Array<{
    kind: ProviderAccountKind;
    label: string;
    running: boolean;
    detail: string;
  }> = [
    {
      kind: "antigravity",
      label: "Antigravity",
      running: agStatus.data?.running ?? false,
      detail: t("accountsGateway.summaryAccounts", {
        defaultValue: "{{count}} 个账号",
        count: agAccounts.data?.length ?? 0,
      }),
    },
    {
      kind: "kiro",
      label: "Kiro",
      running: kiroStatus.data?.running ?? false,
      detail: t("accountsGateway.summaryAccounts", {
        defaultValue: "{{count}} 个账号",
        count: kiroAccounts.data?.length ?? 0,
      }),
    },
    {
      kind: "chatgpt",
      label: "ChatGPT / Codex",
      running: (chatgptAccounts.data?.length ?? 0) > 0,
      detail: t("accountsGateway.summaryAccounts", {
        defaultValue: "{{count}} 个账号",
        count: chatgptAccounts.data?.length ?? 0,
      }),
    },
  ];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <div className="page-strip" role="group" aria-label={t("nav.accounts", { defaultValue: "网关" })}>
        <div className="page-strip-main">
          <Segmented<ProviderAccountKind>
            className="app-segmented-switcher"
            size="small"
            value={tab}
            onChange={setTab}
            options={summaries.map((item) => ({
              value: item.kind,
              label: (
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
                  <span
                    className="page-strip-dot"
                    style={{
                      ["--pill" as string]: item.running
                        ? "var(--color-success)"
                        : "var(--color-text-tertiary)",
                    }}
                    aria-hidden
                  />
                  {item.label}
                </span>
              ),
            }))}
          />
        </div>
        <div className="page-strip-actions">
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {summaries.find((item) => item.kind === tab)?.detail}
          </Text>
        </div>
      </div>

      {tab === "antigravity" && <AntigravityPage embedded />}
      {tab === "kiro" && <KiroPage />}
      {tab === "chatgpt" && <ChatGptAccounts />}
    </div>
  );
}
