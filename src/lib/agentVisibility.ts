import type { ProviderTarget } from "@/types/backend";

/** Agents kept in the backend/schema but hidden from every product surface. */
export const RETIRED_UI_AGENTS: readonly ProviderTarget[] = ["cline", "dsh"];

/** Agents the host OS cannot run; filled once at startup, before the first render. */
const UNSUPPORTED_ON_PLATFORM: Record<string, readonly ProviderTarget[]> = {
  linux: ["claude_desktop"],
};
let platformHidden: readonly ProviderTarget[] = [];

export function applyPlatformAgentVisibility(platform: string): void {
  platformHidden = UNSUPPORTED_ON_PLATFORM[platform] ?? [];
}

export function isAgentUiEnabled(target: ProviderTarget): boolean {
  return !RETIRED_UI_AGENTS.includes(target) && !platformHidden.includes(target);
}

export function filterUiAgents<T extends ProviderTarget>(agents: readonly T[]): T[] {
  return agents.filter((agent) => isAgentUiEnabled(agent));
}

export function coerceUiAgent(
  target: ProviderTarget,
  visible: readonly ProviderTarget[],
): ProviderTarget {
  if (isAgentUiEnabled(target) && visible.includes(target)) {
    return target;
  }
  if (visible.includes("claude_code")) {
    return "claude_code";
  }
  return visible[0] ?? "claude_code";
}
