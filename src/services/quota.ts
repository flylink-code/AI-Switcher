import { call } from "./ipc";
import type { ProviderQuotaResult, ProviderTarget } from "@/types/backend";

export async function getProviderQuota(providerId: string): Promise<ProviderQuotaResult> {
  return call<ProviderQuotaResult>("get_provider_quota", { providerId });
}

export async function getOfficialQuota(target: ProviderTarget): Promise<ProviderQuotaResult> {
  return call<ProviderQuotaResult>("get_official_quota", { target });
}
