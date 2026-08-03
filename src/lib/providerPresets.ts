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
  notes?: string;
  /** Targets that should show this preset. */
  targets: ProviderTarget[];
}

const ALL_TARGETS: ProviderTarget[] = ["claude_code", "claude_desktop", "codex"];

/** Built-in quick-fill presets for common third-party gateways. */
export const PROVIDER_PRESETS: ProviderPreset[] = [
  {
    id: "deepseek-anthropic",
    name: "DeepSeek",
    protocolType: "anthropic",
    baseUrl: "https://api.deepseek.com/anthropic",
    model: "deepseek-v4-pro",
    notes: "DeepSeek Anthropic-compatible gateway",
    targets: ALL_TARGETS,
  },
  {
    id: "deepseek-openai",
    name: "DeepSeek (OpenAI)",
    protocolType: "openai_chat",
    baseUrl: "https://api.deepseek.com",
    model: "deepseek-v4-pro",
    notes: "DeepSeek OpenAI-compatible Chat Completions",
    targets: ALL_TARGETS,
  },
  {
    id: "kimi-cn",
    name: "Kimi",
    protocolType: "openai_chat",
    baseUrl: "https://api.moonshot.cn/v1",
    model: "kimi-k3",
    notes: "Moonshot / Kimi China OpenAI-compatible API",
    targets: ALL_TARGETS,
  },
  {
    id: "kimi-intl",
    name: "Kimi (Intl)",
    protocolType: "openai_chat",
    baseUrl: "https://api.moonshot.ai/v1",
    model: "kimi-k3",
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
];

export function presetsForTarget(target: ProviderTarget): ProviderPreset[] {
  return PROVIDER_PRESETS.filter((preset) => preset.targets.includes(target));
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
