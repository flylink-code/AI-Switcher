import { Segmented } from "antd";
import AppstoreOutlined from "@ant-design/icons/es/icons/AppstoreOutlined";
import BarChartOutlined from "@ant-design/icons/es/icons/BarChartOutlined";
import { useTranslation } from "react-i18next";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

export function FloatingViewSwitcher() {
  const { t } = useTranslation();
  const currentView = usePagePreferencesStore((state) => state.workbenchView);
  const setView = usePagePreferencesStore((state) => state.setWorkbenchView);

  return (
    <div
      style={{
        position: "fixed",
        bottom: 36,
        left: "50%",
        transform: "translateX(-50%)",
        zIndex: 100,
      }}
    >
      <div className="floating-view-switcher">
        <Segmented
          value={currentView}
          onChange={(value) => setView(value as "providers" | "usage")}
          options={[
            {
              label: (
                <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "3px 12px", fontSize: 13, fontWeight: 500 }}>
                  <AppstoreOutlined />
                  <span>{t("workbench.viewProviders", "供应商服务")}</span>
                </div>
              ),
              value: "providers",
            },
            {
              label: (
                <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "3px 12px", fontSize: 13, fontWeight: 500 }}>
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
