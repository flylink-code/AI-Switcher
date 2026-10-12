import { Space } from "antd";
import { useTranslation } from "react-i18next";
import { DetailDrawer } from "@/components/common/DetailDrawer";
import AntigravityPage from "@/pages/AntigravityPage";
import KiroPage from "@/pages/KiroPage";
import { ChatGptSection } from "@/pages/providers/ChatGptSection";
import { SmartGatewaySection } from "@/pages/providers/SmartGatewaySection";
import { useProviderAccountsStore } from "@/stores/providerAccountsStore";
import type { ProviderAccountKind } from "@/stores/providerAccountsStore";

export default function ProvidersPage() {
  const { t } = useTranslation();
  const drawer = useProviderAccountsStore((state) => state.drawer);
  const closeDrawer = useProviderAccountsStore((state) => state.closeDrawer);

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      <SmartGatewaySection />
      <DetailDrawer
        open={drawer != null}
        title={drawer ? accountDrawerTitle(drawer, t) : ""}
        onClose={closeDrawer}
        width={720}
      >
        {drawer ? <AccountDrawerBody kind={drawer} /> : null}
      </DetailDrawer>
    </Space>
  );
}

function accountDrawerTitle(kind: ProviderAccountKind, t: ReturnType<typeof useTranslation>["t"]): string {
  switch (kind) {
    case "antigravity":
      return t("gateway.reverseAntigravity", { defaultValue: "Antigravity" });
    case "kiro":
      return t("gateway.reverseKiro", { defaultValue: "Kiro" });
    case "chatgpt":
      return "ChatGPT / Codex";
    default: {
      const _exhaustive: never = kind;
      return _exhaustive;
    }
  }
}

function AccountDrawerBody({ kind }: { kind: ProviderAccountKind }) {
  switch (kind) {
    case "antigravity":
      return <AntigravityPage embedded />;
    case "kiro":
      return <KiroPage />;
    case "chatgpt":
      return <ChatGptSection />;
    default: {
      const _exhaustive: never = kind;
      return _exhaustive;
    }
  }
}
