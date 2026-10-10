import { Checkbox, Modal, Select, Space, Typography } from "antd";
import { useTranslation } from "react-i18next";
import { filterUiAgents } from "@/lib/agentVisibility";
import { LABEL_KEYS, PROVIDER_TARGET_OPTIONS } from "@/components/AgentTargetSwitcher";
import type { Provider, ProviderTarget } from "@/types/backend";

const { Text } = Typography;

export interface UpstreamImportModalProps {
  open: boolean;
  importTarget: ProviderTarget;
  importIds: string[];
  importAllowlist: boolean;
  importing: boolean;
  importableProviders: Provider[];
  onTargetChange: (target: ProviderTarget) => void;
  onIdsChange: (ids: string[]) => void;
  onAllowlistChange: (checked: boolean) => void;
  onSubmit: () => void;
  onClose: () => void;
}

export function UpstreamImportModal({
  open,
  importTarget,
  importIds,
  importAllowlist,
  importing,
  importableProviders,
  onTargetChange,
  onIdsChange,
  onAllowlistChange,
  onSubmit,
  onClose,
}: UpstreamImportModalProps) {
  const { t } = useTranslation();

  return (
    <Modal
      open={open}
      title={t("proxy.importFromProviders", { defaultValue: "从已有 Agent 导入" })}
      onCancel={onClose}
      onOk={onSubmit}
      confirmLoading={importing}
      destroyOnHidden
    >
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <div>
          <Text type="secondary" style={{ display: "block", marginBottom: 8, fontSize: 12 }}>
            {t("proxy.importUpstreamHint", {
              defaultValue: "选择一个 Agent，勾选要加入全局上游池的供应商卡片：",
            })}
          </Text>
          <Select
            style={{ width: "100%" }}
            value={importTarget}
            onChange={(value) => {
              onTargetChange(value);
              onIdsChange([]);
            }}
            options={filterUiAgents(PROVIDER_TARGET_OPTIONS).map((item) => ({
              value: item,
              label: t(LABEL_KEYS[item] ?? `workspace.${item}`),
            }))}
          />
        </div>
        <Checkbox.Group
          style={{ display: "flex", flexDirection: "column", gap: 8 }}
          value={importIds}
          onChange={(values) => onIdsChange(values.map(String))}
          options={importableProviders.map((item) => ({
            value: item.id,
            label: `${item.name} · ${item.baseUrl}`,
          }))}
        />
        {importableProviders.length === 0 && (
          <Text type="secondary">{t("proxy.importUpstreamEmpty", { defaultValue: "没有可导入的供应商" })}</Text>
        )}
        <Checkbox checked={importAllowlist} onChange={(event) => onAllowlistChange(event.target.checked)}>
          {t("proxy.importAddAllowlist", { defaultValue: "导入后自动加入默认档案的允许列表" })}
        </Checkbox>
      </Space>
    </Modal>
  );
}
