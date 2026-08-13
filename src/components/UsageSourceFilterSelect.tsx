import { Select } from "antd";
import {
  USAGE_SOURCE_FILTER_OPTIONS,
  usageSourceSegmentLabel,
  type UsageSourceFilter,
} from "@/components/UsageSourceIcons";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";

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
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const availableOptions = USAGE_SOURCE_FILTER_OPTIONS.filter((option) => {
    if (option.value === "all" || option.value === "antigravity") return true;
    return visibleAgents.includes(option.value);
  });

  return (
    <Select<UsageSourceFilter>
      size={size}
      value={value}
      onChange={onChange}
      aria-label={t("usage.sourceLabel")}
      style={{ minWidth: size === "small" ? 148 : 168 }}
      variant="outlined"
      options={availableOptions.map((option) => ({
        value: option.value,
        label: usageSourceSegmentLabel(option.value, t(option.labelKey)),
      }))}
    />
  );
}
