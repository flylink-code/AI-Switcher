import type { ClaudeModelMapping, ProviderTarget, ProtocolType } from "@/types/backend";

export interface ProviderPreset {
  id: string;
  /** Display name filled into the provider form. */
  name: string;
  protocolType: ProtocolType;
  baseUrl: string;
  model: string;
  /** Optional Codex context window hint. */
  modelContextWindow?: number;
  /** Extra models written into failover / Codex catalog. */
  failoverModels?: string[];
  notes?: string;
  /** Targets that should show this preset. */
  targets: ProviderTarget[];
}

const CODE_DESKTOP: ProviderTarget[] = ["claude_code", "claude_desktop", "opencode", "pi"];
const CLAUDE_OPENCODE: ProviderTarget[] = ["claude_code", "claude_desktop", "opencode"];
const ALL_TARGETS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex", "opencode", "pi"];

/** Built-in quick-fill presets for common third-party gateways. */
export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: "deepseek-anthropic",
    name: "DeepSeek",
    protocolType: "anthropic",
    baseUrl: "https://api.deepseek.com/anthropic",
    model: "deepseek-v4-pro",
    failoverModels: ["deepseek-v4-flash"],
    notes: "DeepSeek Anthropic-compatible gateway (deepseek-v4-pro / deepseek-v4-flash)",
    targets: CODE_DESKTOP,
  },
  {
    id: "deepseek-openai",
    name: "DeepSeek (OpenAI)",
    protocolType: "openai_chat",
    baseUrl: "https://api.deepseek.com",
    model: "deepseek-v4-pro",
    failoverModels: ["deepseek-v4-flash"],
    notes: "DeepSeek OpenAI-compatible Chat Completions (deepseek-v4-pro / deepseek-v4-flash)",
    targets: CODE_DESKTOP,
  },
  {
    id: "deepseek-codex",
    name: "DeepSeek",
    protocolType: "openai_responses",
    baseUrl: "https://api.deepseek.com",
    model: "deepseek-v4-flash",
    failoverModels: ["deepseek-v4-pro"],
    modelContextWindow: 1_048_576,
    notes: "DeepSeek Responses API for Codex (flash default; pro as failover)",
    targets: ["codex"],
  },
  {
    id: "kimi-cn",
    name: "Kimi",
    protocolType: "openai_chat",
    baseUrl: "https://api.moonshot.cn/v1",
    model: "kimi-k3",
    failoverModels: ["kimi-k2.6"],
    notes: "Moonshot / Kimi China OpenAI-compatible API",
    targets: ALL_TARGETS,
  },
  {
    id: "kimi-intl",
    name: "Kimi (Intl)",
    protocolType: "openai_chat",
    baseUrl: "https://api.moonshot.ai/v1",
    model: "kimi-k3",
    failoverModels: ["kimi-k2.6"],
    notes: "Moonshot / Kimi international OpenAI-compatible API",
    targets: ALL_TARGETS,
  },
  {
    id: "glm",
    name: "GLM",
    protocolType: "openai_chat",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    model: "glm-5.2",
    notes: "Zhipu GLM OpenAI-compatible API",
    targets: ALL_TARGETS,
  },
  {
    id: "qwen",
    name: "Qwen",
    protocolType: "openai_chat",
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    model: "qwen3.6-plus",
    notes: "Alibaba DashScope OpenAI-compatible mode",
    targets: ALL_TARGETS,
  },
  {
    id: "minimax",
    name: "MiniMax",
    protocolType: "openai_chat",
    baseUrl: "https://api.minimax.chat/v1",
    model: "minimax-m3",
    notes: "MiniMax OpenAI-compatible API",
    targets: ALL_TARGETS,
  },
  {
    id: "antigravity-gateway-external",
    name: "Antigravity Gateway",
    protocolType: "anthropic",
    baseUrl: "http://127.0.0.1:8045",
    model: "claude-sonnet-4-6",
    failoverModels: [
      "gemini-3.6-flash-high",
      "gemini-3.6-flash-low",
      "claude-opus-4-6-thinking",
      "claude-sonnet-4-6-thinking",
    ],
    notes:
      "External Antigravity-Manager gateway (start AG Manager first; fill its API key). For the built-in gateway use the Antigravity page.",
    targets: CLAUDE_OPENCODE,
  },
  {
    id: "antigravity-gateway-external-codex",
    name: "Antigravity Gateway",
    protocolType: "openai_responses",
    baseUrl: "http://127.0.0.1:8045/v1",
    model: "claude-sonnet-4-6",
    failoverModels: ["gemini-3.6-flash-high", "gemini-3.6-flash-low", "claude-opus-4-6-thinking"],
    modelContextWindow: 200_000,
    notes:
      "External Antigravity-Manager OpenAI Responses endpoint for Codex (wire_api=responses). Start AG Manager and fill its API key.",
    targets: ["codex"],
  },
  {
    id: "antigravity-builtin",
    name: "Antigravity (Built-in)",
    protocolType: "anthropic",
    baseUrl: "http://127.0.0.1:15830",
    model: "claude-sonnet-4-6",
    failoverModels: [
      "gemini-3.6-flash-high",
      "gemini-3.6-flash-low",
      "claude-opus-4-6-thinking",
      "claude-sonnet-4-6-thinking",
    ],
    notes:
      "Built-in Antigravity gateway. Claude Code 的 Haiku 槽默认映射到 gemini-3.6-flash；也可在模型映射里改成其他 Gemini。",
    targets: CLAUDE_OPENCODE,
  },
  {
    id: "antigravity-builtin-codex",
    name: "Antigravity (Built-in)",
    protocolType: "openai_responses",
    baseUrl: "http://127.0.0.1:15830/v1",
    model: "claude-sonnet-4-6",
    failoverModels: [
      "gemini-3.6-flash-high",
      "gemini-3.6-flash-low",
      "claude-opus-4-6-thinking",
    ],
    modelContextWindow: 200_000,
    notes:
      "Built-in Antigravity gateway (OpenAI Responses) for Codex — wire_api=responses; failover/catalog includes Gemini.",
    targets: ["codex"],
  },
  {
    id: "antigravity-gateway-external-pi",
    name: "Antigravity Gateway",
    protocolType: "anthropic",
    baseUrl: "http://127.0.0.1:8045",
    model: "claude-sonnet-4-6",
    failoverModels: [
      "gemini-3.6-flash-high",
      "gemini-3.6-flash-low",
      "claude-opus-4-6-thinking",
      "claude-sonnet-4-6-thinking",
    ],
    modelContextWindow: 200_000,
    notes:
      "External Antigravity-Manager for Pi (anthropic-messages; baseUrl is gateway root, SDK appends /v1/messages). Start AG Manager and fill its API key.",
    targets: ["pi"],
  },
  {
    id: "antigravity-builtin-pi",
    name: "Antigravity (Built-in)",
    protocolType: "anthropic",
    baseUrl: "http://127.0.0.1:15830",
    model: "claude-sonnet-4-6",
    failoverModels: [
      "gemini-3.6-flash-high",
      "gemini-3.6-flash-low",
      "claude-opus-4-6-thinking",
      "claude-sonnet-4-6-thinking",
    ],
    modelContextWindow: 200_000,
    notes:
      "内建 Antigravity 网关。Pi 写入 ~/.pi/agent/models.json（api: anthropic-messages，baseUrl 为网关根地址，SDK 会再拼 /v1/messages）；默认模型用网关 catalog id，无 Claude 角色映射。",
    targets: ["pi"],
  },
];

export function presetsForTarget(target: ProviderTarget): ProviderPreset[] {
  return PROVIDER_PRESETS.filter((preset) => preset.targets.includes(target));
}

export function mappingFromAntigravityPreset(
  defaultModel: string,
  target: ProviderTarget,
  options?: {
    geminiFlash?: string | null;
    geminiPro?: string | null;
    opus?: string | null;
  },
): ClaudeModelMapping {
  const sonnet = defaultModel.trim() || "claude-sonnet-4-6";
  const flash = options?.geminiFlash?.trim() || "gemini-3.6-flash-high";
  const opus = options?.opus?.trim() || "claude-opus-4-6-thinking";
  return {
    sonnet,
    opus,
    haiku: flash,
    fable: sonnet,
    subagent: target === "claude_code" ? flash : "",
  };
}

export function mappingFromModel(
  model: string,
  target: ProviderTarget,
): ClaudeModelMapping {
  const value = model.trim();
  return {
    sonnet: value,
    opus: value,
    haiku: value,
    fable: value,
    subagent: target === "claude_code" ? value : "",
  };
}

const MAPPING_ROLES = ["sonnet", "opus", "haiku", "fable", "subagent"] as const;

/**
 * When the default model changes, only fill roles that are empty or still equal
 * to the previous default. Custom role mappings must never be overwritten.
 */
export function syncMappingOnDefaultChange(
  current: ClaudeModelMapping | undefined,
  previousDefault: string,
  nextDefault: string,
  target: ProviderTarget,
): ClaudeModelMapping {
  const next = nextDefault.trim();
  const previous = previousDefault.trim();
  const includeSubagent = target === "claude_code";
  const result: ClaudeModelMapping = {
    sonnet: current?.sonnet ?? "",
    opus: current?.opus ?? "",
    haiku: current?.haiku ?? "",
    fable: current?.fable ?? "",
    subagent: includeSubagent ? (current?.subagent ?? "") : "",
  };
  if (!next || next === previous) {
    return result;
  }
  for (const role of MAPPING_ROLES) {
    if (role === "subagent" && !includeSubagent) {
      result.subagent = "";
      continue;
    }
    const value = (current?.[role] ?? "").trim();
    if (!value || value === previous) {
      result[role] = next;
    }
  }
  return result;
}
