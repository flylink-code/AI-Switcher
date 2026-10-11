import { useState } from "react";
import { Button, List, Space, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CodexOauthModal } from "@/components/proxy/upstream/CodexOauthModal";
import {
  ensureCodexOauthProvider,
  listCodexOauthAccounts,
  pollCodexOauthLogin,
  startCodexOauthLogin,
} from "@/services/providers";
import type { CodexOauthDeviceStart } from "@/types/backend";

const { Text } = Typography;

export function ChatGptSection() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [device, setDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [busy, setBusy] = useState(false);
  const accountsQuery = useQuery({
    queryKey: ["codex-oauth-accounts"],
    queryFn: listCodexOauthAccounts,
  });

  const login = async () => {
    setBusy(true);
    try {
      const next = await startCodexOauthLogin();
      setDevice(next);
      await openUrl(next.verificationUri);
      const deadline = Date.now() + next.expiresIn * 1000;
      while (Date.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, Math.max(1, next.interval) * 1000));
        const result = await pollCodexOauthLogin(next.deviceCode);
        if (result.status === "pending") continue;
        if (result.status === "complete" && result.account) {
          await ensureCodexOauthProvider("claude_code", result.account.accountId);
          setDevice(null);
          await queryClient.invalidateQueries({ queryKey: ["codex-oauth-accounts"] });
          await queryClient.invalidateQueries({ queryKey: ["gateway-upstreams"] });
          void message.success(t("providers.chatgptLoginSuccess", { defaultValue: "ChatGPT / Codex 账号登录成功" }));
          return;
        }
        throw new Error(result.message || t("providers.chatgptLoginFailed", { defaultValue: "登录失败" }));
      }
      throw new Error(t("providers.chatgptLoginExpired", { defaultValue: "登录已超时，请重试" }));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Space direction="vertical" size={12} style={{ width: "100%" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <Text strong>{t("providers.chatgptTitle", { defaultValue: "ChatGPT / Codex" })}</Text>
        <Button type="primary" size="small" loading={busy} onClick={() => void login()}>
          {t("providers.chatgptLogin", { defaultValue: "登录" })}
        </Button>
      </div>
      <Text type="secondary">
        {t("providers.chatgptHint", { defaultValue: "OAuth 登录后作为上游加入智能网关，不新增端口。Codex OAuth 必须经网关。" })}
      </Text>
      <List
        size="small"
        dataSource={accountsQuery.data ?? []}
        locale={{ emptyText: t("providers.chatgptEmpty", { defaultValue: "还没有登录账号" }) }}
        renderItem={(account) => (
          <List.Item>
            <span>{account.email || account.accountId}</span>
          </List.Item>
        )}
      />
      <CodexOauthModal device={device} onClose={() => setDevice(null)} />
    </Space>
  );
}
