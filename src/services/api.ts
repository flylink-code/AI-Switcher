/**
 * Thin wrapper over the Tauri IPC bridge.
 *
 * In a browser (no Tauri runtime), calls reject with a clear error so the UI
 * degrades gracefully rather than throwing on a missing global. This makes the
 * frontend buildable/runnable via `pnpm dev` for quick iteration even outside
 * the desktop shell.
 */
import type { PathsInfo, DbInfo, Provider, ProviderInput, PresetInfo, ProxyStatus } from "@/types/backend";

let invokeImpl: typeof import("@tauri-apps/api/core").invoke | null = null;

async function getInvoke() {
  if (invokeImpl) return invokeImpl;
  // Detect the Tauri runtime. The internal global is present only inside the app.
  const hasTauri =
    typeof window !== "undefined" &&
    // @tauri-apps/api checks for "__TAURI_INTERNALS__" (v2).
    // Using a defensive access to avoid a hard reference.
    Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
  if (!hasTauri) {
    throw new Error("Tauri runtime not available (running in a plain browser).");
  }
  const mod = await import("@tauri-apps/api/core");
  invokeImpl = mod.invoke;
  return invokeImpl;
}

export async function ping(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("ping", {});
}

export async function getPaths(): Promise<PathsInfo> {
  const invoke = await getInvoke();
  return invoke<PathsInfo>("get_paths", {});
}

export async function getDbInfo(): Promise<DbInfo> {
  const invoke = await getInvoke();
  return invoke<DbInfo>("get_db_info", {});
}

export async function backupNow(): Promise<string> {
  const invoke = await getInvoke();
  return invoke<string>("backup_now", {});
}

// ---- Providers -------------------------------------------------------------

export async function listProviders(): Promise<Provider[]> {
  const invoke = await getInvoke();
  return invoke<Provider[]>("list_providers", {});
}

export async function getCurrentProvider(): Promise<Provider | null> {
  const invoke = await getInvoke();
  return invoke<Provider | null>("get_current_provider", {});
}

export async function createProvider(input: ProviderInput): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("create_provider", { input });
}

export async function updateProvider(input: ProviderInput): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("update_provider", { input });
}

export async function deleteProvider(id: string): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("delete_provider", { id });
}

export async function switchProvider(id: string): Promise<Provider> {
  const invoke = await getInvoke();
  return invoke<Provider>("switch_provider", { id });
}

export async function switchToOfficial(): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("switch_to_official", {});
}

export async function reorderProviders(orderedIds: string[]): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("reorder_providers", { orderedIds });
}

export async function importLiveConfig(): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("import_live_config", {});
}

export async function listPresets(): Promise<PresetInfo[]> {
  const invoke = await getInvoke();
  return invoke<PresetInfo[]>("list_presets", {});
}

// ---- Local proxy ------------------------------------------------------------

export async function getProxyStatus(): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("get_proxy_status", {});
}

export async function startProxy(port?: number): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("start_proxy", { port });
}

export async function stopProxy(): Promise<ProxyStatus> {
  const invoke = await getInvoke();
  return invoke<ProxyStatus>("stop_proxy", {});
}

export async function setProxyPort(port: number): Promise<void> {
  const invoke = await getInvoke();
  return invoke<void>("set_proxy_port", { port });
}
