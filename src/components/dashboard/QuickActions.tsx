import React from "react";
import { Button, Typography } from "antd";
import AppstoreOutlined from "@ant-design/icons/es/icons/AppstoreOutlined";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import ApiOutlined from "@ant-design/icons/es/icons/ApiOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import FolderOutlined from "@ant-design/icons/es/icons/FolderOutlined";
import { useTranslation } from "react-i18next";
import { Surface, Inline, Stack } from "@/components/ui";
import { useNavigatePage } from "@/lib/navigation";

const { Text } = Typography;

export interface QuickActionsProps {
  className?: string;
  style?: React.CSSProperties;
}

export const QuickActions: React.FC<QuickActionsProps> = ({
  className = "",
  style,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigatePage();

  const actions = [
    { key: "providers", label: t("navigation.providers", { defaultValue: "管理供应商" }), icon: <ClusterOutlined />, page: "providers" as const },
    { key: "proxy", label: t("navigation.proxy", { defaultValue: "代理控制" }), icon: <ApiOutlined />, page: "proxy" as const },
    { key: "usage", label: t("navigation.usage", { defaultValue: "查看用量" }), icon: <BarChartOutlined />, page: "usage" as const },
    { key: "antigravity", label: t("navigation.accounts", { defaultValue: "账号与额度" }), icon: <UserOutlined />, page: "antigravity" as const },
    { key: "workspace", label: t("navigation.workspace", { defaultValue: "工作区资源" }), icon: <FolderOutlined />, page: "mcp" as const },
  ];

  return (
    <Surface padding="md" className={className} style={style}>
      <Stack gap="sm">
        <Inline gap="sm">
          <AppstoreOutlined style={{ fontSize: 18, color: "var(--color-brand)" }} />
          <Text strong style={{ fontSize: "var(--font-size-md)" }}>
            {t("dashboard.quickActionsTitle", { defaultValue: "快捷导航 (Quick Actions)" })}
          </Text>
        </Inline>

        <Inline gap="sm" wrap>
          {actions.map((action) => (
            <Button
              key={action.key}
              size="small"
              icon={action.icon}
              onClick={() => navigate(action.page)}
            >
              {action.label}
            </Button>
          ))}
        </Inline>
      </Stack>
    </Surface>
  );
};
