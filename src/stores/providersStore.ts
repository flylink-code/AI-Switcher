import { listen } from "@tauri-apps/api/event";
import type { Provider, ProviderHealthUpdated } from "@/types/backend";
import { providerListOptions } from "@/lib/appQueries";
import { queryClient } from "@/lib/queryClient";

let healthEventsInitialized = false;

export function initializeProviderHealthEvents(): void {
  if (healthEventsInitialized) return;
  healthEventsInitialized = true;
  void listen<ProviderHealthUpdated>("provider-health-updated", ({ payload }) => {
    queryClient.setQueryData<Provider[]>(
      providerListOptions(payload.targetApp).queryKey,
      (current = []) =>
        current.map((provider) =>
          provider.id === payload.providerId
            ? {
                ...provider,
                healthStatus: payload.ok ? "healthy" : "error",
                healthCheckedAt: payload.checkedAt,
                healthLatencyMs: payload.latencyMs ?? null,
              }
            : provider,
        ),
    );
  });
}
