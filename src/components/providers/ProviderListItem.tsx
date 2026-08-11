import React from "react";
import { Typography } from "antd";
import { useTranslation } from "react-i18next";
import type { Provider } from "@/types/backend";
import { ProviderBrandIcon } from "@/components/ProviderBrandIcon";

const { Text } = Typography;

export interface ProviderListItemProps {
  provider: Provider;
  selected: boolean;
  onSelect: (provider: Provider) => void;
}

/** Compact scan row for the Providers master list. */
export const ProviderListItem: React.FC<ProviderListItemProps> = ({
  provider,
  selected,
  onSelect,
}) => {
  const { t } = useTranslation();
  const isOpencode = provider.targetApp === "opencode";
  const healthColor =
    provider.healthStatus == null
      ? "var(--color-text-tertiary)"
      : provider.healthStatus === "healthy"
        ? "var(--color-success, #22c55e)"
        : "var(--color-error, #ef4444)";

  return (
    <button
      type="button"
      onClick={() => onSelect(provider)}
      aria-current={selected ? "true" : undefined}
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-3)",
        width: "100%",
        padding: "8px 10px",
        border: "none",
        borderRadius: "var(--radius-md)",
        cursor: "pointer",
        textAlign: "left",
        backgroundColor: selected ? "var(--color-brand-subtle)" : "transparent",
        color: "var(--color-text-primary)",
        transition: "background-color 0.15s ease",
      }}
      onMouseEnter={(e) => {
        if (!selected) e.currentTarget.style.backgroundColor = "var(--color-bg-surface)";
      }}
      onMouseLeave={(e) => {
        if (!selected) e.currentTarget.style.backgroundColor = "transparent";
      }}
    >
      <ProviderBrandIcon provider={provider} size={20} />

      <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 1 }}>
        <span style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0 }}>
          <Text
            strong={provider.isCurrent || selected}
            ellipsis
            style={{ fontSize: "var(--font-size-md)", color: "inherit" }}
          >
            {provider.name}
          </Text>
          {!isOpencode && provider.isCurrent && (
            <span
              style={{
                fontSize: "var(--font-size-xs)",
                color: "var(--color-brand)",
                flexShrink: 0,
              }}
            >
              {t("providers.current")}
            </span>
          )}
        </span>
        <span
          style={{
            fontSize: "var(--font-size-xs)",
            color: "var(--color-text-secondary)",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
        >
          {provider.model || t("providers.defaultModel", { defaultValue: "默认" })}
        </span>
      </span>

      <span
        style={{
          display: "flex",
          alignItems: "center",
          gap: 4,
          flexShrink: 0,
          fontSize: "var(--font-size-xs)",
          color: "var(--color-text-secondary)",
        }}
      >
        <span
          aria-hidden
          style={{
            width: 6,
            height: 6,
            borderRadius: "50%",
            backgroundColor: healthColor,
          }}
        />
        {provider.healthLatencyMs != null ? `${provider.healthLatencyMs}ms` : ""}
      </span>
    </button>
  );
};
