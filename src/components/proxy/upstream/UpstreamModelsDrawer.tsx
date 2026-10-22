import { useState } from "react";
import { Button, Drawer, Input, Popconfirm, Space, Switch, Table, Tag, Typography } from "antd";
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
  onSetFast: (modelIds: string[], enabled: boolean, confirmUnknown: boolean) => void;
}

export function UpstreamModelsDrawer({
  upstream,
  models,
  saving,
  onClose,
  onRefresh,
  onToggleModel,
  onSetFast,
}: UpstreamModelsDrawerProps) {
  const { t } = useTranslation();
  const [selected, setSelected] = useState<string[]>([]);
  const [search, setSearch] = useState("");
  const openai = ["openai_chat", "openai_responses", "proxy"].includes(upstream?.protocolType ?? "");
  const selectedRows = models.filter((row) => selected.includes(row.modelId));
  const eligibleIds = selectedRows.filter((row) => row.fastSupported !== false).map((row) => row.modelId);
  const unknownSelected = selectedRows.some((row) => row.fastSupported == null);
  const enableButton = (
    <Button size="small" disabled={saving || !openai || eligibleIds.length === 0}
      onClick={unknownSelected ? undefined : () => onSetFast(eligibleIds, true, false)}>
      {t("proxy.fastEnableSelected")}
    </Button>
  );

  return (
    <Drawer
      title={
        upstream
          ? t("proxy.upstreamModelsTitle", { name: upstream.name })
          : t("proxy.upstreamModels")
      }
      open={Boolean(upstream)}
      onClose={onClose}
      width={660}
      extra={
        <Button size="small" loading={saving} onClick={onRefresh}>
          {t("proxy.refreshModels")}
        </Button>
      }
    >
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("proxy.upstreamModelsHint")}
      </Text>
      <Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
        {t("proxy.fastHint")}
      </Text>
      <Space wrap style={{ marginBottom: 12 }}>
        <Input.Search value={search} onChange={(event) => setSearch(event.target.value)}
          placeholder={t("proxy.fastSearch")} allowClear style={{ width: 210 }} />
        {unknownSelected ? (
          <Popconfirm title={t("proxy.fastConfirm")} description={t("proxy.fastConfirmHint")}
            onConfirm={() => onSetFast(eligibleIds, true, true)} disabled={saving || !openai || eligibleIds.length === 0}>
            {enableButton}
          </Popconfirm>
        ) : enableButton}
        <Button size="small" disabled={saving || selectedRows.length === 0}
          onClick={() => onSetFast(selectedRows.map((row) => row.modelId), false, false)}>
          {t("proxy.fastDisableSelected")}
        </Button>
      </Space>
      <Table<GatewayUpstreamModelRow>
        rowKey="modelId" size="small" pagination={false}
        dataSource={models.filter((row) => row.modelId.toLowerCase().includes(search.toLowerCase()))}
        locale={{ emptyText: t("proxy.upstreamModelsEmpty") }}
        rowSelection={{ selectedRowKeys: selected, onChange: (keys) => setSelected(keys.map(String)),
          getCheckboxProps: () => ({ disabled: saving }) }}
        columns={[
          { title: t("proxy.fastModel"), dataIndex: "modelId", render: (id: string) => (
            <Text style={{ wordBreak: "break-all" }}>{id}
              {id.trim().toLowerCase() === (upstream?.model ?? "").trim().toLowerCase()
                ? ` (${t("proxy.upstreamDefaultModel")})` : ""}
            </Text>
          ) },
          { title: t("proxy.fastVisible"), width: 72, render: (_, row) => (
            <Switch size="small" checked={row.visible} aria-label={`${row.modelId} ${t("proxy.fastVisible")}`}
              disabled={saving || row.modelId.trim().toLowerCase() === (upstream?.model ?? "").trim().toLowerCase()}
              onChange={(checked) => onToggleModel(row.modelId, checked)} />
          ) },
          { title: t("proxy.fastCapability"), width: 114, render: (_, row) => (
            <Tag color={openai && row.fastSupported === true ? "green" : "default"}>
              {t(!openai || row.fastSupported === false ? "proxy.fastUnsupported"
                : row.fastSupported === true ? "proxy.fastSupported" : "proxy.fastUnknown")}
            </Tag>
          ) },
          { title: t("proxy.fastDefault"), width: 78, render: (_, row) => {
            const needsConfirmation = !row.fastEnabled && row.fastSupported == null;
            const toggle = <Switch size="small" checked={row.fastEnabled}
              aria-label={`${row.modelId} ${t("proxy.fastDefault")}`}
              disabled={saving || ((!openai || row.fastSupported === false) && !row.fastEnabled)}
              onChange={needsConfirmation ? undefined : (checked) => onSetFast([row.modelId], checked, false)} />;
            return needsConfirmation && openai ? (
              <Popconfirm title={t("proxy.fastConfirm")} description={t("proxy.fastConfirmHint")}
                disabled={saving} onConfirm={() => onSetFast([row.modelId], true, true)}>
                {toggle}
              </Popconfirm>
            ) : toggle;
          } },
        ]}
      />
    </Drawer>
  );
}
