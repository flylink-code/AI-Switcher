import { listen } from "@tauri-apps/api/event";
import { queryClient } from "@/lib/queryClient";
import { getProxyStatus } from "@/services/api";
import { getSmartGatewayStatus } from "@/services/providers";
import type { ProviderTarget, ProxyStatusUpdated, SmartGatewayStatus } from "@/types/backend";

let initialized = false;

export function initializeProxyStatusEvents(): void {
  if (initialized) return;
  initialized = true;
  void listen<ProxyStatusUpdated>(
    "proxy-status-updated",
    ({ payload }) => {
      queryClient.setQueryData(["proxy-status", payload.target], payload.status);
    },
  )
    .then(() => synchronizeProxySnapshots())
    .catch(() => undefined);
  void listen<SmartGatewayStatus>(
    "smart-gateway-status-updated",
    ({ payload }) => {
      queryClient.setQueryData(["smart-gateway-status"], payload);
    },
  )
    .then(() => getSmartGatewayStatus().then((status) => {
      queryClient.setQueryData(["smart-gateway-status"], status);
    }))
    .catch(() => undefined);
}

async function synchronizeProxySnapshots(): Promise<void> {
  for (const target of [
    "claude_code",
    "claude_desktop",
    "codex",
  ] satisfies ProviderTarget[]) {
    try {
      const status = await getProxyStatus(target);
      queryClient.setQueryData(["proxy-status", target], status);
    } catch {
      // Startup warmup/manual refresh remains available if this best-effort sync fails.
    }
  }
}
