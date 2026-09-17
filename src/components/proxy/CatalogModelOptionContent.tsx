import type { ReactNode } from "react";
import { Typography } from "antd";
import type { CatalogModelLabelSource } from "@/utils/catalogModelLabel";
import { catalogModelView } from "@/utils/catalogModelLabel";

const { Text } = Typography;

export type CatalogModelSelectOption = {
  value: string;
  label: string;
  provider?: string;
  searchText?: string;
  title?: string;
  extra?: string;
};

export function toCatalogModelSelectOption(
  entry: CatalogModelLabelSource,
  extra = "",
): CatalogModelSelectOption {
  const view = catalogModelView(entry);
  const extraText = extra.trim();
  return {
    value: entry.publicId,
    label: view.short,
    provider: view.provider,
    searchText: extraText ? `${view.searchText} ${extraText}` : view.searchText,
    title: extraText ? `${view.title}  ${extraText}` : view.title,
    extra: extraText,
  };
}

export function CatalogModelOptionContent({
  label,
  provider,
  extra,
}: {
  label: string;
  provider?: string;
  extra?: string;
}) {
  const secondary = [provider, extra].filter(Boolean).join("  ");
  return (
    <div style={{ lineHeight: 1.35, padding: "2px 0" }}>
      <div>{label}</div>
      {secondary ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {secondary}
        </Text>
      ) : null}
    </div>
  );
}

export function renderCatalogModelOption(option: {
  data?: CatalogModelSelectOption;
  label?: ReactNode;
}) {
  const data = option.data;
  return (
    <CatalogModelOptionContent
      label={String(data?.label ?? option.label ?? "")}
      provider={data?.provider}
      extra={data?.extra}
    />
  );
}

export const catalogModelSelectProps = {
  showSearch: true,
  optionFilterProp: "searchText" as const,
  optionRender: renderCatalogModelOption,
};
