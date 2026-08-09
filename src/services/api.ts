/**
 * Thin wrapper over the Tauri IPC bridge.
 *
 * In a browser (no Tauri runtime), calls reject with a clear error so the UI
 * degrades gracefully rather than throwing on a missing global. This makes the
 * frontend buildable/runnable via `pnpm dev` for quick iteration even outside
 * the desktop shell.
 */

export * from "./system";
export * from "./config";
export * from "./providers";
export * from "./antigravity";
export * from "./proxy";
export * from "./mcp";
export * from "./prompts";
export * from "./skills";
export * from "./usage";
export * from "./tools";
export * from "./sessions";
