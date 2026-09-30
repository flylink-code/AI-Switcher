import { Button, Card, Space, Tag, Typography } from "antd";
import LinkOutlined from "@ant-design/icons/es/icons/LinkOutlined";
import CheckOutlined from "@ant-design/icons/es/icons/CheckOutlined";
import { useTranslation } from "react-i18next";
import { BIND_TARGETS } from "@/components/antigravity";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ProviderTarget } from "@/types/backend";

const { Text } = Typography;

interface BindAppsCardProps {
  boundMap?: Map<ProviderTarget, boolean>;
  onEnsureBind: (target: ProviderTarget) => void;
  bindingTarget?: ProviderTarget | null;
  accountCount: number;
}

export function BindAppsCard({
  boundMap,
  onEnsureBind,
  bindingTarget,
  accountCount,
}: BindAppsCardProps) {
  const { t } = useTranslation();
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);
  const activeTargets = BIND_TARGETS.filter((target) => visibleAgents.includes(target));

  return (
    <Card title={t("kiro.bindApps")} size="small">
      <Space direction="vertical" style={{ width: "100%" }} size={8}>
        <Text type="secondary">{t("kiro.bindAppsHint")}</Text>
        <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
          {activeTargets.map((target) => {
            const isBound = boundMap?.get(target) ?? false;
            const isBinding = bindingTarget === target;
            return (
              <Button
                key={target}
                size="small"
                icon={isBound ? <CheckOutlined /> : <LinkOutlined />}
                loading={isBinding}
                disabled={accountCount === 0}
                onClick={() => onEnsureBind(target)}
              >
                {t("kiro.bindApp", { app: t(`workspace.${target}`) })}
                {isBound ? (
                  <Tag color="green" style={{ marginLeft: 4, marginRight: 0 }}>
                    {t("kiro.bound")}
                  </Tag>
                ) : null}
              </Button>
            );
          })}
        </div>
        {accountCount === 0 ? (
          <Text type="danger" style={{ fontSize: 12 }}>
            {t("kiro.bindNeedsAccount")}
          </Text>
        ) : null}
      </Space>
    </Card>
  );
}
