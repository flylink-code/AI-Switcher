import { check } from "@tauri-apps/plugin-updater";

export type AppUpdate = NonNullable<Awaited<ReturnType<typeof check>>>;

export async function checkForAppUpdate(timeoutMessage: string): Promise<AppUpdate | null> {
  const attempts = 2;

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      return await withTimeout(check(), 60_000, timeoutMessage);
    } catch (error) {
      if (attempt === attempts) throw error;
      console.warn("Application update check failed; retrying once", error);
    }
  }

  throw new Error(timeoutMessage);
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
