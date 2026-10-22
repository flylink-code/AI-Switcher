import { useState } from "react";
import {
  Button,
  Card,
  Radio,
  Select,
  Space,
  Spin,
  Tag,
  Typography,
  theme,
  message,
} from "antd";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getCurrentWebviewWindow, WebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { ProviderTarget } from "@/types/backend";
import {
  getSmartGatewayStatus,
  getAgentConnectionMode,
  bindSmartGateway,
  switchToOfficial,
} from "@/services/providers";
import {
  getAntigravityGatewayStatus,
  listAntigravityAccounts,
} from "@/services/antigravity";
import { getKiroGatewayStatus, listKiroAccounts } from "@/services/kiro";
import { getUsageDashboard } from "@/services/usage";

const { Text } = Typography;

export function QuickPanel() {
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const queryClient = useQueryClient();
  const [activeTarget, setActiveTarget] = useState<ProviderTarget>("claude_code");

  // 1. 智能网关状态
  const gwQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    refetchInterval: 3_000,
  });

  // 2. 当前选中 Agent 连接状态模式 ("gateway" | "direct" | "external")
  const connectionModeQuery = useQuery({
    queryKey: ["agent-connection-mode", activeTarget],
    queryFn: () => getAgentConnectionMode(activeTarget),
    refetchInterval: 3_000,
  });

  // 3. 反代网关与账号池状态
  const agStatusQuery = useQuery({
    queryKey: ["antigravity-gateway"],
    queryFn: getAntigravityGatewayStatus,
    refetchInterval: 5_000,
  });
  const agAccountsQuery = useQuery({
    queryKey: ["antigravity-accounts"],
    queryFn: listAntigravityAccounts,
  });

  const kiroStatusQuery = useQuery({
    queryKey: ["kiro-gateway"],
    queryFn: getKiroGatewayStatus,
    refetchInterval: 5_000,
  });
  const kiroAccountsQuery = useQuery({
    queryKey: ["kiro-accounts"],
    queryFn: listKiroAccounts,
  });

  // 4. 今日用量概览
  const usageQuery = useQuery({
    queryKey: ["quick-panel-usage-today"],
    queryFn: () => getUsageDashboard({ today: true }, "all"),
    refetchInterval: 15_000,
  });

  // 切换模式操作
  const switchMutation = useMutation({
    mutationFn: async (mode: "gateway" | "official") => {
      if (mode === "gateway") {
        await bindSmartGateway(activeTarget);
      } else {
        await switchToOfficial(activeTarget);
      }
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["agent-connection-mode", activeTarget] });
      void queryClient.invalidateQueries({ queryKey: ["smart-gateway-status"] });
      void message.success(t("common.saved", { defaultValue: "配置已更新" }));
    },
    onError: (err: unknown) => {
      void message.error(String(err));
    },
  });

  const handleOpenMain = async () => {
    try {
      const mainWindow = await WebviewWindow.getByLabel("main");
      if (mainWindow) {
        await mainWindow.show();
        await mainWindow.unminimize();
        await mainWindow.setFocus();
      }
    } catch (e) {
      console.error("打开主窗口失败:", e);
    }
    try {
      const panel = getCurrentWebviewWindow();
      await panel.hide();
    } catch {
      // 忽略浮窗隐藏异常
    }
  };

  const handleClosePanel = async () => {
    try {
      const panel = getCurrentWebviewWindow();
      await panel.hide();
    } catch {
      // 忽略浮窗隐藏异常
    }
  };

  const rawMode = connectionModeQuery.data ?? "external";
  const currentMode = rawMode === "gateway" ? "gateway" : rawMode === "direct" ? "direct" : "official";
  const summary = usageQuery.data?.summary;
  const hasAntigravity =
    (agAccountsQuery.data?.length ?? 0) > 0 || (agStatusQuery.data?.running ?? false);
  const hasKiro =
    (kiroAccountsQuery.data?.length ?? 0) > 0 || (kiroStatusQuery.data?.running ?? false);
  const hasAnyReverseProxy = hasAntigravity || hasKiro;

  const totalTokens = (summary?.inputTokens ?? 0) + (summary?.outputTokens ?? 0);
  const totalTokensDisplay =
    totalTokens > 1_000_000
      ? `${(totalTokens / 1_000_000).toFixed(1)}M`
      : `${Math.round(totalTokens / 1_000)}k`;

  return (
    <div
      style={{
        width: "100vw",
        height: "100vh",
        background: token.colorBgContainer,
        display: "flex",
        flexDirection: "column",
        userSelect: "none",
        overflow: "hidden",
        border: `1px solid ${token.colorBorderSecondary}`,
        boxSizing: "border-box",
      }}
    >
      {/* 顶部标题栏 */}
      <div
        style={{
          padding: "10px 14px",
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorFillAlter,
        }}
      >
        <Space size={8}>
          <div
            style={{
              width: 8,
              height: 8,
              borderRadius: "50%",
              backgroundColor: gwQuery.data?.running
                ? token.colorSuccess
                : token.colorTextQuaternary,
            }}
          />
          <Text strong style={{ fontSize: 13 }}>
            AI-Switcher
          </Text>
        </Space>
        <Space size={12}>
          <Button
            size="small"
            type="link"
            onClick={handleOpenMain}
            style={{ padding: 0, fontSize: 12 }}
          >
            {t("common.openFullWindow", { defaultValue: "完整控制台" })}
          </Button>
          <Button
            size="small"
            type="text"
            onClick={handleClosePanel}
            style={{ padding: 0, width: 20, height: 20, lineHeight: "20px" }}
          >
            ×
          </Button>
        </Space>
      </div>

      {/* 主体滚动区 */}
      <div
        style={{
          flex: 1,
          padding: 12,
          overflowY: "auto",
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        {/* 1. 快速配置 */}
        <Card
          size="small"
          title={
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <span style={{ fontSize: 13 }}>{t("tray.quickConfig", { defaultValue: "快速切换" })}</span>
              <Select<ProviderTarget>
                size="small"
                value={activeTarget}
                onChange={setActiveTarget}
                style={{ width: 130 }}
                options={[
                  { label: "Claude Code", value: "claude_code" },
                  { label: "Codex", value: "codex" },
                ]}
              />
            </div>
          }
          style={{ borderRadius: 6 }}
        >
          <Spin spinning={connectionModeQuery.isLoading || switchMutation.isPending}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t("tray.mode", { defaultValue: "运行模式:" })}
              </Text>
              <Radio.Group
                size="small"
                value={currentMode}
                onChange={(e) => {
                  const val = e.target.value as "gateway" | "official" | "direct";
                  if (val === "gateway" || val === "official") {
                    switchMutation.mutate(val);
                  } else {
                    void handleOpenMain();
                  }
                }}
              >
                <Radio.Button value="gateway">智能网关</Radio.Button>
                <Radio.Button value="official">官方</Radio.Button>
                <Radio.Button value="direct">直连</Radio.Button>
              </Radio.Group>
            </div>
          </Spin>
        </Card>

        {/* 2. 今日用量概览 */}
        <Card
          size="small"
          title={<span style={{ fontSize: 13 }}>{t("tray.usageToday", { defaultValue: "今日用量概览" })}</span>}
          style={{ borderRadius: 6 }}
        >
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "1fr 1fr 1fr",
              gap: 6,
              textAlign: "center",
            }}
          >
            <div>
              <Text type="secondary" style={{ fontSize: 11, display: "block" }}>
                请求数
              </Text>
              <Text strong style={{ fontSize: 14 }}>
                {summary?.requestCount ?? 0}
              </Text>
            </div>
            <div>
              <Text type="secondary" style={{ fontSize: 11, display: "block" }}>
                Tokens
              </Text>
              <Text strong style={{ fontSize: 14 }}>
                {totalTokensDisplay}
              </Text>
            </div>
            <div>
              <Text type="secondary" style={{ fontSize: 11, display: "block" }}>
                预估花费
              </Text>
              <Text strong style={{ fontSize: 14 }}>
                ${(summary?.estimatedCost ?? 0).toFixed(2)}
              </Text>
            </div>
          </div>
        </Card>

        {/* 3. 网关与反代状态（按需展示） */}
        <Card
          size="small"
          title={<span style={{ fontSize: 13 }}>{t("tray.gatewayStatus", { defaultValue: "网关与反代状态" })}</span>}
          style={{ borderRadius: 6 }}
        >
          <Space direction="vertical" style={{ width: "100%" }} size={6}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <Text style={{ fontSize: 12 }}>智能网关 (15828)</Text>
              <Tag color={gwQuery.data?.running ? "success" : "default"}>
                {gwQuery.data?.running ? "运行中" : "已停止"}
              </Tag>
            </div>

            {hasAntigravity && (
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <Text style={{ fontSize: 12 }}>Antigravity (15830)</Text>
                <Space size={4}>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {agAccountsQuery.data?.length ?? 0} 账号
                  </Text>
                  <Tag color={agStatusQuery.data?.running ? "processing" : "default"}>
                    {agStatusQuery.data?.running ? "在线" : "未启动"}
                  </Tag>
                </Space>
              </div>
            )}

            {hasKiro && (
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <Text style={{ fontSize: 12 }}>Kiro (15831)</Text>
                <Space size={4}>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {kiroAccountsQuery.data?.length ?? 0} 账号
                  </Text>
                  <Tag color={kiroStatusQuery.data?.running ? "processing" : "default"}>
                    {kiroStatusQuery.data?.running ? "在线" : "未启动"}
                  </Tag>
                </Space>
              </div>
            )}

            {!hasAnyReverseProxy && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                未配置反向代理账号池
              </Text>
            )}
          </Space>
        </Card>
      </div>

      {/* 底部操作区 */}
      <div
        style={{
          padding: "8px 12px",
          borderTop: `1px solid ${token.colorBorderSecondary}`,
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          background: token.colorFillAlter,
        }}
      >
        <Button size="small" onClick={handleClosePanel}>
          {t("common.close", { defaultValue: "收起" })}
        </Button>
        <Button size="small" type="primary" onClick={handleOpenMain}>
          {t("common.manage", { defaultValue: "进入完整控制台" })}
        </Button>
      </div>
    </div>
  );
}
