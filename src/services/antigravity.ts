import { call, getInvoke } from "./ipc";
import type {
  AntigravityAccountPublic,
  AntigravityCatalogModel,
  AntigravityDefaults,
  AntigravityGatewayStatus,
  Provider,
  ProviderTarget,
} from "@/types/backend";

export type {
  AntigravityAccountPublic,
  AntigravityCatalogModel,
  AntigravityDefaults,
  AntigravityGatewayStatus,
  AntigravityModelQuota,
  AntigravityQuotaBucket,
  AntigravityQuotaGroup,
  AntigravityQuotaSnapshot,
} from "@/types/backend";

export async function listAntigravityAccounts(): Promise<AntigravityAccountPublic[]> {
  return call<AntigravityAccountPublic[]>("list_antigravity_accounts");
}

export async function listAntigravityModels(): Promise<AntigravityCatalogModel[]> {
  return call<AntigravityCatalogModel[]>("list_antigravity_models");
}

export async function importAntigravityAccounts(json: string): Promise<number> {
  return call<number>("import_antigravity_accounts", { json });
}

export async function startAntigravityOauthLogin(): Promise<AntigravityAccountPublic> {
  return call<AntigravityAccountPublic>("start_antigravity_oauth_login");
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
  return call<AntigravityGatewayStatus>("get_antigravity_gateway_status");
}

export async function setAntigravityGatewayPort(port: number): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_antigravity_gateway_port", { port });
}

export async function setAntigravityGatewayApiKey(apiKey: string): Promise<void> {
  const invoke = await getInvoke();
  await invoke("set_antigravity_gateway_api_key", { apiKey });
}

export async function setAntigravityOutboundProxy(
  mode: "direct" | "system" | "custom",
  proxyUrl?: string,
): Promise<AntigravityGatewayStatus> {
  return call<AntigravityGatewayStatus>("set_antigravity_outbound_proxy", {
    mode,
    proxyUrl: proxyUrl ?? null,
  });
}

export async function startAntigravityGateway(
  port?: number,
): Promise<AntigravityGatewayStatus> {
  return call<AntigravityGatewayStatus>("start_antigravity_gateway", {
    port: port ?? null,
  });
}

export async function stopAntigravityGateway(): Promise<AntigravityGatewayStatus> {
  return call<AntigravityGatewayStatus>("stop_antigravity_gateway");
}

export async function refreshAntigravityAccountQuota(
  accountId: string,
): Promise<AntigravityAccountPublic> {
  return call<AntigravityAccountPublic>("refresh_antigravity_account_quota", {
    accountId,
  });
}

export async function refreshAntigravityQuotas(): Promise<AntigravityAccountPublic[]> {
  return call<AntigravityAccountPublic[]>("refresh_antigravity_quotas");
}

export async function ensureAntigravityProvider(
  target: ProviderTarget,
  model?: string,
): Promise<Provider> {
  return call<Provider>("ensure_antigravity_provider", {
    target,
    model: model ?? null,
  });
}

export async function getAntigravityDefaults(): Promise<AntigravityDefaults> {
  return call<AntigravityDefaults>("get_antigravity_defaults");
}
