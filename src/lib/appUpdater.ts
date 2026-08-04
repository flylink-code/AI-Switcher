import { checkAppUpdate, installAppUpdate } from "@/services/api";
import type { AppUpdateInfo } from "@/types/backend";

export type AppUpdate = AppUpdateInfo;

export async function checkForAppUpdate(timeoutMessage: string): Promise<AppUpdate | null> {
  const attempts = 2;

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await withTimeout(checkAppUpdate(), 60_000, timeoutMessage);
    } catch (error) {
      if (attempt === attempts) throw error;
      console.warn("Application update check failed; retrying once", error);
    }
  }

  throw new Error(timeoutMessage);
}

export async function installAvailableAppUpdate(version: string): Promise<void> {
  // On Windows, tauri-plugin-updater launches NSIS with /R then hard-exits; this
  // await typically never resolves. Callers should still attempt restartApp() for
  // macOS/Linux where install returns and the app must relaunch itself.
  await installAppUpdate(version);
}

/** Backend maps missing manifests / platform packages to null; keep a frontend safety net. */
export function isNoAppUpdateAvailableError(raw: string): boolean {
  const text = raw.toLowerCase();
  return (
    text.includes("could not fetch a valid release json")
    || text.includes("were found in the response `platforms`")
    || text.includes("was not found in the response `platforms` object")
    || text.includes("当前没有可安装的应用更新")
  );
}

export function isAppUpdatePackagePendingError(raw: string): boolean {
  return raw.includes("暂时无法获取安装包") || raw.toLowerCase().includes("package pending");
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(message)), timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer !== undefined) clearTimeout(timer);
  });
}
