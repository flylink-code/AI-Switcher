import { useQuery } from "@tanstack/react-query";
import { listKiroModels, type KiroModelInfo } from "@/services/kiro";

export const KIRO_MODEL_GROUPS = ["opus", "sonnet", "haiku", "gpt", "other"] as const;
export type KiroModelGroup = (typeof KIRO_MODEL_GROUPS)[number];

export const KIRO_MODELS_QUERY_KEY = ["kiro-models"] as const;

/** Tier a model id belongs to, for grouping. `-thinking` is not a catalog row. */
export function kiroModelGroup(id: string): KiroModelGroup {
  const lower = id.toLowerCase();
  if (lower.includes("opus")) return "opus";
  if (lower.includes("sonnet")) return "sonnet";
  if (lower.includes("haiku")) return "haiku";
  if (lower.startsWith("gpt-")) return "gpt";
  return "other";
}

/** Context window as a short label, e.g. 1M or 200K; empty when unknown. */
export function kiroContextLabel(model: KiroModelInfo): string {
  if (model.context >= 1_000_000) return `${model.context / 1_000_000}M`;
  if (model.context > 0) return `${Math.round(model.context / 1000)}K`;
  return "";
}

/** Cached Kiro model list (live after a refresh, built-in before). */
export function useKiroModels() {
  return useQuery({
    queryKey: KIRO_MODELS_QUERY_KEY,
    queryFn: listKiroModels,
    staleTime: 60_000,
  });
}
