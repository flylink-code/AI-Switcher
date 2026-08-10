import { useState } from "react";
import { Modal, Input, Typography, Button, Space } from "antd";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import { useTranslation } from "react-i18next";

const { TextArea } = Input;
const { Paragraph, Text } = Typography;

interface ImportAccountsModalProps {
  open: boolean;
  onClose: () => void;
  onImport: (json: string) => Promise<void>;
  isImporting?: boolean;
}

export function ImportAccountsModal({
  open,
  onClose,
  onImport,
  isImporting = false,
}: ImportAccountsModalProps) {
  const { t } = useTranslation();
  const [importJson, setImportJson] = useState("");

  const handleConfirm = async () => {
    if (!importJson.trim()) return;
    await onImport(importJson);
    setImportJson("");
    onClose();
  };

  return (
    <Modal
      title={
        <Space>
          <ImportOutlined />
          <span>{t("antigravity.import")}</span>
        </Space>
      }
      open={open}
      onCancel={onClose}
      footer={[
        <Button key="cancel" onClick={onClose}>
          {t("common.cancel")}
        </Button>,
        <Button
          key="import"
          type="primary"
          loading={isImporting}
          disabled={!importJson.trim()}
          onClick={handleConfirm}
        >
          {t("antigravity.import")}
        </Button>,
      ]}
    >
      <Paragraph type="secondary">
        {t("antigravity.importOptional")}
      </Paragraph>
      <TextArea
        rows={6}
        value={importJson}
        onChange={(event) => setImportJson(event.target.value)}
        placeholder={t("antigravity.importPlaceholder")}
      />
    </Modal>
  );
}
