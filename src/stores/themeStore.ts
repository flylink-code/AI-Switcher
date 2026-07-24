import { create } from "zustand";

export type ThemeMode = "light" | "dark" | "system";

const STORAGE_KEY = "cs.theme";

/** Resolve the OS color-scheme preference. */
function systemPrefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

function readInitial(): ThemeMode {
  if (typeof localStorage === "undefined") return "system";
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "light" || stored === "dark" || stored === "system") return stored;
  return "system";
}

interface ThemeState {
  mode: ThemeMode;
  /** Effective mode after resolving "system". */
  resolved: "light" | "dark";
  setMode: (mode: ThemeMode) => void;
  /** Re-resolve the system preference (e.g. on OS change). */
  refreshSystem: () => void;
}

function resolve(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") return systemPrefersDark() ? "dark" : "light";
  return mode;
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  mode: readInitial(),
  resolved: resolve(readInitial()),
  setMode: (mode) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(STORAGE_KEY, mode);
    set({ mode, resolved: resolve(mode) });
  },
  refreshSystem: () => {
    if (get().mode === "system") set({ resolved: resolve("system") });
  },
}));

// Track OS theme changes so "system" stays accurate.
if (typeof window !== "undefined" && typeof window.matchMedia === "function") {
  const mql = window.matchMedia("(prefers-color-scheme: dark)");
  mql.addEventListener("change", () => useThemeStore.getState().refreshSystem());
}
