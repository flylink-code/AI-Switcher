import { listen } from "@tauri-apps/api/event";
import { queryClient } from "@/lib/queryClient";
import type { ProxyStatusUpdated } from "@/types/backend";

let initialized = false;

export function initializeProxyStatusEvents(): void {
  if (initialized) return;
  initialized = true;
  void listen<ProxyStatusUpdated>("proxy-status-updated", ({ payload }) => {
    queryClient.setQueryData(["proxy-status", payload.target], payload.status);
  });
}
