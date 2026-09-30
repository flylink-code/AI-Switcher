import { call, getInvoke } from "./ipc";
import type { Provider, ProviderTarget } from "@/types/backend";

export interface KiroQuotaSnapshot {
  plan: string;
  used: number;
  limit: number;
  resetAt: string;
  overage: string;
  trialUsed: number | null;
  trialLimit: number | null;
  queriedAt: number;
  error: string;
}

export interface KiroAccountPublic {
  id: string;
  label: string;
  authMethod: string;
  provider: string;
  region: string;
  disabled: boolean;
  disableReason: string;
  hasRefreshToken: boolean;
  active: boolean;
  quota: KiroQuotaSnapshot | null;
}

export interface KiroExitProxy {
  id: string;
  name: string;
  enabled: boolean;
  proxyUrl: string;
  proxyRedacted: string;
  probeOk: boolean | null;
  probeIp: string | null;
  probeHop: string | null;
  probeMessage: string | null;
  latencyOk: boolean | null;
  latencyMs: number | null;
  latencyMessage: string | null;
}

export interface KiroExitProxyInput {
  id: string;
  name: string;
  enabled: boolean;
  proxyUrl: string;
}

export interface KiroExitProbeResult {
  id: string;
  ok: boolean;
  ip: string | null;
  hop: string | null;
  message: string;
}

export interface KiroExitLatencyResult {
  id: string;
  ok: boolean;
  millis: number | null;
  hop: string | null;
  message: string;
}

export interface KiroAccountTestResult {
  ok: boolean;
  category: KiroAccountTestCategory;
  status: number | null;
  model: string;
  latencyMs: number;
  reply: string | null;
  error: string | null;
}

export type KiroAccountTestCategory = "ok" | "rate_limit" | "network" | "auth" | "quota" | "error";

export interface KiroGatewayStatus {
  running: boolean;
  port: number;
  apiKey: string;
  accountCount: number;
  baseUrl: string;
  outboundMode: string;
  outboundProxyUrl: string;
  effectiveOutboundProxy: string | null;
  exitProxies: KiroExitProxy[];
  exitChainLabel: string;
  exitError: string | null;
}

export type KiroOutboundMode = "direct" | "system" | "custom";

export async function listKiroAccounts(): Promise<KiroAccountPublic[]> {
  return call<KiroAccountPublic[]>("list_kiro_accounts");
}

export async function importKiroAccounts(raw: string): Promise<number> {
  return call<number>("import_kiro_accounts", { raw });
}

export async function removeKiroAccount(id: string): Promise<void> {
  const invoke = await getInvoke();
  await invoke("remove_kiro_account", { id });
}

export async function startKiroBuilderIdLogin(): Promise<KiroAccountPublic> {
  return call<KiroAccountPublic>("start_kiro_builder_id_login");
}

export async function startKiroSocialLogin(): Promise<KiroAccountPublic> {
  return call<KiroAccountPublic>("start_kiro_social_login");
}

export async function getKiroGatewayStatus(): Promise<KiroGatewayStatus> {
  return call<KiroGatewayStatus>("get_kiro_gateway_status");
}

export async function setKiroGatewayPort(port: number): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_kiro_gateway_port", { port });
}

export async function setKiroGatewayApiKey(apiKey: string): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_kiro_gateway_api_key", { apiKey });
}

export async function setKiroOutboundProxy(
  mode: KiroOutboundMode,
  proxyUrl?: string,
): Promise<KiroGatewayStatus> {
  return call<KiroGatewayStatus>("set_kiro_outbound_proxy", {
    mode,
    proxyUrl: proxyUrl ?? "",
  });
}

export async function startKiroGateway(port?: number): Promise<KiroGatewayStatus> {
  return call<KiroGatewayStatus>("start_kiro_gateway", { port: port ?? null });
}

export async function stopKiroGateway(): Promise<KiroGatewayStatus> {
  return call<KiroGatewayStatus>("stop_kiro_gateway");
}

export async function ensureKiroProvider(target: ProviderTarget): Promise<Provider> {
  return call<Provider>("ensure_kiro_provider", { target, model: null });
}

export async function refreshKiroAccountQuota(id: string): Promise<KiroAccountPublic> {
  return call<KiroAccountPublic>("refresh_kiro_account_quota", { id });
}

export async function refreshKiroQuotas(): Promise<KiroAccountPublic[]> {
  return call<KiroAccountPublic[]>("refresh_kiro_quotas");
}

export async function testKiroAccount(
  id: string,
  model?: string,
  prompt?: string,
): Promise<KiroAccountTestResult> {
  return call<KiroAccountTestResult>("test_kiro_account", {
    id,
    model: model ?? null,
    prompt: prompt ?? null,
  });
}

export async function setKiroExitProxy(entries: KiroExitProxyInput[]): Promise<KiroGatewayStatus> {
  return call<KiroGatewayStatus>("set_kiro_exit_proxy", { entries });
}

export async function probeKiroExitProxy(id: string, proxyUrl: string): Promise<KiroExitProbeResult> {
  return call<KiroExitProbeResult>("probe_kiro_exit_proxy", { id, proxyUrl });
}

export async function probeKiroExitLatency(
  id: string,
  proxyUrl: string,
): Promise<KiroExitLatencyResult> {
  return call<KiroExitLatencyResult>("probe_kiro_exit_latency", { id, proxyUrl });
}
