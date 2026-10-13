import React, { useEffect, useRef, useState } from "react";
import { Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import ClusterOutlined from "@ant-design/icons/es/icons/ClusterOutlined";
import DashboardOutlined from "@ant-design/icons/es/icons/DashboardOutlined";
import FolderOutlined from "@ant-design/icons/es/icons/FolderOutlined";
import RobotOutlined from "@ant-design/icons/es/icons/RobotOutlined";
import SettingOutlined from "@ant-design/icons/es/icons/SettingOutlined";
import type { PageKey } from "@/lib/pageRegistry";
import { useThemeStore } from "@/stores/themeStore";

export type V2MainTab =
  | "workbench"
  | "agentHub"
  | "providers"
  | "usage"
  | "workspace"
  | "settings";

export interface TopNavigationProps {
  activeKey: PageKey;
  onNavigate: (key: PageKey) => void;
  /** Force compact (icon-only). When omitted, auto-detects available width. */
  compact?: boolean;
}

/** Full labels for the six primary items. Narrower slots fall back to icons. */
const COMPACT_BREAKPOINT = 760;

export const TopNavigation: React.FC<TopNavigationProps> = ({
  activeKey,
  onNavigate,
  compact: compactProp,
}) => {
  const { t } = useTranslation();
  const resolvedTheme = useThemeStore((s) => s.resolved);
  const hostRef = useRef<HTMLDivElement>(null);
  const [autoCompact, setAutoCompact] = useState(false);

  useEffect(() => {
    if (compactProp != null) return;
    const host = hostRef.current;
    if (!host) return;

    const measure = () => {
      // Measure the header center slot — NOT the shrink-wrapped nav parent,
      // or compact mode latches permanently once labels hide.
      const slot =
        host.closest(".v2-top-nav-slot") ??
        host.parentElement?.parentElement ??
        host.parentElement;
      const available = slot?.clientWidth ?? 0;
      setAutoCompact(available > 0 && available < COMPACT_BREAKPOINT);
    };

    measure();
    const slot =
      host.closest(".v2-top-nav-slot") ??
      host.parentElement?.parentElement ??
      host.parentElement;
    const observer = new ResizeObserver(measure);
    if (slot) observer.observe(slot);
    window.addEventListener("resize", measure);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [compactProp]);

  const compact = compactProp ?? autoCompact;

  const getActiveTab = (key: PageKey): V2MainTab => {
    if (key === "workbench") return "workbench";
    if (key === "agentHub" || key === "agentTools") return "agentHub";
    if (key === "providers" || key === "gateway" || key === "proxy" || key === "antigravity") {
      return "providers";
    }
    if (key === "usage") return "usage";
    if (
      key === "workspace" ||
      key === "mcp" ||
      key === "prompts" ||
      key === "skills" ||
      key === "agents" ||
      key === "plugins" ||
      key === "profiles" ||
      key === "sessions"
    ) {
      return "workspace";
    }
    if (
      key === "settings" ||
      key === "environment" ||
      key === "about" ||
      key === "localProxy"
    ) {
      return "settings";
    }
    return "workbench";
  };

  const currentTab = getActiveTab(activeKey);

  const navItems: Array<{ key: V2MainTab; label: string; icon: React.ReactNode; targetPage: PageKey }> = [
    {
      key: "workbench",
      label: t("navigation.dashboard", { defaultValue: "概览" }),
      icon: <DashboardOutlined />,
      targetPage: "workbench",
    },
    {
      key: "agentHub",
      label: t("navigation.agentHub", { defaultValue: "Agent" }),
      icon: <RobotOutlined />,
      targetPage: "agentHub",
    },
    {
      key: "providers",
      label: t("navigation.providers", { defaultValue: "供应商" }),
      icon: <ClusterOutlined />,
      targetPage: "providers",
    },
    {
      key: "usage",
      label: t("navigation.usage", { defaultValue: "用量统计" }),
      icon: <BarChartOutlined />,
      targetPage: "usage",
    },
    {
      key: "workspace",
      label: t("navigation.workspace", { defaultValue: "工作区" }),
      icon: <FolderOutlined />,
      targetPage: "workspace",
    },
    {
      key: "settings",
      label: t("navigation.settings", { defaultValue: "设置" }),
      icon: <SettingOutlined />,
      targetPage: "settings",
    },
  ];

  const isDark = resolvedTheme === "dark";

  return (
    <div
      ref={hostRef}
      className="v2-top-nav"
      style={{
        display: "inline-flex",
        alignItems: "center",
        padding: "3px",
        borderRadius: "999px",
        backgroundColor: isDark ? "#171C24" : "#F3F4F6",
        border: `1px solid ${isDark ? "#222A35" : "#E5E7EB"}`,
        gap: compact ? "1px" : "2px",
        maxWidth: "100%",
        flexShrink: 0,
      }}
    >
      {navItems.map((item) => {
        const isActive = currentTab === item.key;
        const button = (
          <button
            type="button"
            onClick={() => onNavigate(item.targetPage)}
            aria-label={item.label}
            aria-current={isActive ? "page" : undefined}
            style={{
              display: "inline-flex",
              alignItems: "center",
              justifyContent: "center",
              gap: compact ? 0 : "5px",
              padding: compact ? "6px 10px" : "6px 10px",
              borderRadius: "999px",
              fontSize: "13px",
              fontWeight: isActive ? 600 : 500,
              border: "none",
              cursor: "pointer",
              transition: "background-color 0.15s ease, color 0.15s ease, box-shadow 0.15s ease",
              backgroundColor: isActive ? (isDark ? "#F8FAFC" : "#111827") : "transparent",
              color: isActive
                ? isDark
                  ? "#111827"
                  : "#FFFFFF"
                : isDark
                  ? "#9CA3AF"
                  : "#5B6474",
              boxShadow: isActive ? "0 1px 3px rgba(0,0,0,0.1)" : "none",
              flexShrink: 0,
              whiteSpace: "nowrap",
              lineHeight: 1.2,
            }}
          >
            {item.icon}
            {!compact ? <span>{item.label}</span> : null}
          </button>
        );

        if (compact) {
          return (
            <Tooltip key={item.key} title={item.label} mouseEnterDelay={0.25}>
              {button}
            </Tooltip>
          );
        }
        return <React.Fragment key={item.key}>{button}</React.Fragment>;
      })}
    </div>
  );
};
