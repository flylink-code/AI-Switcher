import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getProviderQuota, getOfficialQuota } from "@/services/api";
import type { ProviderQuotaResult, ProviderTarget } from "@/types/backend";

export function useProviderQuota(providerId: string, enabled = true) {
  return useQuery<ProviderQuotaResult>({
    queryKey: ["provider-quota", providerId],
    queryFn: () => getProviderQuota(providerId),
    enabled: Boolean(providerId) && enabled,
    staleTime: 60_000,
    gcTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    retry: 1,
  });
}

export function useOfficialQuota(target: ProviderTarget, enabled = true) {
  const supportsOfficial = target === "claude_code" || target === "claude_desktop" || target === "codex";
  return useQuery<ProviderQuotaResult>({
    queryKey: ["official-quota", target],
    queryFn: () => getOfficialQuota(target),
    enabled: supportsOfficial && enabled,
    staleTime: 60_000,
    gcTime: 5 * 60_000,
    refetchOnWindowFocus: false,
    retry: 1,
  });
}

export function useInvalidateQuota() {
  const queryClient = useQueryClient();
  return {
    invalidateProviderQuota: (providerId: string) =>
      queryClient.invalidateQueries({ queryKey: ["provider-quota", providerId] }),
    invalidateOfficialQuota: (target: ProviderTarget) =>
      queryClient.invalidateQueries({ queryKey: ["official-quota", target] }),
    invalidateAllQuotas: () => {
      queryClient.invalidateQueries({ queryKey: ["provider-quota"] });
      queryClient.invalidateQueries({ queryKey: ["official-quota"] });
    },
  };
}
