import { Modal, Input, Space, Typography } from "antd";
import { useTranslation } from "react-i18next";

const { Text } = Typography;

export interface UpstreamImportJsonModalProps {
  open: boolean;
  text: string;
  loading: boolean;
  onTextChange: (text: string) => void;
  onSubmit: () => void;
  onClose: () => void;
}

export function UpstreamImportJsonModal({
  open,
  text,
  loading,
  onTextChange,
  onSubmit,
  onClose,
}: UpstreamImportJsonModalProps) {
  const { t } = useTranslation();

  return (
    <Modal
      open={open}
      title={t("proxy.importJsonTitle", { defaultValue: "导入全局上游 JSON" })}
      onCancel={onClose}
      onOk={onSubmit}
      confirmLoading={loading}
      destroyOnHidden
    >
      <Space direction="vertical" size="small" style={{ width: "100%" }}>
        <Text type="secondary" style={{ fontSize: 12 }}>
          {t("proxy.importJsonHint", {
            defaultValue: "粘贴此前导出的上游 JSON 内容进行批量导入。",
          })}
        </Text>
        <Input.TextArea
          rows={8}
          value={text}
          onChange={(e) => onTextChange(e.target.value)}
          placeholder='[{"name": "...", "baseUrl": "...", ...}]'
        />
      </Space>
    </Modal>
  );
}
