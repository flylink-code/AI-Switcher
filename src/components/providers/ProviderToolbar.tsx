import React, { useRef } from "react";
import { Button, Dropdown, Input, Select, type MenuProps } from "antd";
import SearchOutlined from "@ant-design/icons/es/icons/SearchOutlined";
import ImportOutlined from "@ant-design/icons/es/icons/ImportOutlined";
import FolderOpenOutlined from "@ant-design/icons/es/icons/FolderOpenOutlined";
import PlusOutlined from "@ant-design/icons/es/icons/PlusOutlined";
import EllipsisOutlined from "@ant-design/icons/es/icons/EllipsisOutlined";
import LoginOutlined from "@ant-design/icons/es/icons/LoginOutlined";
import ExportOutlined from "@ant-design/icons/es/icons/ExportOutlined";
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
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Low-frequency maintenance operations live in the overflow menu.
  const moreItems: MenuProps["items"] = [
    ...(target !== "codex" && target !== "opencode"
      ? [
          {
            key: "chatgptLogin",
            icon: <LoginOutlined />,
            label: t("providers.chatgptLogin"),
            disabled: oauthPolling,
            onClick: onCodexOauthLogin,
          },
        ]
      : []),
    {
      key: "importLive",
      icon: <ImportOutlined />,
      label: target === "opencode" ? t("providers.syncOpenCodeLive") : t("providers.importLive"),
      disabled: busy,
      onClick: onImportLive,
    },
    {
      key: "importClipboard",
      label: t("providers.importClipboard"),
      disabled: busy,
      onClick: onImportClipboard,
    },
    {
      key: "importFile",
      label: t("providers.importFile"),
      disabled: busy,
      onClick: () => fileInputRef.current?.click(),
    },
    {
      key: "export",
      icon: <ExportOutlined />,
      label: t("providers.export"),
      disabled: busy,
      onClick: onExport,
    },
    ...(target === "opencode"
      ? [
          { type: "divider" as const },
          {
            key: "opencodeConfig",
            icon: <FolderOpenOutlined />,
            label: t("providers.opencodeOpenConfig"),
            onClick: onOpenOpencodeConfig,
          },
        ]
      : []),
  ];

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
        <Button type="primary" size="small" icon={<PlusOutlined />} onClick={onCreate}>
          {t("providers.create")}
        </Button>
        <Dropdown menu={{ items: moreItems }} trigger={["click"]}>
          <Button
            size="small"
            icon={<EllipsisOutlined />}
            loading={oauthPolling}
            aria-label={t("providers.moreActions")}
          />
        </Dropdown>
        <input
          ref={fileInputRef}
          type="file"
          accept="application/json"
          hidden
          onChange={(event) => {
            const file = event.target.files?.[0];
            if (file) onImportFile(file);
            event.currentTarget.value = "";
          }}
        />
      </Inline>
    </Inline>
  );
};
