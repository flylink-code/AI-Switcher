import { create } from "zustand";
import type { Language } from "@/i18n";

const LANG_KEY = "cs.language";

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
  /** Whether the backend is reachable (verified by ping on startup). */
  backendReady: boolean;
  setLanguage: (lang: Language) => void;
  setBackendReady: (ready: boolean) => void;
}

export const useAppStore = create<AppState>((set) => ({
  language: readInitialLanguage(),
  backendReady: false,
  setLanguage: (lang) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(LANG_KEY, lang);
    set({ language: lang });
  },
  setBackendReady: (ready) => set({ backendReady: ready }),
}));
