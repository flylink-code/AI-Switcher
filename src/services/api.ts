/**
 * Thin wrapper over the Tauri IPC bridge.
 *
 * In a browser (no Tauri runtime), calls reject with a clear error so the UI
 * degrades gracefully rather than throwing on a missing global. This makes the
 * frontend buildable/runnable via `pnpm dev` for quick iteration even outside
 * the desktop shell.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  DbInfo,
  LivePrompt,
  McpImportSummary,
  McpServer,
  McpServerInput,
  RegistryMcpServer,
  McpTarget,
  PathsInfo,
  PromptDetail,
  PromptInfo,
  Provider,
  ConnectionTestResult,
  ModelDiscoveryResult,
  ProviderImportResult,
  ConfigBackup,
  ProviderInput,
  ProviderTarget,
  ProxyStatus,
  Skill,
  RepositorySkill,
  UsageDashboard,
  ModelPricing,
  ModelPricingInput,
  DesktopLocalizationActionResult,
  DesktopLocalizationPackInfo,
  DesktopLocalizationPackValidation,
  DesktopLocalizationStatus,
  LogMaintenanceResult,
  LogMaintenancePolicy,
  LogMaintenancePreview,
  ProxyLogListInput,
  PaginatedProxyLogs,
  ClaudeCodeVersionInfo,
  AutostartConfig,
  AutostartMode,
  CloseBehavior,
  SessionMessage,
  SessionProvider,
  SessionScanResult,
} from "@/types/backend";

async function getInvoke() {
  // Detect the Tauri runtime. The internal global is present only inside the app.
  const hasTauri =
    typeof window !== "undefined" &&
    // @tauri-apps/api checks for "__TAURI_INTERNALS__" (v2).
    // Using a defensive access to avoid a hard reference.
    Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
  if (!hasTauri) {
    throw new Error("Tauri runtime not available (running in a plain browser).");
  }
  return tauriInvoke;
}

export async function ping(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("ping", {});
}

export async function restartApp(): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("restart_app", {});
}

export async function getPaths(): Promise<PathsInfo> {
  const invoke = await getInvoke();
  return invoke<PathsInfo>("get_paths", {});
}

export async function getDbInfo(): Promise<DbInfo> {
  const invoke = await getInvoke();
  return invoke<DbInfo>("get_db_info", {});
}

export async function backupNow(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("backup_now", {});
}

// ---- Providers -------------------------------------------------------------

export async function listProviders(target: ProviderTarget): Promise<Provider[]> {
  const invoke = await getInvoke();
  return invoke<Provider[]>("list_providers", { target });
}

export async function getCurrentProvider(target: ProviderTarget): Promise<Provider | null> {
  const invoke = await getInvoke();
  return invoke<Provider | null>("get_current_provider", { target });
}

export async function createProvider(input: ProviderInput): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("create_provider", { input });
}

export async function updateProvider(input: ProviderInput): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("update_provider", { input });
}

export async function deleteProvider(id: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_provider", { id });
}

export async function switchProvider(id: string): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("switch_provider", { id });
}

export async function switchToOfficial(target: ProviderTarget): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("switch_to_official", { target });
}

export async function reorderProviders(orderedIds: string[], target: ProviderTarget): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("reorder_providers", { orderedIds, target });
}

export async function importLiveConfig(target: ProviderTarget): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("import_live_config", { target });
}

export async function testProviderConnection(id: string): Promise<ConnectionTestResult> {
  const invoke = await getInvoke();
  return invoke<ConnectionTestResult>("test_provider_connection", { id });
}

export async function testProviderInput(input: ProviderInput): Promise<ConnectionTestResult> {
  const invoke = await getInvoke();
  return invoke<ConnectionTestResult>("test_provider_input", { input });
}

export async function discoverProviderModels(id: string): Promise<ModelDiscoveryResult> {
  const invoke = await getInvoke();
  return invoke<ModelDiscoveryResult>("discover_provider_models", { id });
}

export async function getCachedProviderModels(id: string): Promise<ModelDiscoveryResult> {
  const invoke = await getInvoke();
  return invoke<ModelDiscoveryResult>("get_cached_provider_models", { id });
}

export async function discoverProviderModelsInput(input: ProviderInput): Promise<ModelDiscoveryResult> {
  const invoke = await getInvoke();
  return invoke<ModelDiscoveryResult>("discover_provider_models_input", { input });
}

export async function exportProviders(target: ProviderTarget): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("export_providers", { target });
}

export async function importProvidersJson(json: string): Promise<ProviderImportResult> {
  const invoke = await getInvoke();
  return invoke<ProviderImportResult>("import_providers_json", { json });
}

export async function listConfigBackups(target: ProviderTarget): Promise<ConfigBackup[]> {
  const invoke = await getInvoke();
  return invoke<ConfigBackup[]>("list_config_backups", { target });
}

export async function previewConfigBackup(target: ProviderTarget, name: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("preview_config_backup", { target, name });
}

export async function restoreConfigBackup(target: ProviderTarget, name: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("restore_config_backup", { target, name });
}

// ---- Local proxy ------------------------------------------------------------

export async function getProxyStatus(target?: ProviderTarget): Promise<ProxyStatus> {
  const startedAt = performance.now();
  const invoke = await getInvoke();
  try {
    return await invoke<ProxyStatus>("get_proxy_status", { target });
  } finally {
    void reportFrontendPerformance(
      "proxy_status_ipc",
      target ?? "claude_desktop",
      Math.round(performance.now() - startedAt),
    ).catch(() => undefined);
  }
}

export async function startProxy(port?: number, target?: ProviderTarget): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("start_proxy", { port, target });
}

export async function stopProxy(target?: ProviderTarget): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("stop_proxy", { target });
}

export async function setProxyPort(port: number, target?: ProviderTarget): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_proxy_port", { port, target });
}

// ---- MCP --------------------------------------------------------------------

export async function listMcpServers(): Promise<McpServer[]> {
  const invoke = await getInvoke();
  return invoke<McpServer[]>("list_mcp_servers", {});
}

export async function saveMcpServer(input: McpServerInput): Promise<McpServer> {
  const invoke = await getInvoke();
  return invoke<McpServer>("save_mcp_server", { input });
}

export async function deleteMcpServer(id: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_mcp_server", { id });
}

export async function toggleMcpServer(
  id: string,
  target: McpTarget,
  enabled: boolean,
): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("toggle_mcp_server", { id, target, enabled });
}

export async function importMcpServers(): Promise<McpImportSummary> {
  const invoke = await getInvoke();
  return invoke<McpImportSummary>("import_mcp_servers", {});
}

export async function searchMcpRegistry(query: string): Promise<RegistryMcpServer[]> {
  const invoke = await getInvoke();
  return invoke<RegistryMcpServer[]>("search_mcp_registry", { query });
}

export async function installMcpRegistryServer(
  name: string,
  enabledClaudeCode: boolean,
  enabledClaudeDesktop: boolean,
): Promise<McpServer> {
  const invoke = await getInvoke();
  return invoke<McpServer>("install_mcp_registry_server", {
    name,
    enabledClaudeCode,
    enabledClaudeDesktop,
  });
}

// ---- Prompt presets ---------------------------------------------------------

export async function listPrompts(): Promise<PromptInfo[]> {
  const invoke = await getInvoke();
  return invoke<PromptInfo[]>("list_prompts", {});
}

export async function readPrompt(name: string): Promise<PromptDetail> {
  const invoke = await getInvoke();
  return invoke<PromptDetail>("read_prompt", { name });
}

export async function savePrompt(name: string, content: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("save_prompt", { name, content });
}

export async function deletePrompt(name: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_prompt", { name });
}

export async function activatePrompt(name: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("activate_prompt", { name });
}

export async function readLivePrompt(): Promise<LivePrompt | null> {
  const invoke = await getInvoke();
  return invoke<LivePrompt | null>("read_live_prompt", {});
}


export async function importLivePrompt(name: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("import_live_prompt", { name });
}

// ---- Skills -----------------------------------------------------------------

export async function listSkills(): Promise<Skill[]> {
  const invoke = await getInvoke();
  return invoke<Skill[]>("list_skills", {});
}

export async function getSkillRepository(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("get_skill_repository", {});
}

export async function setSkillRepository(url: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("set_skill_repository", { url });
}

export async function listGithubRepositorySkills(url: string): Promise<RepositorySkill[]> {
  const invoke = await getInvoke();
  return invoke<RepositorySkill[]>("list_github_repository_skills", { url });
}

export async function installGithubRepositorySkills(url: string, paths: string[]): Promise<Skill[]> {
  const invoke = await getInvoke();
  return invoke<Skill[]>("install_github_repository_skills", { url, paths });
}

export async function installGithubSkill(url: string): Promise<Skill> {
  const invoke = await getInvoke();
  return invoke<Skill>("install_github_skill", { url });
}

export async function installZipSkill(path: string): Promise<Skill> {
  const invoke = await getInvoke();
  return invoke<Skill>("install_zip_skill", { path });
}

export async function setSkillEnabled(name: string, enabled: boolean): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_skill_enabled", { name, enabled });
}

export async function deleteSkill(name: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_skill", { name });
}

// ---- System -----------------------------------------------------------------

export async function getAutostartEnabled(): Promise<boolean> {
  const invoke = await getInvoke();
  return invoke<boolean>("get_autostart_enabled", {});
}

export async function setAutostartEnabled(enabled: boolean): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_autostart_enabled", { enabled });
}

export async function getAutostartConfig(): Promise<AutostartConfig> {
  const invoke = await getInvoke();
  return invoke<AutostartConfig>("get_autostart_config", {});
}

export async function setAutostartConfig(mode: AutostartMode): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_autostart_config", { mode });
}

export async function getCloseBehavior(): Promise<CloseBehavior> {
  const invoke = await getInvoke();
  return invoke<CloseBehavior>("get_close_behavior", {});
}

export async function setCloseBehavior(behavior: CloseBehavior): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_close_behavior", { behavior });
}

export async function resolveCloseRequest(
  action: Exclude<CloseBehavior, "ask">,
  remember: boolean,
): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("resolve_close_request", { action, remember });
}

export async function reportFrontendStartup(
  durationMs: number,
  reason: "completed" | "timeout" | "skipped",
  failures: string[],
): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("report_frontend_startup", { durationMs, reason, failures });
}

export async function reportFrontendPerformance(
  kind: string,
  name: string,
  durationMs: number,
): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("report_frontend_performance", {
    kind,
    name,
    durationMs: Math.max(0, Math.round(durationMs)),
  });
}

export async function getDesktopLocalizationStatus(): Promise<DesktopLocalizationStatus> {
  const invoke = await getInvoke();
  return invoke<DesktopLocalizationStatus>("get_desktop_localization_status", {});
}

export async function downloadDesktopLocalizationPack(): Promise<DesktopLocalizationPackInfo> {
  const invoke = await getInvoke();
  return invoke<DesktopLocalizationPackInfo>("download_desktop_localization_pack", {});
}

export async function selectDesktopLocalizationPack(): Promise<string | null> {
  const invoke = await getInvoke();
  return invoke<string | null>("select_desktop_localization_pack", {});
}

export async function validateDesktopLocalizationPack(
  path: string,
): Promise<DesktopLocalizationPackValidation> {
  const invoke = await getInvoke();
  return invoke<DesktopLocalizationPackValidation>(
    "validate_desktop_localization_pack",
    { path },
  );
}

export async function installDesktopLocalization(
  packPath: string,
): Promise<DesktopLocalizationActionResult> {
  const invoke = await getInvoke();
  return invoke<DesktopLocalizationActionResult>("install_desktop_localization", {
    packPath,
  });
}

export async function restoreDesktopLocalization(): Promise<DesktopLocalizationActionResult> {
  const invoke = await getInvoke();
  return invoke<DesktopLocalizationActionResult>("restore_desktop_localization", {});
}

// ---- Usage ------------------------------------------------------------------

export async function getUsageDashboard(days = 30): Promise<UsageDashboard> {
  const invoke = await getInvoke();
  return invoke<UsageDashboard>("get_usage_dashboard", { days });
}

export async function listModelPricing(): Promise<ModelPricing[]> {
  const invoke = await getInvoke();
  return invoke<ModelPricing[]>("list_model_pricing", {});
}

export async function saveModelPricing(input: ModelPricingInput): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("save_model_pricing", { input });
}

export async function deleteModelPricing(model: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_model_pricing", { model });
}

export async function getLogMaintenancePolicy(): Promise<LogMaintenancePolicy> {
  const invoke = await getInvoke();
  return invoke<LogMaintenancePolicy>("get_log_maintenance_policy");
}

export async function saveLogMaintenancePolicy(policy: LogMaintenancePolicy): Promise<LogMaintenancePolicy> {
  const invoke = await getInvoke();
  return invoke<LogMaintenancePolicy>("save_log_maintenance_policy", { policy });
}

export async function previewProxyLogMaintenance(policy?: LogMaintenancePolicy): Promise<LogMaintenancePreview> {
  const invoke = await getInvoke();
  return invoke<LogMaintenancePreview>("preview_proxy_log_maintenance", { policy });
}

export async function maintainProxyLogs(vacuum = false): Promise<LogMaintenanceResult> {
  const invoke = await getInvoke();
  return invoke<LogMaintenanceResult>("maintain_proxy_logs", { vacuum });
}

export async function listProxyRequestLogs(input: ProxyLogListInput = {}): Promise<PaginatedProxyLogs> {
  const invoke = await getInvoke();
  return invoke<PaginatedProxyLogs>("list_proxy_request_logs_cmd", { input });
}

// ---- About / tools ----------------------------------------------------------

export async function getClaudeCodeVersion(includeLatest = true): Promise<ClaudeCodeVersionInfo> {
  const invoke = await getInvoke();
  return invoke<ClaudeCodeVersionInfo>("get_claude_code_version", { includeLatest });
}

export async function runClaudeCodeUpdate(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("run_claude_code_update", {});
}

export async function scanSessions(
  provider?: SessionProvider,
): Promise<SessionScanResult> {
  const invoke = await getInvoke();
  return invoke<SessionScanResult>("scan_sessions", { provider });
}

export async function searchSessionContents(
  query: string,
  provider?: SessionProvider,
  limit = 200,
): Promise<SessionScanResult> {
  const invoke = await getInvoke();
  return invoke<SessionScanResult>("search_session_contents", {
    query,
    provider,
    limit,
  });
}

export async function loadSessionMessages(
  provider: SessionProvider,
  sourcePath: string,
): Promise<SessionMessage[]> {
  const invoke = await getInvoke();
  return invoke<SessionMessage[]>("load_session_messages", {
    provider,
    sourcePath,
  });
}

export async function setAppLanguage(language: "zh-CN" | "en-US"): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_app_language", { language });
}
