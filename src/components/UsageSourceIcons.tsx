import type { CSSProperties, ReactNode } from "react";
import type { ProviderTarget } from "@/types/backend";

export type UsageSourceFilter = ProviderTarget | "all";

const CLAUDE_ORANGE = "#D97757";

type IconProps = {
  size?: number;
  style?: CSSProperties;
  className?: string;
};

function SvgShell({
  size = 14,
  style,
  className,
  children,
  viewBox = "0 0 24 24",
}: IconProps & { children: ReactNode; viewBox?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox={viewBox}
      fill="currentColor"
      aria-hidden
      focusable="false"
      className={className}
      style={{ display: "block", flexShrink: 0, ...style }}
    >
      {children}
    </svg>
  );
}

/** Four-cell grid for “All”. */
export function UsageSourceAllIcon(props: IconProps) {
  return (
    <SvgShell {...props}>
      <rect x="3" y="3" width="8" height="8" rx="1.5" />
      <rect x="13" y="3" width="8" height="8" rx="1.5" />
      <rect x="3" y="13" width="8" height="8" rx="1.5" />
      <rect x="13" y="13" width="8" height="8" rx="1.5" />
    </SvgShell>
  );
}

/**
 * Anthropic / Claude starburst (simplified public brand mark).
 * Used for Claude Code.
 */
export function ClaudeStarburstIcon({ size = 14, style, className }: IconProps) {
  return (
    <SvgShell size={size} className={className} style={{ color: CLAUDE_ORANGE, ...style }}>
      <path d="M12 2.2 13.35 8.1 18.9 5.1 15.9 10.65 21.8 12 15.9 13.35 18.9 18.9 13.35 15.9 12 21.8 10.65 15.9 5.1 18.9 8.1 13.35 2.2 12 8.1 10.65 5.1 5.1 10.65 8.1Z" />
    </SvgShell>
  );
}

/**
 * Claude mark with a small desktop/window badge for Claude Desktop.
 */
export function ClaudeDesktopIcon({ size = 14, style, className }: IconProps) {
  return (
    <SvgShell size={size} className={className} style={{ color: CLAUDE_ORANGE, ...style }} viewBox="0 0 24 24">
      <path d="M10.5 2.4 11.45 6.55 15.35 4.45 13.25 8.35 17.4 9.3 13.25 10.25 15.35 14.15 11.45 12.05 10.5 16.2 9.55 12.05 5.65 14.15 7.75 10.25 3.6 9.3 7.75 8.35 5.65 4.45 9.55 6.55Z" />
      <rect x="12.5" y="13.2" width="8.2" height="6.6" rx="1.1" fill="none" stroke="currentColor" strokeWidth="1.4" />
      <path d="M14.2 19.8h4.8" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </SvgShell>
  );
}

/**
 * OpenAI blossom / sunburst (simplified), used for Codex.
 */
export function OpenAiBlossomIcon(props: IconProps) {
  return (
    <SvgShell {...props}>
      <path d="M22.282 9.821a5.985 5.985 0 0 0-.516-4.91 6.046 6.046 0 0 0-6.51-2.9A6.065 6.065 0 0 0 4.981 4.18a5.985 5.985 0 0 0-3.958 2.9 6.046 6.046 0 0 0 .743 7.097 5.98 5.98 0 0 0 .51 4.911 6.051 6.051 0 0 0 6.515 2.9A5.985 5.985 0 0 0 13.26 24a6.056 6.056 0 0 0 5.774-4.205 5.99 5.99 0 0 0 3.959-2.888 6.056 6.056 0 0 0-.747-7.086zM13.26 22.43a4.476 4.476 0 0 1-2.876-1.04l.141-.081 4.779-2.76a.795.795 0 0 0 .392-.681v-6.737l2.02 1.168a.071.071 0 0 1 .038.052v5.583a4.504 4.504 0 0 1-4.494 4.494zM3.58 18.297a4.47 4.47 0 0 1-.535-3.01l.142.085 4.783 2.762a.771.771 0 0 0 .78 0l5.843-3.373v2.332a.08.08 0 0 1-.033.062L9.74 19.95a4.5 4.5 0 0 1-6.16-1.654zM2.34 7.896a4.485 4.485 0 0 1 2.366-1.973V11.6a.766.766 0 0 0 .388.676l5.814 3.354-2.02 1.168a.076.076 0 0 1-.071 0l-4.83-2.786A4.504 4.504 0 0 1 2.34 7.872zm16.597 3.855-5.833-3.387L15.119 7.2a.076.076 0 0 1 .071 0l4.83 2.791a4.494 4.494 0 0 1-.676 8.105v-5.678a.79.79 0 0 0-.407-.667zm2.01-3.023-.141-.085-4.774-2.782a.776.776 0 0 0-.785 0L9.409 9.23V6.897a.066.066 0 0 1 .028-.061l4.83-2.787a4.5 4.5 0 0 1 6.68 4.66zm-12.64 4.135L5.304 10.68a.08.08 0 0 1-.038-.05V5.049a4.499 4.499 0 0 1 7.375-3.453l-.142.08L7.712 4.436a.795.795 0 0 0-.393.681zm1.097-2.365 2.602-1.5 2.602 1.5v2.999l-2.597 1.5-2.607-1.5Z" />
    </SvgShell>
  );
}

export function usageSourceIcon(source: UsageSourceFilter, props?: IconProps): ReactNode {
  switch (source) {
    case "all":
      return <UsageSourceAllIcon {...props} />;
    case "claude_code":
      return <ClaudeStarburstIcon {...props} />;
    case "claude_desktop":
      return <ClaudeDesktopIcon {...props} />;
    case "codex":
      return <OpenAiBlossomIcon {...props} />;
    default: {
      const _exhaustive: never = source;
      return _exhaustive;
    }
  }
}

export const USAGE_SOURCE_FILTER_OPTIONS: Array<{
  value: UsageSourceFilter;
  labelKey: string;
}> = [
  { value: "all", labelKey: "usage.sourceAll" },
  { value: "claude_code", labelKey: "usage.sourceClaudeCode" },
  { value: "claude_desktop", labelKey: "usage.sourceClaudeDesktop" },
  { value: "codex", labelKey: "usage.sourceCodex" },
];
