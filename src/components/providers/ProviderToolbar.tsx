import React from "react";
import { Button, Input, Select, Space } from "antd";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import { useTranslation } from "react-i18next";
import type { ProviderTarget } from "@/types/backend";
import { Inline } from "@/components/ui/Inline";

export interface ProviderToolbarProps {
  target: ProviderTarget;
  searchQuery: string;
  onSearchChange: (query: string) => void;
  statusFilter: string;
  onStatusFilterChange: (status: string) => void;
  busy: boolean;
  oauthPolling: boolean;
  onCodexOauthLogin: () => void;
  onImportLive: () => void;
  onOpenOpencodeConfig: () => void;
  onExport: () => void;
  onImportClipboard: () => void;
  onImportFile: (file: File) => void;
  onCreate: () => void;
  className?: string;
  style?: React.CSSProperties;
}

export const ProviderToolbar: React.FC<ProviderToolbarProps> = ({
  target,
  searchQuery,
  onSearchChange,
  statusFilter,
  onStatusFilterChange,
  busy,
  oauthPolling,
  onCodexOauthLogin,
  onImportLive,
  onOpenOpencodeConfig,
  onExport,
  onImportClipboard,
  onImportFile,
  onCreate,
  className = "",
  style,
}) => {
  const { t } = useTranslation();

  return (
    <Inline
      justify="space-between"
      align="center"
      wrap
      gap="md"
      className={`providers-toolbar-new ${className}`.trim()}
      style={style}
    >
      {/* Left Search & Filters */}
      <Inline gap="sm" align="center" wrap>
        <Input
          placeholder={t("providers.searchPlaceholder", { defaultValue: "搜索供应商 / Endpoint / 模型..." })}
          prefix={<SearchOutlined style={{ color: "var(--color-text-tertiary)" }} />}
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          allowClear
          style={{ width: 240 }}
          size="small"
        />

        <Select
          value={statusFilter}
          onChange={onStatusFilterChange}
          size="small"
          style={{ width: 110 }}
          options={[
            { value: "all", label: t("providers.filterAllStatus", { defaultValue: "全部状态" }) },
            { value: "healthy", label: t("providers.healthy", { defaultValue: "Healthy" }) },
            { value: "unhealthy", label: t("providers.unhealthy", { defaultValue: "Unhealthy" }) },
          ]}
        />
      </Inline>

      {/* Right Operations */}
      <Inline gap="sm" align="center" wrap>
        <Space size={4} wrap>
          {target !== "codex" && target !== "opencode" && (
            <Button type="text" size="small" loading={oauthPolling} onClick={onCodexOauthLogin}>
              {t("providers.chatgptLogin")}
            </Button>
          )}
          <Button type="text" size="small" icon={<ImportOutlined />} loading={busy} onClick={onImportLive}>
            {target === "opencode" ? t("providers.syncOpenCodeLive") : t("providers.importLive")}
          </Button>
          {target === "opencode" && (
            <Button type="text" size="small" icon={<FolderOpenOutlined />} onClick={onOpenOpencodeConfig}>
              {t("providers.opencodeOpenConfig")}
            </Button>
          )}
          <Button type="text" size="small" loading={busy} onClick={onExport}>
            {t("providers.export")}
          </Button>
          <Button type="text" size="small" loading={busy} onClick={onImportClipboard}>
            {t("providers.importClipboard")}
          </Button>
          <label style={{ display: "inline-block" }}>
            <Button type="text" size="small" loading={busy}>
              {t("providers.importFile")}
            </Button>
            <input
              type="file"
              accept="application/json"
              hidden
              onChange={(event) => {
                const file = event.target.files?.[0];
                if (file) onImportFile(file);
                event.currentTarget.value = "";
              }}
            />
          </label>
        </Space>

        <Button type="primary" size="small" icon={<PlusOutlined />} onClick={onCreate}>
          {t("providers.create")}
        </Button>
      </Inline>
    </Inline>
  );
};
