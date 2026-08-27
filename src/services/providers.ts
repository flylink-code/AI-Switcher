import { call } from "./ipc";
import type {
  CodexAuthStatus,
  CodexOauthAccount,
  CodexOauthDeviceStart,
  CodexOauthPollResult,
  ConnectionTestResult,
  DeeplinkImportResult,
  EndpointSpeedtestResult,
  ImportPreview,
  ModelDiscoveryResult,
  Provider,
  ProviderDoctorReport,
  ProviderImportResult,
  ProviderInput,
  ProviderTarget,
  SwitchProviderResult,
} from "@/types/backend";

export async function listProviders(target: ProviderTarget): Promise<Provider[]> {
  return call<Provider[]>("list_providers", { target });
}

export async function getCodexAuthStatus(): Promise<CodexAuthStatus> {
  return call<CodexAuthStatus>("get_codex_auth_status");
}

export async function startCodexOauthLogin(): Promise<CodexOauthDeviceStart> {
  return call<CodexOauthDeviceStart>("start_codex_oauth_login", {});
}

export async function pollCodexOauthLogin(deviceCode: string): Promise<CodexOauthPollResult> {
  return call<CodexOauthPollResult>("poll_codex_oauth_login", { deviceCode });
}

export async function listCodexOauthAccounts(): Promise<CodexOauthAccount[]> {
  return call<CodexOauthAccount[]>("list_codex_oauth_accounts", {});
}

export async function ensureCodexOauthProvider(
  target: ProviderTarget,
  accountId: string,
  model?: string,
): Promise<Provider> {
  return call<Provider>("ensure_codex_oauth_provider", {
    target,
    accountId,
    model: model ?? null,
  });
}

export async function getCurrentProvider(target: ProviderTarget): Promise<Provider | null> {
  return call<Provider | null>("get_current_provider", { target });
}

export async function getGatewayCatalogEnabled(target: ProviderTarget): Promise<boolean> {
  return call<boolean>("get_gateway_catalog_enabled", { target });
}

export async function setGatewayCatalogEnabled(
  target: ProviderTarget,
  enabled: boolean,
): Promise<boolean> {
  return call<boolean>("set_gateway_catalog_enabled", { target, enabled });
}

export async function getGatewayCatalogSubagent(target: ProviderTarget): Promise<string> {
  return call<string>("get_gateway_catalog_subagent", { target });
}

export async function setGatewayCatalogSubagent(
  target: ProviderTarget,
  model: string,
): Promise<string> {
  return call<string>("set_gateway_catalog_subagent", { target, model });
}

export async function getGatewayCatalogHideOfficial(target: ProviderTarget): Promise<boolean> {
  return call<boolean>("get_gateway_catalog_hide_official", { target });
}

export async function setGatewayCatalogHideOfficial(
  target: ProviderTarget,
  enabled: boolean,
): Promise<boolean> {
  return call<boolean>("set_gateway_catalog_hide_official", { target, enabled });
}

export async function listGatewayCatalogModels(target: ProviderTarget): Promise<string[]> {
  return call<string[]>("list_gateway_catalog_models", { target });
}

export async function getClaudeCodeDefaultPermissionMode(): Promise<string> {
  return call<string>("get_claude_code_default_permission_mode");
}

export async function setClaudeCodeDefaultPermissionMode(mode: string): Promise<string> {
  return call<string>("set_claude_code_default_permission_mode", { mode });
}

export async function createProvider(input: ProviderInput): Promise<Provider> {
  return call<Provider>("create_provider", { input });
}

export async function copyProviderToTarget(
  id: string,
  target: ProviderTarget,
): Promise<Provider> {
  return call<Provider>("copy_provider_to_target", { id, target });
}

export async function updateProvider(input: ProviderInput): Promise<Provider> {
  return call<Provider>("update_provider", { input });
}

export async function deleteProvider(id: string): Promise<void> {
  return call<void>("delete_provider", { id });
}

export async function switchProvider(id: string): Promise<SwitchProviderResult> {
  return call<SwitchProviderResult>("switch_provider", { id });
}

export async function switchToOfficial(target: ProviderTarget): Promise<void> {
  return call<void>("switch_to_official", { target });
}

export async function reorderProviders(orderedIds: string[], target: ProviderTarget): Promise<void> {
  return call<void>("reorder_providers", { orderedIds, target });
}

export async function importLiveConfig(target: ProviderTarget): Promise<void> {
  return call<void>("import_live_config", { target });
}

export async function testProviderConnection(id: string): Promise<ConnectionTestResult> {
  return call<ConnectionTestResult>("test_provider_connection", { id });
}

export async function speedtestProviderEndpoint(id: string): Promise<EndpointSpeedtestResult> {
  return call<EndpointSpeedtestResult>("speedtest_provider_endpoint", { id });
}

export async function testProviderInput(input: ProviderInput): Promise<ConnectionTestResult> {
  return call<ConnectionTestResult>("test_provider_input", { input });
}

export async function batchDiagnoseProviders(
  target?: ProviderTarget | null,
): Promise<ProviderDoctorReport[]> {
  return call<ProviderDoctorReport[]>("batch_diagnose_providers", { target: target ?? null });
}

export async function quarantineFailedProviders(providerIds: string[]): Promise<number> {
  return call<number>("quarantine_failed_providers", { providerIds });
}


export async function discoverProviderModels(id: string): Promise<ModelDiscoveryResult> {
  return call<ModelDiscoveryResult>("discover_provider_models", { id });
}

export async function getCachedProviderModels(id: string): Promise<ModelDiscoveryResult> {
  return call<ModelDiscoveryResult>("get_cached_provider_models", { id });
}

export async function discoverProviderModelsInput(input: ProviderInput): Promise<ModelDiscoveryResult> {
  return call<ModelDiscoveryResult>("discover_provider_models_input", { input });
}

export async function exportProviders(target: ProviderTarget): Promise<string> {
  return call<string>("export_providers", { target });
}

export async function importProvidersJson(json: string): Promise<ProviderImportResult> {
  return call<ProviderImportResult>("import_providers_json", { json });
}

export async function previewImportText(text: string): Promise<ImportPreview> {
  return call<ImportPreview>("preview_import_text", { text });
}

export async function confirmImportPreview(preview: ImportPreview): Promise<DeeplinkImportResult> {
  return call<DeeplinkImportResult>("confirm_import_preview", { preview });
}

export async function buildProviderDeeplink(providerId: string): Promise<string> {
  return call<string>("build_provider_deeplink", { providerId });
}
