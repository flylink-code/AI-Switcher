/**
 * Thin wrapper over the Tauri IPC bridge.
 *
 * In a browser (no Tauri runtime), calls reject with a clear error so the UI
 * degrades gracefully rather than throwing on a missing global. This makes the
 * frontend buildable/runnable via `pnpm dev` for quick iteration even outside
 * the desktop shell.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";

async function getInvoke() {
  // Detect the Tauri runtime. The internal global is present only inside the app.
  const hasTauri =
    typeof window !== "undefined" &&
    // @tauri-apps/api checks for "__TAURI_INTERNALS__" (v2).
    // Using a defensive access to avoid a hard reference.
    Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
  if (!hasTauri) {
    throw new Error("Tauri runtime not available (running in a plain browser).");
  }
  return tauriInvoke;
}

export { getInvoke };

/** Shared IPC call helper — replaces the per-function `getInvoke()` boilerplate. */
export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const invoke = await getInvoke();
  return invoke<T>(cmd, args ?? {});
}
