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
  GatewayCatalogModelOption,
  AgentConnectionView,
  ConnectionType,
  GatewayProfile,
  GatewayProfilePatch,
  GatewayRouteLog,
  GatewayUpstreamImportResult,
  GatewayUpstreamModelRow,
  GatewayUpstreamDiscoverItem,
  SmartGatewayStatus,
  GatewayBinding,
  RouteMode,
  RouteModePatch,
  RouteRule,
  RouteModeUsageStat,
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

export async function getAgentConnection(target: ProviderTarget): Promise<AgentConnectionView> {
  return call<AgentConnectionView>("get_agent_connection", { target });
}

export async function setAgentConnection(
  target: ProviderTarget,
  connectionType: ConnectionType,
): Promise<AgentConnectionView> {
  return call<AgentConnectionView>("set_agent_connection", { target, connectionType });
}

export async function getGatewayProfile(target: ProviderTarget): Promise<GatewayProfile | null> {
  return call<GatewayProfile | null>("get_gateway_profile", { target });
}

export async function updateGatewayProfile(
  target: ProviderTarget,
  patch: GatewayProfilePatch,
): Promise<GatewayProfile> {
  return call<GatewayProfile>("update_gateway_profile", { target, patch });
}

export async function listGatewayRouteLogs(
  target?: ProviderTarget | null,
  limit = 20,
): Promise<GatewayRouteLog[]> {
  return call<GatewayRouteLog[]>("list_gateway_route_logs", { target: target ?? null, limit });
}

export async function listGatewayUpstreams(): Promise<Provider[]> {
  return call<Provider[]>("list_gateway_upstreams");
}

export async function upsertGatewayUpstream(input: ProviderInput): Promise<Provider> {
  return call<Provider>("upsert_gateway_upstream", { input });
}

export async function deleteGatewayUpstream(id: string): Promise<void> {
  return call<void>("delete_gateway_upstream", { id });
}

export async function addAntigravityGatewayUpstream(): Promise<Provider> {
  return call<Provider>("add_antigravity_gateway_upstream");
}

export async function importGatewayUpstreamsFromProviders(
  sourceTarget: ProviderTarget,
  providerIds: string[],
  addToAllowlistTarget?: ProviderTarget | null,
): Promise<GatewayUpstreamImportResult> {
  return call<GatewayUpstreamImportResult>("import_gateway_upstreams_from_providers", {
    sourceTarget,
    providerIds,
    addToAllowlistTarget: addToAllowlistTarget ?? null,
  });
}

export async function listGatewayUpstreamModels(id: string): Promise<GatewayUpstreamModelRow[]> {
  return call<GatewayUpstreamModelRow[]>("list_gateway_upstream_models", { id });
}

export async function setGatewayUpstreamModelVisible(
  id: string,
  modelId: string,
  visible: boolean,
): Promise<GatewayUpstreamModelRow[]> {
  return call<GatewayUpstreamModelRow[]>("set_gateway_upstream_model_visible", {
    id,
    modelId,
    visible,
  });
}

export async function discoverGatewayUpstreamModels(id: string): Promise<ModelDiscoveryResult> {
  return call<ModelDiscoveryResult>("discover_gateway_upstream_models", { id });
}

export async function discoverGatewayUpstreamModelsBatch(
  ids: string[],
): Promise<GatewayUpstreamDiscoverItem[]> {
  return call<GatewayUpstreamDiscoverItem[]>("discover_gateway_upstream_models_batch", { ids });
}

export async function ensureSmartGatewayProvider(target: ProviderTarget): Promise<Provider> {
  return call<Provider>("ensure_smart_gateway_provider", { target });
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

export async function listGatewayCatalogEntries(
  target: ProviderTarget,
): Promise<GatewayCatalogModelOption[]> {
  return call<GatewayCatalogModelOption[]>("list_gateway_catalog_entries", { target });
}

export async function getGatewayCatalogOpusplan(target: ProviderTarget): Promise<boolean> {
  return call<boolean>("get_gateway_catalog_opusplan", { target });
}

export async function setGatewayCatalogOpusplan(
  target: ProviderTarget,
  enabled: boolean,
): Promise<boolean> {
  return call<boolean>("set_gateway_catalog_opusplan", { target, enabled });
}

export async function getGatewayCatalogPlan(target: ProviderTarget): Promise<string> {
  return call<string>("get_gateway_catalog_plan", { target });
}

export async function setGatewayCatalogPlan(
  target: ProviderTarget,
  model: string,
): Promise<string> {
  return call<string>("set_gateway_catalog_plan", { target, model });
}

export async function getGatewayCatalogExecute(target: ProviderTarget): Promise<string> {
  return call<string>("get_gateway_catalog_execute", { target });
}

export async function setGatewayCatalogExecute(
  target: ProviderTarget,
  model: string,
): Promise<string> {
  return call<string>("set_gateway_catalog_execute", { target, model });
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

export async function getSmartGatewayStatus(): Promise<SmartGatewayStatus> {
  return call("get_smart_gateway_status");
}

export async function setSmartGatewayPort(port: number): Promise<void> {
  await call("set_smart_gateway_port", { port });
}

export async function setSmartGatewayApiKey(apiKey: string): Promise<SmartGatewayStatus> {
  return call("set_smart_gateway_api_key", { apiKey });
}

export async function rotateSmartGatewayApiKey(): Promise<SmartGatewayStatus> {
  return call("rotate_smart_gateway_api_key");
}

export async function startSmartGateway(port?: number): Promise<SmartGatewayStatus> {
  return call("start_smart_gateway", { port: port ?? null });
}

export async function stopSmartGateway(): Promise<SmartGatewayStatus> {
  return call("stop_smart_gateway");
}

export async function listSmartGatewayBindings(): Promise<GatewayBinding[]> {
  return call("list_smart_gateway_bindings");
}

export async function bindSmartGateway(target: ProviderTarget): Promise<Provider> {
  return call("bind_smart_gateway", { target });
}

export async function unbindSmartGateway(target: ProviderTarget): Promise<void> {
  await call("unbind_smart_gateway", { target });
}

export async function listRouteModes(): Promise<RouteMode[]> {
  return call("list_route_modes");
}

export async function updateRouteMode(id: string, patch: RouteModePatch): Promise<RouteMode> {
  return call("update_route_mode", { id, patch });
}

export async function listRouteRules(): Promise<RouteRule[]> {
  return call("list_route_rules");
}

export async function upsertRouteRule(rule: RouteRule): Promise<RouteRule> {
  return call("upsert_route_rule", { rule });
}

export async function deleteRouteRule(id: string): Promise<void> {
  await call("delete_route_rule", { id });
}

export async function listRouteModeUsageStats(): Promise<RouteModeUsageStat[]> {
  return call("list_route_mode_usage_stats");
}
