import { Result } from "antd";
import { ToolOutlined } from "@ant-design/icons";
import { useTranslation } from "react-i18next";

interface PlaceholderPageProps {
  /** Feature key, used to look up nav title. */
  feature: "providers" | "mcp" | "prompts" | "skills" | "usage";
  /** Rough phase letter shown in the subtitle. */
  phase: string;
}

export function PlaceholderPage({ feature, phase }: PlaceholderPageProps) {
  const { t } = useTranslation();
  return (
    <Result
      icon={<ToolOutlined />}
      status="info"
      title={t(`nav.${feature}`)}
      subTitle={
        <>
          <div>{t("placeholder.subtitle")}</div>
          <div style={{ marginTop: 8, color: "var(--ant-color-text-tertiary)" }}>
            {t("placeholder.p0")} — {phase}
          </div>
        </>
      }
      style={{ marginTop: 48 }}
    />
  );
}
