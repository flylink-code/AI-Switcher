import {
  Alert,
  Button,
  Card,
  Popconfirm,
  Select,
  Space,
  Tooltip,
  Typography,
} from "antd";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import CopyOutlined from "@ant-design/icons/es/icons/CopyOutlined";
import EditOutlined from "@ant-design/icons/es/icons/EditOutlined";
import DeleteOutlined from "@ant-design/icons/es/icons/DeleteOutlined";
import { useTranslation } from "react-i18next";
import type { GatewayProfile } from "@/types/backend";

const { Text } = Typography;

const SHARED_PROFILE_ID = "gprof_shared";

export interface GatewayProfileToolbarProps {
  profileId: string;
  profiles: GatewayProfile[];
  editingProfile?: GatewayProfile;
  profileUsers: string[];
  profileBusy: boolean;
  hasBindings: boolean;
  onSelectProfile: (id: string) => void;
  onCreateProfile: () => void;
  onCloneProfile: () => void;
  onRenameProfile: () => void;
  onDeleteProfile: () => void;
  onUpdateFallbackMode: (mode: string) => void;
}

export function GatewayProfileToolbar({
  profileId,
  profiles,
  editingProfile,
  profileUsers,
  profileBusy,
  hasBindings,
  onSelectProfile,
  onCreateProfile,
  onCloneProfile,
  onRenameProfile,
  onDeleteProfile,
  onUpdateFallbackMode,
}: GatewayProfileToolbarProps) {
  const { t } = useTranslation();

  const profileOptions = profiles.map((profile) => ({
    value: profile.id,
    label:
      profile.id === SHARED_PROFILE_ID
        ? t("gateway.profileDefault", { defaultValue: profile.name || "默认" })
        : profile.name.trim() || profile.id,
  }));

  return (
    <Card size="small">
      <Space wrap style={{ width: "100%", justifyContent: "space-between" }}>
        <Space wrap>
          <Text type="secondary">{t("gateway.profileToolbar", { defaultValue: "正在编辑" })}</Text>
          <Select
            size="small"
            style={{ minWidth: 180 }}
            value={profileId}
            options={profileOptions}
            onChange={(value) => onSelectProfile(String(value))}
          />
          <Button
            size="small"
            icon={<PlusOutlined />}
            onClick={onCreateProfile}
          >
            {t("gateway.profileCreate", { defaultValue: "新建" })}
          </Button>
          <Button
            size="small"
            icon={<CopyOutlined />}
            onClick={onCloneProfile}
          >
            {t("gateway.profileClone", { defaultValue: "复制" })}
          </Button>
          <Button
            size="small"
            icon={<EditOutlined />}
            onClick={onRenameProfile}
          >
            {t("gateway.profileRename", { defaultValue: "重命名" })}
          </Button>
          <Popconfirm
            title={t("gateway.profileDeleteConfirm", {
              defaultValue: "删除这套档案？已绑定的 Agent 会回到默认档案。",
            })}
            disabled={profileId === SHARED_PROFILE_ID}
            onConfirm={onDeleteProfile}
          >
            <Button
              size="small"
              danger
              icon={<DeleteOutlined />}
              disabled={profileId === SHARED_PROFILE_ID}
              loading={profileBusy}
            >
              {t("gateway.profileDelete", { defaultValue: "删除" })}
            </Button>
          </Popconfirm>
        </Space>
        <Space wrap align="center">
          <Text type="secondary">{t("gateway.profileFallbackMode", { defaultValue: "故障转移" })}</Text>
          <Tooltip
            title={t("gateway.profileFallbackModeHint", {
              defaultValue: "档案故障转移开关不关闭模式中显式配置的备用模型",
            })}
          >
            <Select
              size="small"
              style={{ minWidth: 160 }}
              value={editingProfile?.fallbackMode || "off"}
              options={[
                { value: "off", label: t("gateway.fallbackModeOff", { defaultValue: "关闭 (off)" }) },
                { value: "retry", label: t("gateway.fallbackModeRetry", { defaultValue: "同模型重试 (retry)" }) },
                { value: "model_chain", label: t("gateway.fallbackModeModelChain", { defaultValue: "备用链 (model_chain)" }) },
              ]}
              onChange={(value) => onUpdateFallbackMode(String(value))}
            />
          </Tooltip>
        </Space>
      </Space>
      <Text type="secondary" style={{ display: "block", marginTop: 8 }}>
        {t("gateway.profileToolbarHint", {
          defaultValue: "这里改的是档案内容，和各 Agent Auto 卡选用哪一套无关。",
        })}
      </Text>
      <Text type="secondary" style={{ display: "block", marginTop: 2, fontSize: 12 }}>
        {t("gateway.profileFallbackModeHint", {
          defaultValue: "档案故障转移开关不关闭模式中显式配置的备用模型",
        })}
      </Text>
      {profileUsers.length > 0 ? (
        <Text type="secondary" style={{ display: "block", marginTop: 4 }}>
          {t("gateway.profileToolbarUsers", {
            agents: profileUsers.join("、"),
            defaultValue: "已绑定并选用：{{agents}}",
          })}
        </Text>
      ) : hasBindings ? (
        <Alert
          style={{ marginTop: 8 }}
          type="warning"
          showIcon
          message={t("gateway.profileToolbarUnused", {
            defaultValue: "没有任何已绑定 Agent 选用这套档案。未绑定的请求走默认档案，不会因为这里「正在编辑」而切换。",
          })}
        />
      ) : null}
    </Card>
  );
}
