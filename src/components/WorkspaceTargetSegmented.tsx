import { Segmented, Tooltip } from "antd";
import {
  usageSourceSegmentLabel,
  type UsageSourceFilter,
} from "@/components/UsageSourceIcons";
import { usePagePreferencesStore } from "@/stores/pagePreferencesStore";
import type { ProviderTarget } from "@/types/backend";

const TARGET_OPTIONS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode", "pi"];

const LABEL_KEYS: Record<ProviderTarget, string> = {
  claude_code: "workspace.claude_code",
  claude_desktop: "workspace.claude_desktop",
  codex: "workspace.codex",
  opencode: "workspace.opencode",
  pi: "workspace.pi",
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
  size = "small",
  className = "",
}: Props<T>) {
  const visibleAgents = usePagePreferencesStore((state) => state.visibleAgents);

  const baseOptions: readonly T[] = targets ?? (TARGET_OPTIONS as unknown as T[]);
  const filteredOptions = baseOptions.filter((opt) => visibleAgents.includes(opt as ProviderTarget));
  const options = filteredOptions.length > 0 ? filteredOptions : baseOptions;
  const activeValue = options.includes(value) ? value : options[0];

  return (
    <Segmented<T>
      className={["app-segmented-switcher", className].filter(Boolean).join(" ")}
      size={size}
      value={activeValue}
      aria-label={ariaLabel ?? t("workspace.target")}
      onChange={onChange}
      options={options.map((option) => {
        const label = t(LABEL_KEYS[option]);
        return {
          value: option,
          label: (
            <Tooltip title={label}>
              {usageSourceSegmentLabel(
                option as UsageSourceFilter,
                label,
              )}
            </Tooltip>
          ),
        };
      })}
    />
  );
}
