/**
 * Thin wrapper over the Tauri IPC bridge.
 *
 * In a browser (no Tauri runtime), calls reject with a clear error so the UI
 * degrades gracefully rather than throwing on a missing global. This makes the
 * frontend buildable/runnable via `pnpm dev` for quick iteration even outside
 * the desktop shell.
 */
import type {
  DbInfo,
  LivePrompt,
  McpImportSummary,
  McpServer,
  McpServerInput,
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
  UsageDashboard,
  ModelPricing,
  ModelPricingInput,
} from "@/types/backend";

let invokeImpl: typeof import("@tauri-apps/api/core").invoke | null = null;

async function getInvoke() {
  if (invokeImpl) return invokeImpl;
  // Detect the Tauri runtime. The internal global is present only inside the app.
  const hasTauri =
    typeof window !== "undefined" &&
    // @tauri-apps/api checks for "__TAURI_INTERNALS__" (v2).
    // Using a defensive access to avoid a hard reference.
    Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
  if (!hasTauri) {
    throw new Error("Tauri runtime not available (running in a plain browser).");
  }
  const mod = await import("@tauri-apps/api/core");
  invokeImpl = mod.invoke;
  return invokeImpl;
}

export async function ping(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("ping", {});
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

export async function discoverProviderModels(id: string): Promise<ModelDiscoveryResult> {
  const invoke = await getInvoke();
  return invoke<ModelDiscoveryResult>("discover_provider_models", { id });
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

export async function getProxyStatus(): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("get_proxy_status", {});
}

export async function startProxy(port?: number): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("start_proxy", { port });
}

export async function stopProxy(): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("stop_proxy", {});
}

export async function setProxyPort(port: number): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_proxy_port", { port });
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
