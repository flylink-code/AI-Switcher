import { Result, Typography } from "antd";
import ToolOutlined from "@ant-design/icons/es/icons/ToolOutlined";
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
          <Typography.Text type="secondary" style={{ display: "block", marginTop: 8 }}>
            {t("placeholder.p0")} — {phase}
          </Typography.Text>
        </>
      }
      style={{ marginTop: 48 }}
    />
  );
}
