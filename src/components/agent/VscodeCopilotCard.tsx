import { useState } from "react";
import {
  Alert,
  Button,
  Card,
  Modal,
  Radio,
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
  removeVscodeCopilot,
} from "@/services/providers";

const { Text, Paragraph } = Typography;
const QUERY_KEY = ["vscode-copilot-status"];

type CopilotConnectionMode = "gateway" | "official";

/**
 * VS Code Copilot 配置卡片。
 * 采用与其他 Agent 卡片一致的顶栏图标/状态、连接模式切换（智能网关 / 官方）、
 * 配置漂移检查/预览弹窗与折叠式「高级设置」布局。
 */
export function VscodeCopilotCard() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [removeOpen, setRemoveOpen] = useState(false);

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

  const status = query.data;
  const gatewayStatus = gatewayStatusQuery.data;
  const isManaged = Boolean(status?.bound || status?.applied);
  const drifted = isManaged && (!status?.applied || (status?.drift.length ?? 0) > 0);
  const currentMode: CopilotConnectionMode = isManaged ? "gateway" : "official";
  const gatewayPort = gatewayStatus?.port ?? 15828;
  const gatewayRunning = gatewayStatus?.running ?? status?.gatewayRunning ?? false;

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

  const handleModeChange = (nextMode: CopilotConnectionMode) => {
    if (nextMode === currentMode) return;
    if (nextMode === "gateway") {
      void run(applyVscodeCopilot, "copilot.applied");
    } else {
      setRemoveOpen(true);
    }
  };

  return (
    <Card size="small" className="page-surface">
      {/* 卡片顶栏：与 AgentCardFrame 保持一致 */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        {usageSourceIcon("vscode_copilot", { size: 18 })}
        <Text strong>{t("copilot.title")}</Text>
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
            value={currentMode}
            onChange={(event) =>
              handleModeChange(event.target.value as CopilotConnectionMode)
            }
            options={[
              {
                label: t("providers.modeGateway", { defaultValue: "智能网关" }),
                value: "gateway",
              },
              {
                label: t("providers.modeOfficial", { defaultValue: "官方" }),
                value: "official",
              },
            ]}
          />
          {currentMode === "gateway" ? (
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
                onClick={() => void run(applyVscodeCopilot, "copilot.applied")}
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
                onClick={() => void run(applyVscodeCopilot, "copilot.applied")}
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
          await run(applyVscodeCopilot, "copilot.applied");
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

