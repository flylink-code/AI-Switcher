import { create } from "zustand";
import type { Language } from "@/i18n";

const LANG_KEY = "cs.language";
/** New key for layout chrome mode. */
const LAYOUT_MODE_KEY = "cs.layoutMode";
/** Legacy V1/V2 experiment key — still read for migration. */
const LEGACY_UI_MODE_KEY = "cs.uiMode";

export type LayoutMode = "sidebar" | "top";

function readInitialLayoutMode(): LayoutMode {
  if (typeof localStorage === "undefined") return "top";

  const stored = localStorage.getItem(LAYOUT_MODE_KEY);
  if (stored === "sidebar" || stored === "top") return stored;

  // Migrate old V1/V2 labels.
  const legacy = localStorage.getItem(LEGACY_UI_MODE_KEY);
  if (legacy === "v1") return "sidebar";
  if (legacy === "v2") return "top";

  return "top";
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
  /** Chrome layout: left sidebar vs top pill navigation. */
  layoutMode: LayoutMode;
  /** Whether the backend is reachable (verified by ping on startup). */
  backendReady: boolean;
  setLanguage: (lang: Language) => void;
  setLayoutMode: (mode: LayoutMode) => void;
  setBackendReady: (ready: boolean) => void;
}

export const useAppStore = create<AppState>((set) => ({
  language: readInitialLanguage(),
  layoutMode: readInitialLayoutMode(),
  backendReady: false,
  setLanguage: (lang) => {
    if (typeof localStorage !== "undefined") localStorage.setItem(LANG_KEY, lang);
    set({ language: lang });
  },
  setLayoutMode: (mode) => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(LAYOUT_MODE_KEY, mode);
      // Keep legacy key in sync so older builds don't surprise-reset.
      localStorage.setItem(LEGACY_UI_MODE_KEY, mode === "sidebar" ? "v1" : "v2");
    }
    set({ layoutMode: mode });
  },
  setBackendReady: (ready) => set({ backendReady: ready }),
}));
