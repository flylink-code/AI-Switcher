import { Segmented, Space } from "antd";
import { useTranslation } from "react-i18next";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import AntigravityPage from "@/pages/AntigravityPage";
import KiroPage from "@/pages/KiroPage";
import { ChatGptSection } from "@/pages/providers/ChatGptSection";
import { SmartGatewaySection } from "@/pages/providers/SmartGatewaySection";
import type { ProvidersTab, ReverseGatewayTab } from "@/stores/pagePreferencesStore";

export default function ProvidersPage() {
  const { t } = useTranslation();
  const tab = usePagePreferencesStore((state) => state.providersTab);
  const setTab = usePagePreferencesStore((state) => state.setProvidersTab);
  const reverseTab = usePagePreferencesStore((state) => state.reverseTab);
  const setReverseTab = usePagePreferencesStore((state) => state.setReverseTab);

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <Segmented<ProvidersTab>
        value={tab}
        onChange={setTab}
        options={[
          { value: "smart", label: t("gateway.tabSmart", { defaultValue: "智能网关" }) },
          { value: "reverse", label: t("gateway.tabAntigravity", { defaultValue: "反代网关" }) },
        ]}
      />
      {tab === "smart" ? (
        <SmartGatewaySection />
      ) : (
        <Space direction="vertical" size={16} style={{ width: "100%" }}>
          <Segmented<ReverseGatewayTab>
            size="small"
            value={reverseTab}
            onChange={setReverseTab}
            options={[
              { value: "antigravity", label: t("gateway.reverseAntigravity", { defaultValue: "Antigravity" }) },
              { value: "kiro", label: t("gateway.reverseKiro", { defaultValue: "Kiro" }) },
              { value: "chatgpt", label: "ChatGPT" },
            ]}
          />
          <ReverseBody tab={reverseTab} />
        </Space>
      )}
    </Space>
  );
}

function ReverseBody({ tab }: { tab: ReverseGatewayTab }) {
  switch (tab) {
    case "antigravity":
      return <AntigravityPage embedded />;
    case "kiro":
      return <KiroPage />;
    case "chatgpt":
      return <ChatGptSection />;
    default: {
      const _exhaustive: never = tab;
      return _exhaustive;
    }
  }
}
