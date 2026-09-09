import { Button, Card, InputNumber, Select, Space, Switch, Typography, message } from "antd";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  getGatewayProfile,
  listGatewayCatalogEntries,
  listGatewayUpstreams,
  updateGatewayProfile,
} from "@/services/providers";
import type { GatewayCatalogModelOption, ProviderTarget } from "@/types/backend";

const { Text } = Typography;

function groupedCatalogOptions(entries: GatewayCatalogModelOption[]) {
  const groups = new Map<string, { label: string; value: string }[]>();
  for (const entry of entries) {
    const name = entry.providerName.trim() || entry.displayName;
    const list = groups.get(name) ?? [];
    list.push({ label: entry.displayName, value: entry.publicId });
    groups.set(name, list);
  }
  return [...groups.entries()].map(([label, options]) => ({ label, options }));
}

export function GatewayProfilePanel({ target }: { target: ProviderTarget }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const profileQuery = useQuery({
    queryKey: ["gateway-profile"],
    queryFn: () => getGatewayProfile(target),
  });
  const entriesQuery = useQuery({
    queryKey: ["gateway-catalog-entries", target],
    queryFn: () => listGatewayCatalogEntries(target),
  });
  const upstreamsQuery = useQuery({
    queryKey: ["gateway-upstreams"],
    queryFn: listGatewayUpstreams,
  });

  const profile = profileQuery.data;
  const modelOptions = groupedCatalogOptions(entriesQuery.data ?? []);
  const upstreamOptions = (upstreamsQuery.data ?? []).map((item) => ({
    label: item.name,
    value: item.id,
  }));

  const patch = async (next: Parameters<typeof updateGatewayProfile>[1]) => {
    try {
      const saved = await updateGatewayProfile(target, next);
      queryClient.setQueryData(["gateway-profile"], saved);
      await queryClient.invalidateQueries({ queryKey: ["agent-connection"] });
      void message.success(t("proxy.profileSaved"));
    } catch (error) {
      void message.error(error instanceof Error ? error.message : String(error));
    }
  };

  return (
    <Card size="small" className="page-surface" title={t("proxy.profileTitle")}>
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("proxy.profileHint")}
      </Text>
      <Space direction="vertical" size="middle" style={{ width: "100%" }}>
        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("proxy.slotDefault")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("proxy.slotDefaultHint")}
            </Text>
          </Space>
          <Select
            allowClear
            showSearch
            optionFilterProp="label"
            style={{ minWidth: 280 }}
            value={profile?.defaultModel || undefined}
            options={modelOptions}
            placeholder="auto"
            onChange={(value) => void patch({ defaultModel: String(value ?? "") })}
          />
        </Space>

        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("providers.catalogSubagentLabel")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("providers.catalogSubagentHint")}
            </Text>
          </Space>
          <Select
            allowClear
            showSearch
            optionFilterProp="label"
            style={{ minWidth: 280 }}
            value={profile?.subagentModel || undefined}
            options={modelOptions}
            onChange={(value) => void patch({ subagentModel: String(value ?? "") })}
          />
        </Space>

        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("proxy.slotLongContext")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("proxy.slotLongContextHint")}
            </Text>
          </Space>
          <Space>
            <InputNumber
              min={0}
              step={1000}
              style={{ width: 120 }}
              value={profile?.longContextTokens ?? 0}
              onChange={(value) => void patch({ longContextTokens: Number(value ?? 0) })}
            />
            <Select
              allowClear
              showSearch
              optionFilterProp="label"
              style={{ minWidth: 220 }}
              value={profile?.longContextModel || undefined}
              options={modelOptions}
              placeholder={t("proxy.slotOff")}
              onChange={(value) => void patch({ longContextModel: String(value ?? "") })}
            />
          </Space>
        </Space>

        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("proxy.slotWebSearch")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("proxy.slotWebSearchHint")}
            </Text>
          </Space>
          <Select
            allowClear
            showSearch
            optionFilterProp="label"
            style={{ minWidth: 280 }}
            value={profile?.webSearchModel || undefined}
            options={modelOptions}
            placeholder={t("proxy.slotOff")}
            onChange={(value) => void patch({ webSearchModel: String(value ?? "") })}
          />
        </Space>

        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("providers.fallbackModeLabel")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("providers.fallbackModeHint")}
            </Text>
          </Space>
          <Select
            style={{ minWidth: 180 }}
            value={profile?.fallbackMode ?? "off"}
            onChange={(value) => void patch({ fallbackMode: String(value) })}
            options={[
              { value: "off", label: t("providers.fallbackModeOff") },
              { value: "retry", label: t("providers.fallbackModeRetry") },
              { value: "model_chain", label: t("providers.fallbackModeChain") },
            ]}
          />
        </Space>

        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("providers.catalogHideOfficialLabel")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("providers.catalogHideOfficialHint")}
            </Text>
          </Space>
          <Switch
            checked={profile?.hideOfficial === true}
            onChange={(checked) => void patch({ hideOfficial: checked })}
          />
        </Space>

        <Space align="start" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space direction="vertical" size={0}>
            <strong>{t("proxy.allowedUpstreams")}</strong>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {t("proxy.allowedUpstreamsHint")}
            </Text>
          </Space>
          <Select
            mode="multiple"
            allowClear
            style={{ minWidth: 280, maxWidth: 420 }}
            value={profile?.allowedUpstreamIds ?? []}
            options={upstreamOptions}
            placeholder={t("proxy.allowedUpstreamsAll")}
            onChange={(value) => void patch({ allowedUpstreamIds: value })}
          />
        </Space>

        <Button
          type="link"
          size="small"
          style={{ padding: 0, alignSelf: "flex-start" }}
          onClick={() => void profileQuery.refetch()}
        >
          {t("proxy.refresh")}
        </Button>
      </Space>
    </Card>
  );
}
