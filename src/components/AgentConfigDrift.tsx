import { useState } from "react";
import { Alert, Button, Modal, Space, Table, Tag, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getAgentConfigDrift, reapplyAgentConfig } from "@/services/providers";
import type { ConfigDriftReport, ProviderTarget } from "@/types/backend";

export function AgentConfigDrift({ target }: { target: ProviderTarget }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [preview, setPreview] = useState<ConfigDriftReport | null>(null);
  const [busy, setBusy] = useState(false);
  const query = useQuery({
    queryKey: ["agent-config-drift", target],
    queryFn: () => getAgentConfigDrift(target),
    refetchInterval: 15_000,
  });
  const status = query.isError ? "unknown" : query.data?.status;

  const inspect = async () => {
    setBusy(true);
    try {
      setPreview(await getAgentConfigDrift(target));
    } catch (error) {
      void message.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!preview) return;
    setBusy(true);
    try {
      await reapplyAgentConfig(target, preview.revision);
      setPreview(null);
      void message.success(t("configDrift.applied"));
    } catch (error) {
      // 冲突后必须重新预览，不能沿用过期确认。
      setPreview(null);
      void message.error(String(error));
    } finally {
      setBusy(false);
      await queryClient.invalidateQueries({ queryKey: ["agent-config-drift", target] });
    }
  };

  return (
    <>
      <Space size={4} wrap>
        <Tag color={status === "drifted" ? "warning" : status === "in_sync" ? "success" : "default"}>
          {t(`configDrift.${status ?? "loading"}`)}
        </Tag>
        {status !== "unmanaged" && (
          <Button size="small" type="link" loading={busy} onClick={() => void inspect()}>
            {t("configDrift.inspect")}
          </Button>
        )}
      </Space>
      <Modal
        open={preview !== null}
        title={t("configDrift.title")}
        onCancel={() => !busy && setPreview(null)}
        onOk={() => void apply()}
        okText={t("configDrift.reapply")}
        cancelText={t("common.cancel")}
        confirmLoading={busy}
        okButtonProps={{ disabled: preview?.status === "unmanaged" }}
        closable={!busy}
        maskClosable={!busy}
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Alert type="warning" showIcon message={t("configDrift.warning")} />
          <Typography.Paragraph>{t("configDrift.scope")}</Typography.Paragraph>
          {preview?.status === "unknown" ? (
            <Alert type="info" message={t("configDrift.noBaseline")} />
          ) : (
            <Table
              size="small"
              rowKey="field"
              pagination={false}
              dataSource={preview?.fields ?? []}
              locale={{ emptyText: t("configDrift.noChanges") }}
              columns={[
                { title: t("configDrift.field"), dataIndex: "field", render: (value: string) => <span style={{ overflowWrap: "anywhere" }}>{value}</span> },
                { title: t("configDrift.appliedValue"), dataIndex: "appliedPresent", render: (present: boolean) => t(present ? "configDrift.present" : "configDrift.absent") },
                { title: t("configDrift.currentValue"), dataIndex: "currentPresent", render: (present: boolean) => t(present ? "configDrift.present" : "configDrift.absent") },
              ]}
            />
          )}
        </Space>
      </Modal>
    </>
  );
}
