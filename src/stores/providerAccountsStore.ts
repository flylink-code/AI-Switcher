import { create } from "zustand";

/** Account-login upstream families managed from the providers page drawer. */
export type ProviderAccountKind = "antigravity" | "kiro" | "chatgpt";

interface ProviderAccountsState {
  /** Which account drawer is open. Not persisted. */
  drawer: ProviderAccountKind | null;
  openDrawer: (kind: ProviderAccountKind) => void;
  closeDrawer: () => void;
}

export const useProviderAccountsStore = create<ProviderAccountsState>((set) => ({
  drawer: null,
  openDrawer: (drawer) => set({ drawer }),
  closeDrawer: () => set({ drawer: null }),
}));
