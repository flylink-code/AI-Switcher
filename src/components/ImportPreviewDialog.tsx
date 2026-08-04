import { Alert, List, Modal, Space, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { ImportPreview } from "@/types/backend";

const { Text } = Typography;

interface ImportPreviewDialogProps {
  open: boolean;
  preview: ImportPreview | null;
  confirming?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function ImportPreviewDialog({
  open,
  preview,
  confirming = false,
  onCancel,
  onConfirm,
}: ImportPreviewDialogProps) {
  const { t } = useTranslation();
  if (!preview) return null;

  const resourceLabel =
    preview.resource === "provider"
      ? t("deeplink.resourceProvider")
      : t("deeplink.resourceMcp");

  return (
    <Modal
      open={open}
      title={t("deeplink.previewTitle")}
      okText={t("deeplink.confirmImport")}
      cancelText={t("common.cancel")}
      onCancel={onCancel}
      onOk={onConfirm}
      confirmLoading={confirming}
      destroyOnHidden
      width={560}
    >
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Text type="secondary">
          {t("deeplink.previewSummary", {
            resource: resourceLabel,
            count: preview.items.length,
            source: preview.source,
          })}
        </Text>
        {preview.warnings.length > 0 && (
          <Alert
            type="warning"
            showIcon
            message={t("deeplink.warningsTitle")}
            description={
              <ul style={{ margin: 0, paddingInlineStart: 18 }}>
                {preview.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            }
          />
        )}
        <List
          size="small"
          bordered
          dataSource={preview.items}
          renderItem={(item) => (
            <List.Item>
              <Space direction="vertical" size={0}>
                <Text strong>{item.name}</Text>
                <Text type="secondary">{item.summary}</Text>
              </Space>
            </List.Item>
          )}
        />
      </Space>
    </Modal>
  );
}
