import type { ProviderTarget } from "@/types/backend";

/** Agents kept in the backend/schema but hidden from every product surface. */
export const RETIRED_UI_AGENTS: readonly ProviderTarget[] = ["claude_desktop", "dsh"];

export function isAgentUiEnabled(target: ProviderTarget): boolean {
  return !RETIRED_UI_AGENTS.includes(target);
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
