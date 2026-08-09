import { call } from "./ipc";
import type {
  ClaudeCodeVersionInfo,
  CodexCliVersionInfo,
  NodeRuntimeStatus,
  OpenCodeCliVersionInfo,
  OpenCodeDesktopStatus,
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

export async function getOpenCodeDesktopStatus(): Promise<OpenCodeDesktopStatus> {
  return call<OpenCodeDesktopStatus>("get_opencode_desktop_status", {});
}

export async function getNodeRuntimeStatus(): Promise<NodeRuntimeStatus> {
  return call<NodeRuntimeStatus>("get_node_runtime_status", {});
}

export async function ensureNodeRuntimeViaFnm(): Promise<NodeRuntimeStatus> {
  return call<NodeRuntimeStatus>("ensure_node_runtime_via_fnm", {});
}
