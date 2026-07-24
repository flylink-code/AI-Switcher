/**
 * Thin wrapper over the Tauri IPC bridge.
 *
 * In a browser (no Tauri runtime), calls reject with a clear error so the UI
 * degrades gracefully rather than throwing on a missing global. This makes the
 * frontend buildable/runnable via `pnpm dev` for quick iteration even outside
 * the desktop shell.
 */
import type { PathsInfo, DbInfo } from "@/types/backend";

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
