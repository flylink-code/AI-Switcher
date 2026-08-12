import React from "react";
import { Segmented, Tooltip } from "antd";
import { useTranslation } from "react-i18next";
import MenuOutlined from "@ant-design/icons/es/icons/MenuOutlined";
import PicCenterOutlined from "@ant-design/icons/es/icons/PicCenterOutlined";
import { useAppStore, type LayoutMode } from "@/stores/appStore";

export interface LayoutModeSwitcherProps {
  /** Compact icon-only segmented control for the title bar. */
  size?: "small" | "middle";
}

/**
 * Title-bar control to switch chrome layout — left sidebar vs top navigation —
 * similar to a browser's default tabs vs vertical tabs.
 */
export const LayoutModeSwitcher: React.FC<LayoutModeSwitcherProps> = ({ size = "small" }) => {
  const { t } = useTranslation();
  const layoutMode = useAppStore((s) => s.layoutMode);
  const setLayoutMode = useAppStore((s) => s.setLayoutMode);

  const sidebarLabel = t("common.layoutSidebar", { defaultValue: "左侧导航" });
  const topLabel = t("common.layoutTop", { defaultValue: "顶部导航" });

  return (
    <Tooltip
      title={t("common.layoutModeHint", {
        defaultValue: "切换导航布局：左侧栏 / 顶部栏",
      })}
      mouseEnterDelay={0.35}
    >
      <Segmented<LayoutMode>
        size={size}
        value={layoutMode}
        onChange={(value) => setLayoutMode(value)}
        options={[
          {
            value: "sidebar",
            icon: <MenuOutlined aria-label={sidebarLabel} />,
            title: sidebarLabel,
          },
          {
            value: "top",
            icon: <PicCenterOutlined aria-label={topLabel} />,
            title: topLabel,
          },
        ]}
        aria-label={t("common.layoutMode", { defaultValue: "导航布局" })}
      />
    </Tooltip>
  );
};
