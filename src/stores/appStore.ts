import { create } from "zustand";
import type { Language } from "@/i18n";

const LANG_KEY = "cs.language";
const UI_MODE_KEY = "cs.uiMode";

function readInitialUiMode(): "v1" | "v2" {
  if (typeof localStorage !== "undefined") {
    const stored = localStorage.getItem(UI_MODE_KEY);
    if (stored === "v1" || stored === "v2") return stored;
  }
  return "v2";
}

function readInitialLanguage(): Language {
  if (typeof localStorage !== "undefined") {
    const stored = localStorage.getItem(LANG_KEY);
    if (stored === "zh-CN" || stored === "en-US") return stored;
  }
  // Browser heuristic.
  const nav = typeof navigator !== "undefined" ? navigator.language : "zh-CN";
  return nav?.toLowerCase().startsWith("en") ? "en-US" : "zh-CN";
}

interface AppState {
  language: Language;
  uiMode: "v1" | "v2";
  /** Whether the backend is reachable (verified by ping on startup). */
  backendReady: boolean;
  setLanguage: (lang: Language) => void;
  setUiMode: (mode: "v1" | "v2") => void;
  setBackendReady: (ready: boolean) => void;
}

export const useAppStore = create<AppState>((set) => ({
  language: readInitialLanguage(),
  uiMode: readInitialUiMode(),
  backendReady: false,
  setLanguage: (lang) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(LANG_KEY, lang);
    set({ language: lang });
  },
  setUiMode: (mode) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(UI_MODE_KEY, mode);
    set({ uiMode: mode });
  },
  setBackendReady: (ready) => set({ backendReady: ready }),
}));
