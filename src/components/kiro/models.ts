/** Matches `kiro::models::catalog_ids`. `-thinking` is not a catalog row. */
export const KIRO_CATALOG = [
  { group: "sonnet", id: "claude-sonnet-4.5" },
  { group: "sonnet", id: "claude-sonnet-4.6" },
  { group: "sonnet", id: "claude-sonnet-4.8" },
  { group: "opus", id: "claude-opus-4.5" },
  { group: "opus", id: "claude-opus-4.6" },
  { group: "opus", id: "claude-opus-4.7" },
  { group: "opus", id: "claude-opus-4.8" },
  { group: "haiku", id: "claude-haiku-4.5" },
  { group: "fable", id: "claude-fable-5" },
] as const;

export const KIRO_MODEL_GROUPS = ["sonnet", "opus", "haiku", "fable"] as const;
