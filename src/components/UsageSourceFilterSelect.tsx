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
    if (
      option.value === "all" ||
      option.value === "antigravity" ||
      option.value === "kiro" ||
      option.value === "vscode_copilot"
    ) {
      return true;
    }
    return visibleAgents.includes(option.value);
  });

  const allOption = availableOptions.find((option) => option.group === "all");
  const agentOptions = availableOptions.filter((option) => option.group === "agent");
  const gatewayOptions = availableOptions.filter((option) => option.group === "gateway");

  return (
    <Select<UsageSourceFilter>
      size={size}
      value={value}
      onChange={onChange}
      aria-label={t("usage.sourceLabel")}
      style={{ minWidth: size === "small" ? 156 : 176 }}
      variant="outlined"
      options={[
        ...(allOption
          ? [
              {
                value: allOption.value,
                label: usageSourceSegmentLabel(allOption.value, t(allOption.labelKey)),
              },
            ]
          : []),
        ...(agentOptions.length > 0
          ? [
              {
                label: t("usage.groupAgent"),
                options: agentOptions.map((option) => ({
                  value: option.value,
                  label: usageSourceSegmentLabel(option.value, t(option.labelKey)),
                })),
              },
            ]
          : []),
        ...(gatewayOptions.length > 0
          ? [
              {
                label: t("usage.groupGateway"),
                options: gatewayOptions.map((option) => ({
                  value: option.value,
                  label: usageSourceSegmentLabel(option.value, t(option.labelKey)),
                })),
              },
            ]
          : []),
      ]}
    />
  );
}
