import { useEffect, useRef, useState } from "react";
import { Segmented } from "antd";
import AppstoreOutlined from "@ant-design/icons/es/icons/AppstoreOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import { useTranslation } from "react-i18next";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

export function FloatingViewSwitcher() {
  const { t } = useTranslation();
  const currentView = usePagePreferencesStore((state) => state.workbenchView);
  const setView = usePagePreferencesStore((state) => state.setWorkbenchView);

  const [visible, setVisible] = useState(true);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isHoveredRef = useRef(false);

  const resetHideTimer = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    // 鼠标在触发区外时，1.5s 后淡出隐藏
    timerRef.current = setTimeout(() => {
      if (!isHoveredRef.current) {
        setVisible(false);
      }
    }, 1500);
  };

  useEffect(() => {
    // 初始显示 2.5 秒后自动隐入底栏，提示用户其存在
    const initialTimer = setTimeout(() => {
      if (!isHoveredRef.current) {
        setVisible(false);
      }
    }, 2500);

    const handleMouseMove = (e: MouseEvent) => {
      const windowHeight = window.innerHeight;
      // 当鼠标移动到页面底部 90px 区域内时唤出
      if (windowHeight - e.clientY < 90) {
        setVisible(true);
        resetHideTimer();
      }
    };

    window.addEventListener("mousemove", handleMouseMove);
    return () => {
      clearTimeout(initialTimer);
      if (timerRef.current) clearTimeout(timerRef.current);
      window.removeEventListener("mousemove", handleMouseMove);
    };
  }, []);

  return (
    <div
      style={{
        position: "fixed",
        bottom: 24,
        left: "50%",
        transform: visible
          ? "translateX(-50%) translateY(0)"
          : "translateX(-50%) translateY(24px)",
        opacity: visible ? 1 : 0,
        pointerEvents: visible ? "auto" : "none",
        transition: "all 0.3s cubic-bezier(0.4, 0, 0.2, 1)",
        zIndex: 100,
      }}
      onMouseEnter={() => {
        isHoveredRef.current = true;
        setVisible(true);
        if (timerRef.current) clearTimeout(timerRef.current);
      }}
      onMouseLeave={() => {
        isHoveredRef.current = false;
        resetHideTimer();
      }}
    >
      <div className="floating-view-switcher">
        <Segmented
          value={currentView}
          onChange={(value) => setView(value as "providers" | "usage")}
          options={[
            {
              label: (
                <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 8px" }}>
                  <AppstoreOutlined />
                  <span>{t("workbench.viewProviders", "供应商服务")}</span>
                </div>
              ),
              value: "providers",
            },
            {
              label: (
                <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 8px" }}>
                  <BarChartOutlined />
                  <span>{t("workbench.viewUsage", "用量与统计")}</span>
                </div>
              ),
              value: "usage",
            },
          ]}
          style={{
            background: "transparent",
            borderRadius: 24,
          }}
        />
      </div>
    </div>
  );
}
