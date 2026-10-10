import { Button, Drawer, Space, Switch, Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { GatewayUpstreamModelRow, Provider } from "@/types/backend";

const { Text } = Typography;

export interface UpstreamModelsDrawerProps {
  upstream: Provider | null;
  models: GatewayUpstreamModelRow[];
  saving: boolean;
  onClose: () => void;
  onRefresh: () => void;
  onToggleModel: (modelId: string, visible: boolean) => void;
}

export function UpstreamModelsDrawer({
  upstream,
  models,
  saving,
  onClose,
  onRefresh,
  onToggleModel,
}: UpstreamModelsDrawerProps) {
  const { t } = useTranslation();

  return (
    <Drawer
      title={
        upstream
          ? t("proxy.upstreamModelsTitle", { name: upstream.name })
          : t("proxy.upstreamModels")
      }
      open={Boolean(upstream)}
      onClose={onClose}
      width={420}
      extra={
        <Button size="small" loading={saving} onClick={onRefresh}>
          {t("proxy.refreshModels")}
        </Button>
      }
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("proxy.upstreamModelsHint")}
      </Text>
      <Space direction="vertical" size="small" style={{ width: "100%" }}>
        {models.map((row) => {
          const isDefault =
            Boolean(upstream) &&
            row.modelId.trim().toLowerCase() === (upstream?.model ?? "").trim().toLowerCase();
          return (
            <Space
              key={row.modelId}
              align="center"
              style={{ width: "100%", justifyContent: "space-between" }}
            >
              <Text ellipsis style={{ maxWidth: 260 }}>
                {row.modelId}
                {isDefault ? ` (${t("proxy.upstreamDefaultModel")})` : ""}
              </Text>
              <Switch
                size="small"
                checked={row.visible}
                disabled={isDefault || saving}
                onChange={(checked) => onToggleModel(row.modelId, checked)}
              />
            </Space>
          );
        })}
        {models.length === 0 && (
          <Text type="secondary">{t("proxy.upstreamModelsEmpty")}</Text>
        )}
      </Space>
    </Drawer>
  );
}
