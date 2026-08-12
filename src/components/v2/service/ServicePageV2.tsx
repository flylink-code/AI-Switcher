import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import AppstoreOutlined from "@ant-design/icons/es/icons/AppstoreOutlined";
import CloudServerOutlined from "@ant-design/icons/es/icons/CloudServerOutlined";
import UserOutlined from "@ant-design/icons/es/icons/UserOutlined";
import { useThemeStore } from "@/stores/themeStore";
import type { PageKey } from "@/lib/pageRegistry";
import ProvidersPage from "@/pages/ProvidersPage";
import AntigravityPage from "@/pages/AntigravityPage";
import WorkspacePage from "@/pages/WorkspacePage";

export type ServiceSubTab = "providers" | "accounts" | "workspace";

export interface ServicePageV2Props {
  initialTab?: ServiceSubTab;
  onNavigate?: (key: PageKey) => void;
}

const TAB_TO_PAGE: Record<ServiceSubTab, PageKey> = {
  providers: "providers",
  accounts: "antigravity",
  workspace: "workspace",
};

export const ServicePageV2: React.FC<ServicePageV2Props> = ({
  initialTab = "providers",
  onNavigate,
}) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const isDark = resolvedTheme === "dark";

  const [activeTab, setActiveTab] = useState<ServiceSubTab>(initialTab);

  useEffect(() => {
    setActiveTab(initialTab);
  }, [initialTab]);

  const subNavItems: Array<{ key: ServiceSubTab; label: string; icon: React.ReactNode }> = [
    {
      key: "providers",
      label: t("navigation.providers", { defaultValue: "供应商" }),
      icon: <CloudServerOutlined />,
    },
    {
      key: "accounts",
      label: t("navigation.accounts", { defaultValue: "账号与额度" }),
      icon: <UserOutlined />,
    },
    {
      key: "workspace",
      label: t("navigation.workspace", { defaultValue: "工作区" }),
      icon: <AppstoreOutlined />,
    },
  ];

  const handleTabChange = (tab: ServiceSubTab) => {
    setActiveTab(tab);
    onNavigate?.(TAB_TO_PAGE[tab]);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
      {/* Top Secondary Segmented Bar */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          paddingBottom: "8px",
          borderBottom: `1px solid ${isDark ? "#242D38" : "#E8ECF1"}`,
        }}
      >
        <div
          style={{
            display: "inline-flex",
            alignItems: "center",
            padding: "3px",
            borderRadius: "8px",
            backgroundColor: isDark ? "#151B23" : "#F4F6F8",
            gap: "2px",
          }}
        >
          {subNavItems.map((item) => {
            const isActive = activeTab === item.key;
            return (
              <button
                key={item.key}
                type="button"
                onClick={() => handleTabChange(item.key)}
                style={{
                  display: "inline-flex",
                  alignItems: "center",
                  gap: "6px",
                  padding: "5px 14px",
                  borderRadius: "6px",
                  fontSize: "12px",
                  fontWeight: isActive ? 600 : 500,
                  border: "none",
                  cursor: "pointer",
                  transition: "all 0.15s ease-in-out",
                  backgroundColor: isActive ? (isDark ? "#1A212B" : "#FFFFFF") : "transparent",
                  color: isActive
                    ? isDark
                      ? "#F2F4F7"
                      : "#111827"
                    : isDark
                      ? "#9CA3AF"
                      : "#6B7280",
                  boxShadow: isActive ? (isDark ? "none" : "0 1px 2px rgba(0,0,0,0.05)") : "none",
                }}
              >
                {item.icon}
                <span>{item.label}</span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Subtab Content Area */}
      <div style={{ minHeight: "500px" }}>
        {activeTab === "providers" && <ProvidersPage />}
        {activeTab === "accounts" && <AntigravityPage />}
        {activeTab === "workspace" && <WorkspacePage />}
      </div>
    </div>
  );
};
