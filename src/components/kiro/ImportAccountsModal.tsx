import { useState } from "react";
import { Button, Input, Modal, Space, Typography } from "antd";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import { useTranslation } from "react-i18next";

const { TextArea } = Input;
const { Paragraph } = Typography;

interface ImportAccountsModalProps {
  open: boolean;
  onClose: () => void;
  onImport: (raw: string) => Promise<void>;
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
    const raw = importJson.trim();
    if (!raw) return;
    await onImport(raw);
    setImportJson("");
    onClose();
  };

  return (
    <Modal
      title={
        <Space>
          <ImportOutlined />
          <span>{t("kiro.import")}</span>
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
          onClick={() => void handleConfirm()}
        >
          {t("kiro.import")}
        </Button>,
      ]}
    >
      <Paragraph type="secondary">{t("kiro.importHint")}</Paragraph>
      <TextArea
        rows={8}
        value={importJson}
        onChange={(event) => setImportJson(event.target.value)}
        placeholder={t("kiro.importPlaceholder")}
      />
    </Modal>
  );
}
