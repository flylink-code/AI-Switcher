import { Modal, Space, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { CodexOauthDeviceStart } from "@/types/backend";

const { Text } = Typography;

export interface CodexOauthModalProps {
  device: CodexOauthDeviceStart | null;
  onClose: () => void;
}

export function CodexOauthModal({ device, onClose }: CodexOauthModalProps) {
  const { t } = useTranslation();

  return (
    <Modal
      open={Boolean(device)}
      title={t("providers.chatgptLogin", { defaultValue: "ChatGPT / Codex OAuth 登录" })}
      onCancel={onClose}
      footer={null}
      destroyOnHidden
    >
      <Space direction="vertical" size="middle" style={{ width: "100%", padding: "12px 0" }}>
        <Text>
          {t("providers.chatgptLoginPrompt", {
            defaultValue: "请在打开的浏览器页面中确认授权，输入以下设备代码：",
          })}
        </Text>
        <div
          style={{
            textAlign: "center",
            padding: "16px",
            background: "var(--ant-color-fill-quaternary, rgba(0,0,0,0.04))",
            borderRadius: 8,
          }}
        >
          <Text strong copyable style={{ fontSize: 24, letterSpacing: 2 }}>
            {device?.userCode}
          </Text>
        </div>
        <Text type="secondary" style={{ fontSize: 12 }}>
          {t("providers.chatgptLoginPolling", {
            defaultValue: "正在等待授权完成，请勿关闭此窗口...",
          })}
        </Text>
      </Space>
    </Modal>
  );
}
