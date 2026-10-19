import { useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Modal,
  Radio,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message,
} from "antd";
import RightOutlined from "@ant-design/icons/es/icons/RightOutlined";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { usageSourceIcon } from "@/components/UsageSourceIcons";
import { errMsg } from "@/lib/errMsg";
import {
  applyVscodeCopilot,
  getSmartGatewayStatus,
  getVscodeCopilotStatus,
  listGatewayUpstreams,
  removeVscodeCopilot,
  setVscodeCopilotDirect,
} from "@/services/providers";

const { Text, Paragraph } = Typography;
const QUERY_KEY = ["vscode-copilot-status"];

type CopilotConnectionMode = "gateway" | "direct" | "official";

/**
 * VS Code Copilot 配置卡片。
 * 采用与其他 Agent 卡片一致的顶栏图标/状态、连接模式切换（智能网关 / 直连 / 官方）、
 * 配置漂移检查/预览弹窗与折叠式「高级设置」布局。
 */
export function VscodeCopilotCard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);
  const [pendingDirect, setPendingDirect] = useState(false);
  const [rememberedDirectId, setRememberedDirectId] = useState<string | undefined>();

  const query = useQuery({
    queryKey: QUERY_KEY,
    queryFn: getVscodeCopilotStatus,
    refetchInterval: 15_000,
  });

  const gatewayStatusQuery = useQuery({
    queryKey: ["smart-gateway-status"],
    queryFn: getSmartGatewayStatus,
    refetchInterval: 10_000,
  });

  const upstreamsQuery = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
  });

  const status = query.data;
  const gatewayStatus = gatewayStatusQuery.data;
  const upstreams = useMemo(
    () => (upstreamsQuery.data ?? []).filter((u) => u.providerKind !== "smart_gateway"),
    [upstreamsQuery.data],
  );
  const isManaged = Boolean(status?.bound || status?.applied);
  const drifted = isManaged && (!status?.applied || (status?.drift.length ?? 0) > 0);
  const currentMode: CopilotConnectionMode =
    status?.mode === "direct"
      ? "direct"
      : isManaged
        ? "gateway"
        : "official";
  const displayMode: CopilotConnectionMode = pendingDirect ? "direct" : currentMode;
  const gatewayPort = gatewayStatus?.port ?? 15828;
  const gatewayRunning = gatewayStatus?.running ?? status?.gatewayRunning ?? false;
  const selectedUpstreamId =
    (status?.mode === "direct" && status.directUpstreamId
      ? status.directUpstreamId
      : undefined) || rememberedDirectId;

  const directOptions = useMemo(
    () =>
      upstreams.map((upstream) => {
        const paused = upstream.enabled === false;
        const name = `${upstream.name} (${upstream.model || upstream.protocolType})`;
        return {
          value: upstream.id,
          label: paused ? `${name} — ${t("proxy.upstreamPaused")}` : name,
          disabled: paused,
        };
      }),
    [upstreams, t],
  );

  const driftStatus = query.isError
    ? "unknown"
    : !status
      ? "loading"
      : !isManaged
        ? "unmanaged"
        : drifted
          ? "drifted"
          : "in_sync";

  const invalidateRelatedQueries = async () => {
    await queryClient.invalidateQueries({ queryKey: QUERY_KEY });
    await queryClient.invalidateQueries({ queryKey: ["smart-gateway-bindings"] });
    await queryClient.invalidateQueries({ queryKey: ["smart-gateway-status"] });
  };

  const run = async (action: () => Promise<unknown>, successKey: string) => {
    setBusy(true);
    try {
      await action();
      void message.success(t(successKey));
    } catch (error) {
      void message.error(errMsg(error));
    } finally {
      setBusy(false);
      await invalidateRelatedQueries();
    }
  };

  const resyncCurrent = () => {
    if (currentMode === "direct" && selectedUpstreamId) {
      return run(() => setVscodeCopilotDirect(selectedUpstreamId), "copilot.applied");
    }
    return run(applyVscodeCopilot, "copilot.applied");
  };

  const handleModeChange = (nextMode: CopilotConnectionMode) => {
    if (nextMode === displayMode && !pendingDirect) return;
    if (nextMode === "gateway") {
      setPendingDirect(false);
      void run(applyVscodeCopilot, "copilot.applied");
    } else if (nextMode === "direct") {
      const enabledUpstreams = upstreams.filter((u) => u.enabled !== false);
      const existingId =
        selectedUpstreamId && enabledUpstreams.some((u) => u.id === selectedUpstreamId)
          ? selectedUpstreamId
          : undefined;
      if (existingId) {
        setPendingDirect(false);
        void run(async () => {
          await setVscodeCopilotDirect(existingId);
          setRememberedDirectId(existingId);
        }, "copilot.applied");
      } else {
        setPendingDirect(true);
      }
    } else {
      setPendingDirect(false);
      if (isManaged) {
        setRemoveOpen(true);
      }
    }
  };

  const handleSelectDirectUpstream = (upstreamId: string) => {
    setRememberedDirectId(upstreamId);
    setPendingDirect(false);
    void run(() => setVscodeCopilotDirect(upstreamId), "copilot.applied");
  };

  return (
    <Card size="small" className="page-surface">
      {/* 卡片顶栏：与 AgentCardFrame 保持一致 */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        {usageSourceIcon("vscode_copilot", { size: 18 })}
        <Text strong>{t("copilot.title")}</Text>
        {status?.vscodeInstalled !== undefined ? (
          status.vscodeInstalled ? (
            <>
              {status.vscodeVersion ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {status.vscodeVersion}
                </Text>
              ) : null}
              <Text type="secondary" style={{ fontSize: 12 }}>
                <span aria-hidden style={{ color: "#34c759", marginRight: 4 }}>
                  ●
                </span>
                {t("agentHub.installed", { defaultValue: "已安装" })}
              </Text>
            </>
          ) : (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("copilot.vscodeNotDetected", { defaultValue: "未检测到 VS Code" })}
            </Text>
          )
        ) : null}
        {status ? (
          isManaged ? (
            <>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t("copilot.modelsCount", {
                  count: status.modelCount,
                  defaultValue: "{{count}} 个模型",
                })}
              </Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                <span
                  aria-hidden
                  style={{ color: drifted ? "#faad14" : "#34c759", marginRight: 4 }}
                >
                  ●
                </span>
                {t(drifted ? "copilot.statusDrifted" : "copilot.statusApplied")}
              </Text>
            </>
          ) : (
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("copilot.statusNotApplied")}
            </Text>
          )
        ) : null}
        <button
          type="button"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((value) => !value)}
          style={{
            display: "inline-flex",
            alignItems: "center",
            gap: 6,
            marginLeft: "auto",
            padding: 0,
            border: "none",
            background: "transparent",
            color: "inherit",
            cursor: "pointer",
            font: "inherit",
          }}
        >
          <RightOutlined
            style={{
              fontSize: 10,
              transform: advancedOpen ? "rotate(90deg)" : undefined,
              transition: "transform 0.15s ease",
            }}
          />
          {t("agentHub.advanced", { defaultValue: "高级设置" })}
        </button>
      </div>

      {/* 主控制区：模式切换 + 网关运行状态 + 配置漂移检查 */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
          <Radio.Group
            size="small"
            optionType="button"
            buttonStyle="solid"
            disabled={busy}
            value={displayMode}
            onChange={(event) =>
              handleModeChange(event.target.value as CopilotConnectionMode)
            }
            options={[
              {
                label: t("providers.modeGateway", { defaultValue: "智能网关" }),
                value: "gateway",
              },
              {
                label: t("providers.modeDirect", { defaultValue: "直连" }),
                value: "direct",
              },
              {
                label: t("providers.modeOfficial", { defaultValue: "官方" }),
                value: "official",
              },
            ]}
          />
          {displayMode === "gateway" || displayMode === "direct" ? (
            status || gatewayStatus ? (
              <Tag
                color={gatewayRunning ? "success" : "warning"}
                style={{ marginInlineEnd: 0 }}
                title={gatewayRunning ? undefined : t("providers.gatewayStoppedHint")}
              >
                {gatewayRunning
                  ? t("providers.statusGateway", { port: gatewayPort })
                  : t("providers.statusGatewayStopped", { port: gatewayPort })}
              </Tag>
            ) : (
              <Tag style={{ marginInlineEnd: 0 }}>
                {t("providers.statusGatewayUnknown")}
              </Tag>
            )
          ) : null}
          {displayMode === "direct" ? (
            <Select
              size="small"
              style={{ minWidth: 220, maxWidth: 420, flex: "1 1 220px" }}
              disabled={busy}
              value={selectedUpstreamId}
              placeholder={t("providers.selectDirectUpstream", {
                defaultValue: "请选择直连上游...",
              })}
              options={directOptions}
              onChange={(value) => handleSelectDirectUpstream(String(value))}
            />
          ) : null}
        </div>

        {query.error ? (
          <Alert type="error" showIcon message={errMsg(query.error)} />
        ) : null}

        {/* 配置同步与漂移状态行：对齐 AgentConfigDrift */}
        <Space size={4} wrap>
          <Tag
            color={
              driftStatus === "drifted"
                ? "warning"
                : driftStatus === "in_sync"
                  ? "success"
                  : "default"
            }
          >
            {t(`configDrift.${driftStatus}`)}
          </Tag>
          {isManaged ? (
            <>
              <Button
                size="small"
                type="link"
                disabled={busy}
                onClick={() => {
                  void query.refetch();
                  setPreviewOpen(true);
                }}
              >
                {t("configDrift.inspect")}
              </Button>
              <Button
                size="small"
                type="link"
                loading={busy}
                onClick={() => void resyncCurrent()}
              >
                {t("copilot.resync")}
              </Button>
            </>
          ) : null}
        </Space>
      </div>

      {/* 折叠式高级设置 */}
      {advancedOpen ? (
        <div style={{ marginTop: 10 }}>
          <Space direction="vertical" size={10} style={{ width: "100%" }}>
            <Paragraph type="secondary" style={{ marginBottom: 0, fontSize: 12 }}>
              {t("copilot.desc")}
            </Paragraph>
            {status ? (
              <Text type="secondary" style={{ fontSize: 12, overflowWrap: "anywhere" }}>
                {t("copilot.path")}：{status.configPath}
              </Text>
            ) : null}
            {status?.vscodePath ? (
              <Text type="secondary" style={{ fontSize: 12, overflowWrap: "anywhere" }}>
                {t("copilot.vscodePath", { defaultValue: "VS Code 路径" })}：{status.vscodePath}
              </Text>
            ) : null}
            {status?.copilotExtensionInstalled !== undefined ? (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {t("copilot.extensionStatus", { defaultValue: "Copilot 扩展" })}：
                {status.copilotExtensionInstalled
                  ? t("copilot.extensionInstalled", { defaultValue: "已安装" })
                  : t("copilot.extensionMissing", {
                      defaultValue: "未检测到 GitHub Copilot Chat 扩展（可在 VS Code 扩展市场安装）",
                    })}
              </Text>
            ) : null}
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("copilot.tokenNote")}
            </Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("copilot.reload")}
            </Text>
            <Space wrap>
              <Button
                size="small"
                disabled={busy}
                loading={query.isFetching && !busy}
                onClick={() => void query.refetch()}
              >
                {t("agentHub.detect", { defaultValue: "检测" })}
              </Button>
              <Button
                type="primary"
                size="small"
                loading={busy}
                onClick={() => void resyncCurrent()}
              >
                {isManaged ? t("copilot.resync") : t("copilot.apply")}
              </Button>
              {isManaged ? (
                <Button
                  size="small"
                  danger
                  disabled={busy}
                  onClick={() => setRemoveOpen(true)}
                >
                  {t("copilot.remove")}
                </Button>
              ) : null}
            </Space>
          </Space>
        </div>
      ) : null}

      {/* 配置漂移检查与重新应用弹窗（对齐 AgentConfigDrift） */}
      <Modal
        open={previewOpen}
        title={t("configDrift.title")}
        onCancel={() => !busy && setPreviewOpen(false)}
        onOk={async () => {
          await resyncCurrent();
          setPreviewOpen(false);
        }}
        okText={t("configDrift.reapply")}
        cancelText={t("common.cancel")}
        confirmLoading={busy}
        okButtonProps={{ disabled: !isManaged }}
        closable={!busy}
        maskClosable={!busy}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Alert type="warning" showIcon message={t("configDrift.warning")} />
          <Paragraph style={{ marginBottom: 0 }}>
            {t("copilot.driftScope")}
          </Paragraph>
          <Table
            size="small"
            rowKey="field"
            pagination={false}
            dataSource={status?.drift ?? []}
            locale={{ emptyText: t("configDrift.noChanges") }}
            columns={[
              {
                title: t("configDrift.field"),
                dataIndex: "field",
                render: (value: string) => (
                  <span style={{ overflowWrap: "anywhere" }}>{value}</span>
                ),
              },
              {
                title: t("configDrift.appliedValue"),
                dataIndex: "appliedPresent",
                render: (present: boolean) =>
                  t(present ? "configDrift.present" : "configDrift.absent"),
              },
              {
                title: t("configDrift.currentValue"),
                dataIndex: "currentPresent",
                render: (present: boolean) =>
                  t(present ? "configDrift.present" : "configDrift.absent"),
              },
            ]}
          />
        </Space>
      </Modal>

      {/* 禁用 Modal.confirm：用受控 Modal 做移除二次确认。 */}
      <Modal
        open={removeOpen}
        title={t("copilot.removeTitle")}
        okText={t("copilot.remove")}
        okButtonProps={{ danger: true }}
        cancelText={t("common.cancel")}
        confirmLoading={busy}
        onCancel={() => !busy && setRemoveOpen(false)}
        onOk={async () => {
          await run(removeVscodeCopilot, "copilot.removed");
          setRemoveOpen(false);
        }}
      >
        <Text>{t("copilot.removeBody")}</Text>
      </Modal>
    </Card>
  );
}

