import { call, getInvoke } from "./ipc";
import type {
  LogMaintenancePolicy,
  LogMaintenancePreview,
  LogMaintenanceResult,
  ModelPricing,
  ModelPricingInput,
  PaginatedProxyLogs,
  PricingImportPreview,
  ProviderTarget,
  ProxyLogListInput,
  UsageDashboard,
} from "@/types/backend";

export async function getUsageDashboard(
  range: { days?: number; hours?: number; today?: boolean } | number = 30,
  source: ProviderTarget | "all" | "antigravity" = "all",
): Promise<UsageDashboard> {
  const invoke = await getInvoke();
  const params =
    typeof range === "number"
      ? { days: range }
      : {
          days: range.days,
          hours: range.hours,
          today: range.today,
        };
  return invoke<UsageDashboard>("get_usage_dashboard", {
    ...params,
    source: source === "all" ? undefined : source,
  });
}

export interface CodexSessionSyncResult {
  scannedFiles: number;
  insertedRows: number;
  skippedRows: number;
  message: string;
}

export async function syncCodexSessionUsage(): Promise<CodexSessionSyncResult> {
  return call<CodexSessionSyncResult>("sync_codex_session_usage_cmd", {});
}

export async function rebuildCodexSessionUsage(): Promise<CodexSessionSyncResult> {
  return call<CodexSessionSyncResult>("rebuild_codex_session_usage_cmd", {});
}

export async function syncClaudeCodeSessionUsage(): Promise<CodexSessionSyncResult> {
  return call<CodexSessionSyncResult>("sync_claude_code_session_usage_cmd", {});
}

export async function rebuildClaudeCodeSessionUsage(): Promise<CodexSessionSyncResult> {
  return call<CodexSessionSyncResult>("rebuild_claude_code_session_usage_cmd", {});
}

export interface OpenCodeSessionSyncResult {
  scannedSessions: number;
  insertedRows: number;
  skippedRows: number;
  message: string;
}

export async function syncOpenCodeSessionUsage(): Promise<OpenCodeSessionSyncResult> {
  return call<OpenCodeSessionSyncResult>("sync_opencode_session_usage_cmd", {});
}

export async function rebuildOpenCodeSessionUsage(): Promise<OpenCodeSessionSyncResult> {
  return call<OpenCodeSessionSyncResult>("rebuild_opencode_session_usage_cmd", {});
}

export async function listModelPricing(): Promise<ModelPricing[]> {
  return call<ModelPricing[]>("list_model_pricing", {});
}

export async function saveModelPricing(input: ModelPricingInput): Promise<void> {
  return call<void>("save_model_pricing", { input });
}

export async function deleteModelPricing(model: string): Promise<void> {
  return call<void>("delete_model_pricing", { model });
}

export async function exportModelPricingXlsx(destinationPath: string): Promise<string> {
  return call<string>("export_model_pricing_xlsx", { destinationPath });
}

export async function previewModelPricingXlsx(sourcePath: string): Promise<PricingImportPreview> {
  return call<PricingImportPreview>("preview_model_pricing_xlsx", { sourcePath });
}

export async function importModelPricingXlsx(sourcePath: string): Promise<PricingImportPreview> {
  return call<PricingImportPreview>("import_model_pricing_xlsx", { sourcePath });
}

export async function getLogMaintenancePolicy(): Promise<LogMaintenancePolicy> {
  return call<LogMaintenancePolicy>("get_log_maintenance_policy");
}

export async function saveLogMaintenancePolicy(policy: LogMaintenancePolicy): Promise<LogMaintenancePolicy> {
  return call<LogMaintenancePolicy>("save_log_maintenance_policy", { policy });
}

export async function previewProxyLogMaintenance(policy?: LogMaintenancePolicy): Promise<LogMaintenancePreview> {
  return call<LogMaintenancePreview>("preview_proxy_log_maintenance", { policy });
}

export async function maintainProxyLogs(vacuum = false): Promise<LogMaintenanceResult> {
  return call<LogMaintenanceResult>("maintain_proxy_logs", { vacuum });
}

export async function listProxyRequestLogs(input: ProxyLogListInput = {}): Promise<PaginatedProxyLogs> {
  return call<PaginatedProxyLogs>("list_proxy_request_logs_cmd", { input });
}

// ---- About / tools ----------------------------------------------------------
