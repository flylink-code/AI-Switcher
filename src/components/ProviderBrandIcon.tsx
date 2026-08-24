import type { CSSProperties } from "react";
import type { Provider } from "@/types/backend";

interface BrandRule {
  /** hostname substrings (lowercase) that identify the vendor */
  match: string[];
  /** short label rendered inside the avatar (1–2 chars) */
  label: string;
  /** brand background color */
  color: string;
}

/**
 * Well-known provider brands, matched against the provider's baseUrl host.
 * Order matters: first match wins, so list more specific hosts first.
 */
const BRAND_RULES: BrandRule[] = [
  { match: ["openai.azure.com", "azure.com", "azure.cn"], label: "Az", color: "#0078d4" },
  { match: ["api.openai.com", "openai.com"], label: "OA", color: "#0f0f0f" },
  { match: ["api.anthropic.com", "anthropic.com", "claude.ai"], label: "An", color: "#d97757" },
  { match: ["generativelanguage", "googleapis.com", "deepmind"], label: "G", color: "#4285f4" },
  { match: ["deepseek"], label: "DS", color: "#4d6bfe" },
  { match: ["moonshot", "kimi"], label: "Mo", color: "#6c5ce7" },
  { match: ["dashscope", "aliyuncs.com", "qwen"], label: "Qw", color: "#615ced" },
  { match: ["bigmodel.cn", "zhipu"], label: "Zp", color: "#365dff" },
  { match: ["qianfan", "baidubce.com", "baidu"], label: "千", color: "#2932e1" },
  { match: ["openrouter"], label: "OR", color: "#6366f1" },
  { match: ["siliconflow", "siliconcloud"], label: "SF", color: "#7c3aed" },
  { match: ["api.githubcopilot.com", "copilot"], label: "CP", color: "#24292f" },
  { match: ["groq"], label: "Gr", color: "#f55036" },
  { match: ["mistral"], label: "Mi", color: "#ff7000" },
  { match: ["x.ai", "xai"], label: "X", color: "#0f0f0f" },
  { match: ["cohere"], label: "Co", color: "#39594d" },
  { match: ["together"], label: "To", color: "#0ea5e9" },
  { match: ["fireworks"], label: "Fw", color: "#e11d48" },
  { match: ["perplexity"], label: "Px", color: "#20b8cd" },
  { match: ["stepfun"], label: "阶", color: "#16d6d2" },
  { match: ["modelscope"], label: "MS", color: "#624aff" },
  { match: ["novita"], label: "No", color: "#000000" },
  { match: ["ollama", "localhost", "127.0.0.1", "0.0.0.0", "[::1]"], label: "本", color: "#6b7280" },
];

/** Neutral fallback palette for unrecognized vendors (hashed by name). */
const FALLBACK_COLORS = ["#007aff", "#5856d6", "#af52de", "#ff2d55", "#ff9500", "#34c759", "#5ac8fa", "#8e8e93"];

function hostOf(baseUrl: string): string {
  try {
    return new URL(baseUrl).hostname.toLowerCase();
  } catch {
    return baseUrl.toLowerCase();
  }
}

export function providerBrand(
  provider?: Partial<Pick<Provider, "name" | "baseUrl">> | null
): {
  label: string;
  color: string;
} {
  if (!provider) return { label: "?", color: "#6b7280" };
  const host = hostOf(provider.baseUrl ?? "");
  const rule = BRAND_RULES.find((r) => r.match.some((m) => host.includes(m)));
  if (rule) return { label: rule.label, color: rule.color };
  const name = (provider.name ?? "").trim();
  const label = Array.from(name)[0]?.toUpperCase() ?? "?";
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + (ch.codePointAt(0) ?? 0)) >>> 0;
  return { label, color: FALLBACK_COLORS[hash % FALLBACK_COLORS.length] };
}

export interface ProviderBrandIconProps {
  provider?: Partial<Pick<Provider, "name" | "baseUrl">> | null;
  name?: string;
  baseUrl?: string;
  targetApp?: string;
  size?: number;
  style?: CSSProperties;
}

/**
 * Vendor avatar for provider cards: brand-colored badge when the baseUrl host
 * matches a known vendor, otherwise a neutral first-letter avatar.
 */
export function ProviderBrandIcon(props: ProviderBrandIconProps) {
  const { provider, name, baseUrl, size = 22, style } = props;
  const targetProvider = provider ?? { name, baseUrl };
  const { label, color } = providerBrand(targetProvider);
  return (
    <span
      aria-hidden
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        width: size,
        height: size,
        borderRadius: Math.max(4, size * 0.28),
        background: color,
        color: "#fff",
        fontSize: Math.max(9, size * 0.44),
        fontWeight: 700,
        lineHeight: 1,
        flexShrink: 0,
        userSelect: "none",
        ...style,
      }}
    >
      {label}
    </span>
  );
}
