/**
 * Quota and balance types for providers and official subscriptions.
 * Mirrors `src-tauri/src/quota/types.rs`.
 */

export interface QuotaTier {
  name: string;
  utilization: number;
  resets_at?: string | null;
  used_value?: number | null;
  max_value?: number | null;
}

export interface ExtraUsage {
  is_enabled: boolean;
  monthly_limit?: number | null;
  used?: number | null;
  currency?: string | null;
}

export type ProviderQuotaResult =
  | {
      kind: "subscription";
      provider_type: string;
      plan_name?: string | null;
      tiers: QuotaTier[];
      extra_usage?: ExtraUsage | null;
      queried_at: number;
    }
  | {
      kind: "balance";
      provider_type: string;
      currency: string;
      total_balance: number;
      granted_balance?: number | null;
      topped_up_balance?: number | null;
      is_available: boolean;
      queried_at: number;
    }
  | {
      kind: "unsupported";
      reason?: string | null;
    }
  | {
      kind: "error";
      code: string;
      message: string;
      queried_at: number;
    };
