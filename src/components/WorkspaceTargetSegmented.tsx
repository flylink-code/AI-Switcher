import { ConfigProvider, Segmented, Tooltip, theme } from "antd";
import {
  usageSourceSegmentLabel,
  type UsageSourceFilter,
} from "@/components/UsageSourceIcons";
import type { ProviderTarget } from "@/types/backend";

const TARGET_OPTIONS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex"];

const LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "workspace.claude_code",
  claude_desktop: "workspace.claude_desktop",
  codex: "workspace.codex",
};

type Props<T extends ProviderTarget> = {
  value: T;
  onChange: (value: T) => void;
  t: (key: string) => string;
  /** Subset of targets to show; defaults to all three apps. */
  targets?: readonly T[];
  /** Optional aria-label for accessibility. */
  ariaLabel?: string;
  size?: "small" | "middle";
  className?: string;
};

/** Code / Desktop / Codex target switcher styled like Overview/Usage filters. */
export function WorkspaceTargetSegmented<T extends ProviderTarget>({
  value,
  onChange,
  t,
  targets,
  ariaLabel,
  size = "middle",
  className,
}: Props<T>) {
  const { token } = theme.useToken();
  const options: readonly T[] = targets ?? (TARGET_OPTIONS as unknown as T[]);

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
      <Segmented<T>
        className={["heatmap-source-filter", className].filter(Boolean).join(" ")}
        size={size}
        value={value}
        aria-label={ariaLabel ?? t("workspace.target")}
        onChange={onChange}
        style={{
          border: `1px solid ${token.colorBorder}`,
          borderRadius: token.borderRadiusLG ?? token.borderRadius,
          height: size === "small" ? token.controlHeightSM : token.controlHeight,
          boxSizing: "border-box",
        }}
        options={options.map((option) => {
          const label = t(LABEL_KEYS[option]);
          return {
            value: option,
            label: (
              <Tooltip title={label}>
                {usageSourceSegmentLabel(option as UsageSourceFilter, label)}
              </Tooltip>
            ),
          };
        })}
      />
    </ConfigProvider>
  );
}
