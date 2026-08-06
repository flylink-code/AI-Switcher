import { ConfigProvider, Segmented, Tooltip, theme } from "antd";
import {
  USAGE_SOURCE_FILTER_OPTIONS,
  usageSourceIcon,
  usageSourceSegmentLabel,
  type UsageSourceFilter,
} from "@/components/UsageSourceIcons";

type Props = {
  value: UsageSourceFilter;
  onChange: (value: UsageSourceFilter) => void;
  /** i18n `t` for option labels. */
  t: (key: string) => string;
  /** Icon-only items for narrow rails (labels remain as tooltips). */
  iconOnly?: boolean;
};

/** Source filter Segmented styled to match adjacent middle-size Select controls. */
export function UsageSourceFilterSegmented({ value, onChange, t, iconOnly = false }: Props) {
  const { token } = theme.useToken();

  return (
    <ConfigProvider
      theme={{
        components: {
          Segmented: {
            trackBg: token.colorBgContainer,
            itemSelectedBg: token.colorFillSecondary,
            itemHoverBg: token.colorFillTertiary,
            trackPadding: 2,
          },
        },
      }}
    >
      <Segmented<UsageSourceFilter>
        className="heatmap-source-filter"
        size="middle"
        value={value}
        onChange={onChange}
        style={{
          border: `1px solid ${token.colorBorder}`,
          borderRadius: token.borderRadius,
          height: token.controlHeight,
          boxSizing: "border-box",
        }}
        options={USAGE_SOURCE_FILTER_OPTIONS.map((option) => {
          const label = t(option.labelKey);
          return {
            value: option.value,
            label: (
              <Tooltip title={label}>
                {iconOnly ? (
                  <span style={{ display: "inline-flex", alignItems: "center" }}>
                    {usageSourceIcon(option.value, { size: 16 })}
                  </span>
                ) : (
                  usageSourceSegmentLabel(option.value, label)
                )}
              </Tooltip>
            ),
          };
        })}
      />
    </ConfigProvider>
  );
}
