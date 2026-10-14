import { useState } from "react";
import { Button, Card, Empty, Popconfirm, Space, Tag, Typography, message } from "antd";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import CheckOutlined from "@ant-design/icons/es/icons/CheckOutlined";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { CodexOauthModal } from "@/components/proxy/upstream/CodexOauthModal";
import { StatusBadge } from "@/components/ui";
import { errMsg } from "@/lib/errMsg";
import {
  ensureCodexOauthProvider,
  getCodexOauthDefaultAccount,
  listCodexOauthAccounts,
  pollCodexOauthLogin,
  removeCodexOauthAccount,
  setCodexOauthDefaultAccount,
  startCodexOauthLogin,
} from "@/services/providers";
import type { CodexOauthDeviceStart } from "@/types/backend";

const { Text } = Typography;

export const CODEX_OAUTH_ACCOUNTS_KEY = ["codex-oauth-accounts"] as const;
const CODEX_OAUTH_DEFAULT_KEY = ["codex-oauth-default-account"] as const;

/** ChatGPT / Codex OAuth account pool: login, default account, removal. */
export function ChatGptAccounts() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [device, setDevice] = useState<CodexOauthDeviceStart | null>(null);
  const [loggingIn, setLoggingIn] = useState(false);

  const accountsQuery = useQuery({
    queryKey: CODEX_OAUTH_ACCOUNTS_KEY,
    queryFn: listCodexOauthAccounts,
  });
  const defaultQuery = useQuery({
    queryKey: CODEX_OAUTH_DEFAULT_KEY,
    queryFn: getCodexOauthDefaultAccount,
  });
  const accounts = accountsQuery.data ?? [];
  const defaultId = defaultQuery.data ?? null;

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: CODEX_OAUTH_ACCOUNTS_KEY }),
      queryClient.invalidateQueries({ queryKey: CODEX_OAUTH_DEFAULT_KEY }),
      queryClient.invalidateQueries({ queryKey: ["gateway-upstreams"] }),
    ]);
  };

  const setDefaultMutation = useMutation({
    mutationFn: setCodexOauthDefaultAccount,
    onSuccess: refresh,
    onError: (error) => void message.error(errMsg(error)),
  });
  const removeMutation = useMutation({
    mutationFn: removeCodexOauthAccount,
    onSuccess: async () => {
      void message.success(t("accountsGateway.chatgptRemoved", { defaultValue: "账号已移除" }));
      await refresh();
    },
    onError: (error) => void message.error(errMsg(error)),
  });

  const login = async () => {
    setLoggingIn(true);
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
          await refresh();
          void message.success(
            t("providers.chatgptLoginSuccess", { defaultValue: "ChatGPT / Codex 账号登录成功" }),
          );
          return;
        }
        throw new Error(
          result.message || t("providers.chatgptLoginFailed", { defaultValue: "登录失败" }),
        );
      }
      throw new Error(t("providers.chatgptLoginExpired", { defaultValue: "登录已超时，请重试" }));
    } catch (error) {
      setDevice(null);
      void message.error(errMsg(error));
    } finally {
      setLoggingIn(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          flexWrap: "wrap",
          gap: 12,
        }}
      >
        <Space size={12} wrap>
          <StatusBadge
            status={accounts.length > 0 ? "running" : "stopped"}
            label={t("accountsGateway.chatgptPool", { defaultValue: "ChatGPT / Codex 账号池" })}
          />
          <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
            {t("accountsGateway.chatgptCount", {
              defaultValue: "已登录 {{count}} 个账号",
              count: accounts.length,
            })}
          </Text>
        </Space>
        <Button
          type="primary"
          icon={<LoginOutlined />}
          loading={loggingIn}
          onClick={() => void login()}
        >
          {loggingIn
            ? t("antigravity.oauthWaiting")
            : t("accountsGateway.chatgptLogin", { defaultValue: "登录 ChatGPT 账号" })}
        </Button>
      </div>

      <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
        {t("providers.chatgptHint", {
          defaultValue: "OAuth 登录后作为上游加入智能网关，不新增端口。Codex OAuth 必须经网关。",
        })}
      </Text>

      {accounts.length === 0 ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={t("providers.chatgptEmpty", { defaultValue: "还没有登录账号" })}
        />
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {accounts.map((account) => {
            const isDefault = account.accountId === defaultId;
            return (
              <Card key={account.accountId} size="small" styles={{ body: { padding: 12 } }}>
                <div
                  style={{
                    display: "flex",
                    justifyContent: "space-between",
                    alignItems: "center",
                    gap: 12,
                    flexWrap: "wrap",
                  }}
                >
                  <Space size={8} wrap>
                    <UserOutlined />
                    <Text strong>{account.email || account.accountId}</Text>
                    {isDefault && (
                      <Tag color="blue" style={{ margin: 0 }}>
                        {t("accountsGateway.chatgptDefault", { defaultValue: "默认" })}
                      </Tag>
                    )}
                    <Text type="secondary" style={{ fontSize: "var(--font-size-xs)" }}>
                      {t("accountsGateway.chatgptLoggedAt", {
                        defaultValue: "登录于 {{time}}",
                        time: new Date(account.authenticatedAt * 1000).toLocaleString(),
                      })}
                    </Text>
                  </Space>
                  <Space size={8}>
                    {!isDefault && (
                      <Button
                        size="small"
                        icon={<CheckOutlined />}
                        loading={
                          setDefaultMutation.isPending &&
                          setDefaultMutation.variables === account.accountId
                        }
                        onClick={() => setDefaultMutation.mutate(account.accountId)}
                      >
                        {t("accountsGateway.chatgptSetDefault", { defaultValue: "设为默认" })}
                      </Button>
                    )}
                    <Popconfirm
                      title={t("accountsGateway.chatgptRemoveConfirm", {
                        defaultValue: "移除该账号及其绑定的网关上游？",
                      })}
                      okText={t("common.delete")}
                      cancelText={t("common.cancel")}
                      okButtonProps={{ danger: true }}
                      onConfirm={() => removeMutation.mutate(account.accountId)}
                    >
                      <Button
                        size="small"
                        danger
                        icon={<DeleteOutlined />}
                        loading={
                          removeMutation.isPending &&
                          removeMutation.variables === account.accountId
                        }
                      >
                        {t("common.delete")}
                      </Button>
                    </Popconfirm>
                  </Space>
                </div>
              </Card>
            );
          })}
        </div>
      )}
      <CodexOauthModal device={device} onClose={() => setDevice(null)} />
    </div>
  );
}
