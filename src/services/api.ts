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
  DoctorReport,
  DoctorRepairResult,
  VisibilityRepairResult,
  DataRootInfo,
  LivePrompt,
  McpImportSummary,
  McpOauthStatus,
  McpDesktopConflictStatus,
  McpServer,
  McpServerInput,
  RegistryMcpServer,
  McpTarget,
  PathsInfo,
  PromptDetail,
  PromptInfo,
  PromptTarget,
  Provider,
  ConnectionTestResult,
  EndpointSpeedtestResult,
  ModelDiscoveryResult,
    ProviderImportResult,
    ImportPreview,
    DeeplinkImportResult,
    ConfigBackup,
    LibraryBackupInfo,
    LibraryArchivePreview,
    LibraryRestoreResult,
  ProviderInput,
  ProviderTarget,
  CodexAuthStatus,
  CodexProviderSyncResult,
  CodexOauthAccount,
  CodexOauthDeviceStart,
  CodexOauthPollResult,
  SwitchProviderResult,
  ProxyStatus,
  ManagedAppRuntimeStatus,
  Skill,
  SkillTarget,
  SkillUpdateStatus,
  RepositorySkill,
  SkillRepositorySnapshot,
  UnmanagedSkill,
  Agent,
  AgentDraft,
  CodexPluginsSnapshot,
  CodexMarketplaceListResult,
  CodexPluginCommandResult,
  CodexWebSearchMode,
  CodexWebSearchSnapshot,
  UsageDashboard,
  ModelPricing,
  ModelPricingInput,
  DesktopLocalizationActionResult,
  DesktopLocalizationPackInfo,
  DesktopLocalizationPackValidation,
  DesktopLocalizationStatus,
  LocalizationHubStatus,
  LogMaintenanceResult,
  LogMaintenancePolicy,
  LogMaintenancePreview,
  ProxyLogListInput,
  PaginatedProxyLogs,
  ClaudeCodeVersionInfo,
  CodexCliVersionInfo,
  NodeRuntimeStatus,
  AutostartConfig,
  AutostartMode,
  CloseBehavior,
  SessionMessage,
  SessionMeta,
  SessionArchiveInfo,
  SessionBatchBackupInfo,
  SessionBatchExportInfo,
  SyncPreview,
  SyncPushResult,
  SyncTarget,
  SessionProvider,
  SessionScanResult,
  AppUpdateInfo,
  UpdateMirrorSettings,
  PricingImportPreview,
  Profile,
  ProfilePayload,
  ProfileSnapshotScopes,
  ApplyProfileResult,
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

export async function runEnvironmentDoctor(): Promise<DoctorReport> {
  const invoke = await getInvoke();
  return invoke<DoctorReport>("run_environment_doctor", {});
}

export async function repairEnvironmentVisibility(): Promise<VisibilityRepairResult> {
  const invoke = await getInvoke();
  return invoke<VisibilityRepairResult>("repair_environment_visibility", {});
}

export async function repairDoctorCheck(id: string): Promise<DoctorRepairResult> {
  const invoke = await getInvoke();
  return invoke<DoctorRepairResult>("repair_doctor_check", { id });
}

export async function getDataRoot(): Promise<DataRootInfo> {
  const invoke = await getInvoke();
  return invoke<DataRootInfo>("get_data_root", {});
}

export async function migrateDataRoot(targetPath: string): Promise<DataRootInfo> {
  const invoke = await getInvoke();
  return invoke<DataRootInfo>("migrate_data_root", { targetPath });
}

export async function backupNow(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("backup_now", {});
}

export async function exportLibraryBackup(
  destinationDir?: string | null,
  includeCredentials = false,
): Promise<LibraryBackupInfo> {
  const invoke = await getInvoke();
  return invoke<LibraryBackupInfo>("export_library_backup", {
    destinationDir: destinationDir?.trim() ? destinationDir : null,
    includeCredentials,
  });
}

export async function previewLibraryBackup(archivePath: string): Promise<LibraryArchivePreview> {
  const invoke = await getInvoke();
  return invoke<LibraryArchivePreview>("preview_library_backup", { archivePath });
}

export async function restoreLibraryBackup(archivePath: string): Promise<LibraryRestoreResult> {
  const invoke = await getInvoke();
  return invoke<LibraryRestoreResult>("restore_library_backup", { archivePath });
}

export async function findLatestLibraryArchive(directory: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("find_latest_library_archive_cmd", { directory });
}

// ---- Cross-environment sync -------------------------------------------------

export async function listSyncTargets(): Promise<SyncTarget[]> {
  const invoke = await getInvoke();
  return invoke<SyncTarget[]>("list_sync_targets", {});
}

export async function saveSyncTarget(target: SyncTarget): Promise<SyncTarget> {
  const invoke = await getInvoke();
  return invoke<SyncTarget>("save_sync_target", { target });
}

export async function deleteSyncTarget(id: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_sync_target", { id });
}

export async function discoverWslDistributions(): Promise<string[]> {
  const invoke = await getInvoke();
  return invoke<string[]>("discover_wsl_distributions", {});
}

export async function previewSync(targetId: string): Promise<SyncPreview> {
  const invoke = await getInvoke();
  return invoke<SyncPreview>("preview_sync", { targetId });
}

export async function pushSyncArchive(
  targetId: string,
  password?: string | null,
  includeApiKeys = false,
): Promise<SyncPushResult> {
  const invoke = await getInvoke();
  return invoke<SyncPushResult>("push_sync_archive", {
    targetId,
    password: password?.trim() ? password : null,
    includeApiKeys,
  });
}

// ---- Providers -------------------------------------------------------------

export async function listProviders(target: ProviderTarget): Promise<Provider[]> {
  const invoke = await getInvoke();
  return invoke<Provider[]>("list_providers", { target });
}

export async function getCodexAuthStatus(): Promise<CodexAuthStatus> {
  const invoke = await getInvoke();
  return invoke<CodexAuthStatus>("get_codex_auth_status");
}

export async function startCodexOauthLogin(): Promise<CodexOauthDeviceStart> {
  const invoke = await getInvoke();
  return invoke<CodexOauthDeviceStart>("start_codex_oauth_login", {});
}

export async function pollCodexOauthLogin(deviceCode: string): Promise<CodexOauthPollResult> {
  const invoke = await getInvoke();
  return invoke<CodexOauthPollResult>("poll_codex_oauth_login", { deviceCode });
}

export async function listCodexOauthAccounts(): Promise<CodexOauthAccount[]> {
  const invoke = await getInvoke();
  return invoke<CodexOauthAccount[]>("list_codex_oauth_accounts", {});
}

export async function ensureCodexOauthProvider(
  target: ProviderTarget,
  accountId: string,
  model?: string,
): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("ensure_codex_oauth_provider", {
    target,
    accountId,
    model: model ?? null,
  });
}

export interface AntigravityQuotaBucket {
  bucketId: string;
  window: string;
  remainingFraction: number;
  resetTime: string;
  displayName?: string | null;
}

export interface AntigravityQuotaGroup {
  displayName: string;
  buckets: AntigravityQuotaBucket[];
}

export interface AntigravityModelQuota {
  name: string;
  percentage: number;
  resetTime: string;
  displayName?: string | null;
}

export interface AntigravityQuotaSnapshot {
  models: AntigravityModelQuota[];
  groups: AntigravityQuotaGroup[];
  lastUpdated: number;
  isForbidden: boolean;
  forbiddenReason?: string | null;
  subscriptionTier?: string | null;
}

export interface AntigravityAccountPublic {
  id: string;
  email: string;
  name?: string | null;
  disabled: boolean;
  disabledReason?: string | null;
  isActive: boolean;
  createdAt: number;
  lastUsed: number;
  healthScore: number;
  cooldownUntil?: number | null;
  remainingQuota?: number | null;
  hasProjectId: boolean;
  tokenExpiresAt: number;
  subscriptionTier?: string | null;
  quota5hPercent?: number | null;
  quotaWeeklyPercent?: number | null;
  quotaGemini5hPercent?: number | null;
  quotaGeminiWeeklyPercent?: number | null;
  quotaClaude5hPercent?: number | null;
  quotaClaudeWeeklyPercent?: number | null;
  quotaUpdatedAt?: number | null;
  quotaForbidden?: boolean;
  quota?: AntigravityQuotaSnapshot | null;
}

export interface AntigravityGatewayStatus {
  running: boolean;
  port: number;
  apiKey: string;
  accountCount: number;
  baseUrl: string;
  outboundMode?: string;
  outboundProxyUrl?: string;
  effectiveOutboundProxy?: string | null;
}

export interface AntigravityDefaults {
  defaultPort: number;
  externalPort: number;
  port: number;
  baseUrl: string;
  apiKey: string;
  running: boolean;
  models?: AntigravityCatalogModel[];
  defaultModel?: string;
  geminiFlash?: string | null;
  geminiPro?: string | null;
  reasoningLevel?: string | null;
}

export interface AntigravityCatalogModel {
  id: string;
  displayName?: string | null;
}

export async function listAntigravityAccounts(): Promise<AntigravityAccountPublic[]> {
  const invoke = await getInvoke();
  return invoke<AntigravityAccountPublic[]>("list_antigravity_accounts");
}

export async function listAntigravityModels(): Promise<AntigravityCatalogModel[]> {
  const invoke = await getInvoke();
  return invoke<AntigravityCatalogModel[]>("list_antigravity_models");
}

export async function importAntigravityAccounts(json: string): Promise<number> {
  const invoke = await getInvoke();
  return invoke<number>("import_antigravity_accounts", { json });
}

export async function startAntigravityOauthLogin(): Promise<AntigravityAccountPublic> {
  const invoke = await getInvoke();
  return invoke<AntigravityAccountPublic>("start_antigravity_oauth_login");
}

export async function removeAntigravityAccount(accountId: string): Promise<void> {
  const invoke = await getInvoke();
  await invoke("remove_antigravity_account", { accountId });
}

export async function setAntigravityActiveAccount(accountId: string): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_antigravity_active_account", { accountId });
}

export async function getAntigravityGatewayStatus(): Promise<AntigravityGatewayStatus> {
  const invoke = await getInvoke();
  return invoke<AntigravityGatewayStatus>("get_antigravity_gateway_status");
}

export async function setAntigravityGatewayPort(port: number): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_antigravity_gateway_port", { port });
}

export async function setAntigravityGatewayApiKey(apiKey: string): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_antigravity_gateway_api_key", { apiKey });
}

export async function setAntigravityReasoningLevel(
  level: "low" | "medium" | "high" | null,
): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_antigravity_reasoning_level", { level });
}

export async function setAntigravityOutboundProxy(
  mode: "direct" | "system" | "custom",
  proxyUrl?: string,
): Promise<AntigravityGatewayStatus> {
  const invoke = await getInvoke();
  return invoke<AntigravityGatewayStatus>("set_antigravity_outbound_proxy", {
    mode,
    proxyUrl: proxyUrl ?? null,
  });
}

export async function startAntigravityGateway(
  port?: number,
): Promise<AntigravityGatewayStatus> {
  const invoke = await getInvoke();
  return invoke<AntigravityGatewayStatus>("start_antigravity_gateway", {
    port: port ?? null,
  });
}

export async function stopAntigravityGateway(): Promise<AntigravityGatewayStatus> {
  const invoke = await getInvoke();
  return invoke<AntigravityGatewayStatus>("stop_antigravity_gateway");
}

export async function refreshAntigravityAccountQuota(
  accountId: string,
): Promise<AntigravityAccountPublic> {
  const invoke = await getInvoke();
  return invoke<AntigravityAccountPublic>("refresh_antigravity_account_quota", {
    accountId,
  });
}

export async function refreshAntigravityQuotas(): Promise<AntigravityAccountPublic[]> {
  const invoke = await getInvoke();
  return invoke<AntigravityAccountPublic[]>("refresh_antigravity_quotas");
}

export async function ensureAntigravityProvider(
  target: ProviderTarget,
  model?: string,
): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("ensure_antigravity_provider", {
    target,
    model: model ?? null,
  });
}

export async function getAntigravityDefaults(): Promise<AntigravityDefaults> {
  const invoke = await getInvoke();
  return invoke<AntigravityDefaults>("get_antigravity_defaults");
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

export async function switchProvider(id: string): Promise<SwitchProviderResult> {
  const invoke = await getInvoke();
  return invoke<SwitchProviderResult>("switch_provider", { id });
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

export async function speedtestProviderEndpoint(id: string): Promise<EndpointSpeedtestResult> {
  const invoke = await getInvoke();
  return invoke<EndpointSpeedtestResult>("speedtest_provider_endpoint", { id });
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

export async function previewImportText(text: string): Promise<ImportPreview> {
  const invoke = await getInvoke();
  return invoke<ImportPreview>("preview_import_text", { text });
}

export async function confirmImportPreview(preview: ImportPreview): Promise<DeeplinkImportResult> {
  const invoke = await getInvoke();
  return invoke<DeeplinkImportResult>("confirm_import_preview", { preview });
}

export async function buildProviderDeeplink(providerId: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("build_provider_deeplink", { providerId });
}

export async function buildMcpDeeplink(serverId: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("build_mcp_deeplink", { serverId });
}

export async function listConfigBackups(
  target: ProviderTarget,
  directory?: string | null,
): Promise<ConfigBackup[]> {
  const invoke = await getInvoke();
  return invoke<ConfigBackup[]>("list_config_backups", {
    target,
    directory: directory?.trim() ? directory : null,
  });
}

export async function previewConfigBackup(
  target: ProviderTarget,
  name: string,
  directory?: string | null,
): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("preview_config_backup", {
    target,
    name,
    directory: directory?.trim() ? directory : null,
  });
}

export async function restoreConfigBackup(
  target: ProviderTarget,
  name: string,
  directory?: string | null,
): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("restore_config_backup", {
    target,
    name,
    directory: directory?.trim() ? directory : null,
  });
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

export async function getManagedAppsRuntimeStatus(): Promise<ManagedAppRuntimeStatus> {
  const invoke = await getInvoke();
  return invoke<ManagedAppRuntimeStatus>("get_managed_apps_runtime_status");
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

export async function getProxyFailoverEnabled(): Promise<boolean> {
  const invoke = await getInvoke();
  return invoke<boolean>("get_proxy_failover_enabled", {});
}

export async function setProxyFailoverEnabled(enabled: boolean): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_proxy_failover_enabled", { enabled });
}

export async function getProxyRetryableStatusCodes(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("get_proxy_retryable_status_codes", {});
}

export async function setProxyRetryableStatusCodes(value: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_proxy_retryable_status_codes", { value });
}

export async function getProxyStreamingIdleTimeoutSecs(): Promise<number> {
  const invoke = await getInvoke();
  return invoke<number>("get_proxy_streaming_idle_timeout_secs", {});
}

export async function setProxyStreamingIdleTimeoutSecs(secs: number): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_proxy_streaming_idle_timeout_secs", { secs });
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

export async function reorderMcpServers(orderedIds: string[]): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("reorder_mcp_servers", { orderedIds });
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

export async function getMcpOauthStatus(): Promise<McpOauthStatus> {
  const invoke = await getInvoke();
  return invoke<McpOauthStatus>("get_mcp_oauth_status", {});
}

export async function clearMcpOauth(serverNames: string[] = []): Promise<McpOauthStatus> {
  const invoke = await getInvoke();
  return invoke<McpOauthStatus>("clear_mcp_oauth", { input: { serverNames } });
}

export async function getMcpDesktopConflictStatus(): Promise<McpDesktopConflictStatus> {
  const invoke = await getInvoke();
  return invoke<McpDesktopConflictStatus>("get_mcp_desktop_conflict_status", {});
}

// ---- Prompt presets ---------------------------------------------------------

export async function listPrompts(target: PromptTarget = "claude_code"): Promise<PromptInfo[]> {
  const invoke = await getInvoke();
  return invoke<PromptInfo[]>("list_prompts", { target });
}

export async function readPrompt(name: string, target: PromptTarget = "claude_code"): Promise<PromptDetail> {
  const invoke = await getInvoke();
  return invoke<PromptDetail>("read_prompt", { name, target });
}

export async function savePrompt(name: string, content: string, target: PromptTarget = "claude_code"): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("save_prompt", { name, content, target });
}

export async function renamePrompt(
  oldName: string,
  newName: string,
  target: PromptTarget = "claude_code",
): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("rename_prompt", { oldName, newName, target });
}

export async function deletePrompt(name: string, target: PromptTarget = "claude_code"): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_prompt", { name, target });
}

export async function activatePrompt(name: string, target: PromptTarget = "claude_code"): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("activate_prompt", { name, target });
}

export async function readLivePrompt(target: PromptTarget = "claude_code"): Promise<LivePrompt | null> {
  const invoke = await getInvoke();
  return invoke<LivePrompt | null>("read_live_prompt", { target });
}


export async function importLivePrompt(name: string, target: PromptTarget = "claude_code"): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("import_live_prompt", { name, target });
}

// ---- Skills -----------------------------------------------------------------

export async function listSkills(target?: SkillTarget): Promise<Skill[]> {
  const invoke = await getInvoke();
  return invoke<Skill[]>("list_skills", { target });
}

export async function scanUnmanagedSkills(target?: SkillTarget): Promise<UnmanagedSkill[]> {
  const invoke = await getInvoke();
  return invoke<UnmanagedSkill[]>("scan_unmanaged_skills", { target });
}

export async function registerUnmanagedSkill(path: string, target?: SkillTarget): Promise<Skill> {
  const invoke = await getInvoke();
  return invoke<Skill>("register_unmanaged_skill", { path, target });
}

export async function ignoreUnmanagedSkill(path: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("ignore_unmanaged_skill", { path });
}

// ---- Agents -----------------------------------------------------------------

export async function listAgents(): Promise<Agent[]> {
  const invoke = await getInvoke();
  return invoke<Agent[]>("list_agents", {});
}

export async function saveAgent(draft: AgentDraft): Promise<Agent> {
  const invoke = await getInvoke();
  return invoke<Agent>("save_agent", { draft });
}

export async function setAgentEnabled(name: string, enabled: boolean): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_agent_enabled", { name, enabled });
}

export async function deleteAgent(name: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_agent", { name });
}

export async function installZipAgent(path: string): Promise<Agent[]> {
  const invoke = await getInvoke();
  return invoke<Agent[]>("install_zip_agent", { path });
}

// ---- Codex Plugins ----------------------------------------------------------

export async function listCodexPlugins(): Promise<CodexPluginsSnapshot> {
  const invoke = await getInvoke();
  return invoke<CodexPluginsSnapshot>("list_codex_plugins", {});
}

export async function setCodexPluginEnabled(pluginId: string, enabled: boolean): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_codex_plugin_enabled", { pluginId, enabled });
}

export async function listCodexPluginMarketplaces(): Promise<CodexMarketplaceListResult> {
  const invoke = await getInvoke();
  return invoke<CodexMarketplaceListResult>("list_codex_plugin_marketplaces", {});
}

export async function addCodexPluginMarketplace(source: string): Promise<CodexPluginCommandResult> {
  const invoke = await getInvoke();
  return invoke<CodexPluginCommandResult>("add_codex_plugin_marketplace", { source });
}

export async function removeCodexPluginMarketplace(name: string): Promise<CodexPluginCommandResult> {
  const invoke = await getInvoke();
  return invoke<CodexPluginCommandResult>("remove_codex_plugin_marketplace", { name });
}

export async function uninstallCodexPlugin(pluginId: string): Promise<CodexPluginCommandResult> {
  const invoke = await getInvoke();
  return invoke<CodexPluginCommandResult>("uninstall_codex_plugin", { pluginId });
}

export async function getCodexWebSearchMode(): Promise<CodexWebSearchSnapshot> {
  const invoke = await getInvoke();
  return invoke<CodexWebSearchSnapshot>("get_codex_web_search_mode", {});
}

export async function setCodexWebSearchMode(mode: CodexWebSearchMode): Promise<CodexWebSearchSnapshot> {
  const invoke = await getInvoke();
  return invoke<CodexWebSearchSnapshot>("set_codex_web_search_mode", { mode });
}

export async function getSkillRepository(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("get_skill_repository", {});
}

export async function getSkillRepositorySnapshot(): Promise<SkillRepositorySnapshot> {
  const invoke = await getInvoke();
  return invoke<SkillRepositorySnapshot>("get_skill_repository_snapshot", {});
}

export async function setSkillRepository(url: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("set_skill_repository", { url });
}

export async function listGithubRepositorySkills(url: string): Promise<RepositorySkill[]> {
  const invoke = await getInvoke();
  return invoke<RepositorySkill[]>("list_github_repository_skills", { url });
}

export async function refreshGithubRepositorySkills(url: string): Promise<SkillRepositorySnapshot> {
  const invoke = await getInvoke();
  return invoke<SkillRepositorySnapshot>("refresh_github_repository_skills", { url });
}

export async function installGithubRepositorySkills(url: string, paths: string[], target?: SkillTarget): Promise<Skill[]> {
  const invoke = await getInvoke();
  return invoke<Skill[]>("install_github_repository_skills", { url, paths, target });
}

export async function installGithubSkill(url: string, target?: SkillTarget): Promise<Skill> {
  const invoke = await getInvoke();
  return invoke<Skill>("install_github_skill", { url, target });
}

export async function checkSkillUpdate(name: string, target?: SkillTarget): Promise<SkillUpdateStatus> {
  const invoke = await getInvoke();
  return invoke<SkillUpdateStatus>("check_skill_update", { name, target });
}

export async function checkSkillUpdates(target?: SkillTarget): Promise<SkillUpdateStatus[]> {
  const invoke = await getInvoke();
  return invoke<SkillUpdateStatus[]>("check_skill_updates", { target });
}

export async function updateGithubSkills(names: string[], target?: SkillTarget): Promise<Skill[]> {
  const invoke = await getInvoke();
  return invoke<Skill[]>("update_github_skills", { names, target });
}

export async function installZipSkill(path: string, target?: SkillTarget): Promise<Skill> {
  const invoke = await getInvoke();
  return invoke<Skill>("install_zip_skill", { path, target });
}

export async function setSkillEnabled(name: string, enabled: boolean, target?: SkillTarget): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_skill_enabled", { name, enabled, target });
}

export async function deleteSkill(name: string, target?: SkillTarget): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_skill", { name, target });
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

export async function getLocalizationHubStatus(): Promise<LocalizationHubStatus> {
  const invoke = await getInvoke();
  return invoke<LocalizationHubStatus>("get_localization_hub_status", {});
}

export async function installClaudeCodeLocalization(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("install_claude_code_localization", {});
}

export async function installEditorLocalizationHelper(editor: "vscode" | "cursor"): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("install_editor_localization_helper", { editor });
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
  const invoke = await getInvoke();
  return invoke<CodexSessionSyncResult>("sync_codex_session_usage_cmd", {});
}

export async function rebuildCodexSessionUsage(): Promise<CodexSessionSyncResult> {
  const invoke = await getInvoke();
  return invoke<CodexSessionSyncResult>("rebuild_codex_session_usage_cmd", {});
}

export async function syncClaudeCodeSessionUsage(): Promise<CodexSessionSyncResult> {
  const invoke = await getInvoke();
  return invoke<CodexSessionSyncResult>("sync_claude_code_session_usage_cmd", {});
}

export async function rebuildClaudeCodeSessionUsage(): Promise<CodexSessionSyncResult> {
  const invoke = await getInvoke();
  return invoke<CodexSessionSyncResult>("rebuild_claude_code_session_usage_cmd", {});
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

export async function exportModelPricingXlsx(destinationPath: string): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("export_model_pricing_xlsx", { destinationPath });
}

export async function previewModelPricingXlsx(sourcePath: string): Promise<PricingImportPreview> {
  const invoke = await getInvoke();
  return invoke<PricingImportPreview>("preview_model_pricing_xlsx", { sourcePath });
}

export async function importModelPricingXlsx(sourcePath: string): Promise<PricingImportPreview> {
  const invoke = await getInvoke();
  return invoke<PricingImportPreview>("import_model_pricing_xlsx", { sourcePath });
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

export async function getCodexCliVersion(includeLatest = true): Promise<CodexCliVersionInfo> {
  const invoke = await getInvoke();
  return invoke<CodexCliVersionInfo>("get_codex_cli_version", { includeLatest });
}

export async function runCodexCliUpdate(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("run_codex_cli_update", {});
}

export async function getNodeRuntimeStatus(): Promise<NodeRuntimeStatus> {
  const invoke = await getInvoke();
  return invoke<NodeRuntimeStatus>("get_node_runtime_status", {});
}

export async function ensureNodeRuntimeViaFnm(): Promise<NodeRuntimeStatus> {
  const invoke = await getInvoke();
  return invoke<NodeRuntimeStatus>("ensure_node_runtime_via_fnm", {});
}

export async function scanSessions(
  provider?: SessionProvider,
  offset?: number,
  limit?: number,
): Promise<SessionScanResult> {
  const invoke = await getInvoke();
  return invoke<SessionScanResult>("scan_sessions", {
    provider,
    offset: offset ?? null,
    limit: limit ?? null,
  });
}

export async function syncCodexSessionProviders(
  targetProvider?: string,
): Promise<CodexProviderSyncResult> {
  const invoke = await getInvoke();
  return invoke<CodexProviderSyncResult>("sync_codex_session_providers", {
    targetProvider: targetProvider ?? null,
  });
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

export async function exportSession(provider: SessionProvider, sourcePath: string, destinationDir?: string): Promise<SessionArchiveInfo> {
  const invoke = await getInvoke();
  return invoke<SessionArchiveInfo>("export_session", { provider, sourcePath, destinationDir });
}

export async function backupSessions(provider: SessionProvider, sourcePaths: string[]): Promise<SessionBatchBackupInfo> {
  const invoke = await getInvoke();
  return invoke<SessionBatchBackupInfo>("backup_sessions", { provider, sourcePaths });
}

export async function exportSessions(provider: SessionProvider, sourcePaths: string[], destinationDir?: string): Promise<SessionBatchExportInfo> {
  const invoke = await getInvoke();
  return invoke<SessionBatchExportInfo>("export_sessions", { provider, sourcePaths, destinationDir });
}

export async function importSession(provider: SessionProvider, archivePath: string): Promise<SessionMeta> {
  const invoke = await getInvoke();
  return invoke<SessionMeta>("import_session", { provider, archivePath });
}

export async function trashSession(provider: SessionProvider, sourcePath: string): Promise<SessionArchiveInfo> {
  const invoke = await getInvoke();
  return invoke<SessionArchiveInfo>("trash_session", { provider, sourcePath });
}

export async function restoreTrashedSession(provider: SessionProvider, archivePath: string): Promise<SessionMeta> {
  const invoke = await getInvoke();
  return invoke<SessionMeta>("restore_trashed_session", { provider, archivePath });
}

export async function listTrashedSessions(provider: SessionProvider): Promise<SessionArchiveInfo[]> {
  const invoke = await getInvoke();
  return invoke<SessionArchiveInfo[]>("list_trashed_sessions", { provider });
}

export async function exportClaudeCodeSession(
  sourcePath: string,
  destinationDir?: string,
): Promise<SessionArchiveInfo> {
  const invoke = await getInvoke();
  return invoke<SessionArchiveInfo>("export_claude_code_session", { sourcePath, destinationDir });
}

export async function backupClaudeCodeSessions(sourcePaths: string[]): Promise<SessionBatchBackupInfo> {
  const invoke = await getInvoke();
  return invoke<SessionBatchBackupInfo>("backup_claude_code_sessions", { sourcePaths });
}

export async function exportClaudeCodeSessions(
  sourcePaths: string[],
  destinationDir?: string,
): Promise<SessionBatchExportInfo> {
  const invoke = await getInvoke();
  return invoke<SessionBatchExportInfo>("export_claude_code_sessions", { sourcePaths, destinationDir });
}

export async function importClaudeCodeSession(archivePath: string): Promise<SessionMeta> {
  const invoke = await getInvoke();
  return invoke<SessionMeta>("import_claude_code_session", { archivePath });
}

export async function trashClaudeCodeSession(sourcePath: string): Promise<SessionArchiveInfo> {
  const invoke = await getInvoke();
  return invoke<SessionArchiveInfo>("trash_claude_code_session", { sourcePath });
}

export async function restoreTrashedClaudeCodeSession(archivePath: string): Promise<SessionMeta> {
  const invoke = await getInvoke();
  return invoke<SessionMeta>("restore_trashed_claude_code_session", { archivePath });
}

export async function listTrashedClaudeCodeSessions(): Promise<SessionArchiveInfo[]> {
  const invoke = await getInvoke();
  return invoke<SessionArchiveInfo[]>("list_trashed_claude_code_sessions", {});
}

export async function setAppLanguage(language: "zh-CN" | "en-US"): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_app_language", { language });
}

export async function getUpdateMirrorSettings(): Promise<UpdateMirrorSettings> {
  const invoke = await getInvoke();
  return invoke<UpdateMirrorSettings>("get_update_mirror_settings", {});
}

export async function setUpdateMirrorSettings(settings: UpdateMirrorSettings): Promise<UpdateMirrorSettings> {
  const invoke = await getInvoke();
  return invoke<UpdateMirrorSettings>("set_update_mirror_settings", { settings });
}

export async function checkAppUpdate(): Promise<AppUpdateInfo | null> {
  const invoke = await getInvoke();
  return invoke<AppUpdateInfo | null>("check_app_update", {});
}

export async function installAppUpdate(version: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("install_app_update", { version });
}

export async function getDismissedOnboardingTips(): Promise<string[]> {
  const invoke = await getInvoke();
  return invoke<string[]>("get_dismissed_onboarding_tips", {});
}

export async function dismissOnboardingTip(tipKey: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("dismiss_onboarding_tip", { tipKey });
}

export async function restoreOnboardingTips(): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("restore_onboarding_tips", {});
}

export async function listProfiles(): Promise<Profile[]> {
  const invoke = await getInvoke();
  return invoke<Profile[]>("list_profiles", {});
}

export async function getCurrentProfileId(): Promise<string | null> {
  const invoke = await getInvoke();
  return invoke<string | null>("get_current_profile_id", {});
}

export async function createProfile(
  name: string,
  scopes: ProfileSnapshotScopes,
): Promise<Profile> {
  const invoke = await getInvoke();
  return invoke<Profile>("create_workspace_profile", { name, scopes });
}

export async function updateProfile(
  id: string,
  name?: string,
  payload?: ProfilePayload,
): Promise<Profile> {
  const invoke = await getInvoke();
  return invoke<Profile>("update_workspace_profile", { id, name, payload });
}

export async function deleteProfile(id: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_workspace_profile", { id });
}

export async function applyProfile(
  id: string,
  autosavePrevious = true,
): Promise<ApplyProfileResult> {
  const invoke = await getInvoke();
  return invoke<ApplyProfileResult>("apply_profile", { id, autosavePrevious });
}
