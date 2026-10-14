import { create } from "zustand";

/** Account-login upstream families managed on the gateway (accounts) page. */
export type ProviderAccountKind = "antigravity" | "kiro" | "chatgpt";

interface ProviderAccountsState {
  /** Which gateway tab the accounts page shows. Not persisted. */
  tab: ProviderAccountKind;
  setTab: (kind: ProviderAccountKind) => void;
}

export const useProviderAccountsStore = create<ProviderAccountsState>((set) => ({
  tab: "antigravity",
  setTab: (tab) => set({ tab }),
}));
