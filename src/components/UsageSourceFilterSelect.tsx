import { Select } from "antd";
import {
  USAGE_SOURCE_FILTER_OPTIONS,
  usageSourceSegmentLabel,
  type UsageSourceFilter,
} from "@/components/UsageSourceIcons";

type Props = {
  value: UsageSourceFilter;
  onChange: (value: UsageSourceFilter) => void;
  /** i18n `t` for option labels. */
  t: (key: string) => string;
  size?: "small" | "middle";
};

/**
 * Compact "Data Source" dropdown for usage analytics. Replaces the former
 * Segmented control that visually mimicked the global Agent switcher —
 * analytics filtering is a data-source concern, not Agent context (plan §29).
 */
export function UsageSourceFilterSelect({ value, onChange, t, size = "small" }: Props) {
  return (
    <Select<UsageSourceFilter>
      size={size}
      value={value}
      onChange={onChange}
      aria-label={t("usage.sourceLabel")}
      style={{ minWidth: size === "small" ? 148 : 168 }}
      variant="outlined"
      options={USAGE_SOURCE_FILTER_OPTIONS.map((option) => ({
        value: option.value,
        label: usageSourceSegmentLabel(option.value, t(option.labelKey)),
      }))}
    />
  );
}
