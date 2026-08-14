import { call } from "./ipc";
import type {
  ClaudeCodeVersionInfo,
  CodexCliVersionInfo,
  DshCliVersionInfo,
  NodeRuntimeStatus,
  OpenCodeCliVersionInfo,
  OpenCodeDesktopStatus,
  PiCliVersionInfo,
} from "@/types/backend";

export async function getClaudeCodeVersion(includeLatest = true): Promise<ClaudeCodeVersionInfo> {
  return call<ClaudeCodeVersionInfo>("get_claude_code_version", { includeLatest });
}

export async function runClaudeCodeUpdate(): Promise<string> {
  return call<string>("run_claude_code_update", {});
}

export async function getCodexCliVersion(includeLatest = true): Promise<CodexCliVersionInfo> {
  return call<CodexCliVersionInfo>("get_codex_cli_version", { includeLatest });
}

export async function runCodexCliUpdate(): Promise<string> {
  return call<string>("run_codex_cli_update", {});
}

export async function getOpenCodeCliVersion(includeLatest = true): Promise<OpenCodeCliVersionInfo> {
  return call<OpenCodeCliVersionInfo>("get_opencode_cli_version", { includeLatest });
}

export async function runOpenCodeCliUpdate(): Promise<string> {
  return call<string>("run_opencode_cli_update", {});
}

export async function getPiCliVersion(includeLatest = true): Promise<PiCliVersionInfo> {
  return call<PiCliVersionInfo>("detect_pi_cli", { includeLatest });
}

export async function runPiCliUpdate(): Promise<string> {
  return call<string>("install_pi_cli", {});
}

export async function getDshCliVersion(includeLatest = true): Promise<DshCliVersionInfo> {
  return call<DshCliVersionInfo>("get_dsh_cli_version", { includeLatest });
}

export async function runDshCliUpdate(): Promise<string> {
  return call<string>("run_dsh_cli_update", {});
}

export async function getPiSettings(): Promise<Record<string, unknown>> {
  return call<Record<string, unknown>>("get_pi_settings", {});
}

export async function updatePiSettings(
  defaultProvider?: string | null,
  defaultModel?: string | null,
  defaultThinkingLevel?: string | null,
  extraPatch?: Record<string, unknown> | null,
): Promise<void> {
  return call<void>("update_pi_settings", {
    defaultProvider: defaultProvider ?? null,
    defaultModel: defaultModel ?? null,
    defaultThinkingLevel: defaultThinkingLevel ?? null,
    extraPatch: extraPatch ?? null,
  });
}


export async function getOpenCodeDesktopStatus(): Promise<OpenCodeDesktopStatus> {
  return call<OpenCodeDesktopStatus>("get_opencode_desktop_status", {});
}

export async function getNodeRuntimeStatus(): Promise<NodeRuntimeStatus> {
  return call<NodeRuntimeStatus>("get_node_runtime_status", {});
}

export async function ensureNodeRuntimeViaFnm(): Promise<NodeRuntimeStatus> {
  return call<NodeRuntimeStatus>("ensure_node_runtime_via_fnm", {});
}
