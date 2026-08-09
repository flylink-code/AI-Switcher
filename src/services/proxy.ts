import { call, getInvoke } from "./ipc";
import { reportFrontendPerformance } from "./system";
import type {
  ManagedAppRuntimeStatus,
  ProviderTarget,
  ProxyStatus,
} from "@/types/backend";

export async function getProxyStatus(target?: ProviderTarget): Promise<ProxyStatus> {
  const startedAt = performance.now();
  const invoke = await getInvoke();
  try {
    return await invoke<ProxyStatus>("get_proxy_status", { target });
  } finally {
    void reportFrontendPerformance(
      "proxy_status_ipc",
      target ?? "claude_desktop",
      Math.round(performance.now() - startedAt),
    ).catch(() => undefined);
  }
}

export async function getManagedAppsRuntimeStatus(): Promise<ManagedAppRuntimeStatus> {
  return call<ManagedAppRuntimeStatus>("get_managed_apps_runtime_status");
}

export async function startProxy(port?: number, target?: ProviderTarget): Promise<ProxyStatus> {
  return call<ProxyStatus>("start_proxy", { port, target });
}

export async function stopProxy(target?: ProviderTarget): Promise<ProxyStatus> {
  return call<ProxyStatus>("stop_proxy", { target });
}

export async function setProxyPort(port: number, target?: ProviderTarget): Promise<void> {
  return call<void>("set_proxy_port", { port, target });
}

export async function getProxyFailoverEnabled(): Promise<boolean> {
  return call<boolean>("get_proxy_failover_enabled", {});
}

export async function setProxyFailoverEnabled(enabled: boolean): Promise<void> {
  return call<void>("set_proxy_failover_enabled", { enabled });
}

export async function getProxyRetryableStatusCodes(): Promise<string> {
  return call<string>("get_proxy_retryable_status_codes", {});
}

export async function setProxyRetryableStatusCodes(value: string): Promise<void> {
  return call<void>("set_proxy_retryable_status_codes", { value });
}

export async function getProxyStreamingIdleTimeoutSecs(): Promise<number> {
  return call<number>("get_proxy_streaming_idle_timeout_secs", {});
}

export async function setProxyStreamingIdleTimeoutSecs(secs: number): Promise<void> {
  return call<void>("set_proxy_streaming_idle_timeout_secs", { secs });
}

// ---- MCP --------------------------------------------------------------------
