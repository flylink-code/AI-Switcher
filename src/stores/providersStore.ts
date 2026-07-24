import { create } from "zustand";
import type { Provider, ProviderInput, PresetInfo } from "@/types/backend";
import {
  createProvider,
  deleteProvider,
  importLiveConfig,
  listPresets,
  listProviders,
  reorderProviders,
  switchProvider,
  switchToOfficial,
  updateProvider,
} from "@/services/api";

interface ProvidersState {
  providers: Provider[];
  presets: PresetInfo[];
  loading: boolean;
  /** Last error message, surfaced in the UI banner. */
  error: string | null;

  load: () => Promise<void>;
  loadPresets: () => Promise<void>;
  create: (input: ProviderInput) => Promise<void>;
  update: (input: ProviderInput) => Promise<void>;
  remove: (id: string) => Promise<void>;
  /** Switch to a provider. Throws on validation error (caller shows message). */
  switchTo: (id: string) => Promise<void>;
  /** Switch to official login mode. */
  useOfficial: () => Promise<void>;
  /** Move a provider up or down by one slot. */
  move: (id: string, direction: -1 | 1) => Promise<void>;
  /** Re-import the live settings.json into the DB, then reload. */
  importLive: () => Promise<void>;
  clearError: () => void;
}

export const useProvidersStore = create<ProvidersState>((set, get) => ({
  providers: [],
  presets: [],
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const providers = await listProviders();
      set({ providers, loading: false });
    } catch (e) {
      set({ loading: false, error: errMsg(e) });
    }
  },

  loadPresets: async () => {
    try {
      const presets = await listPresets();
      set({ presets });
    } catch (e) {
      set({ error: errMsg(e) });
    }
  },

  create: async (input) => {
    await createProvider(input);
    await get().load();
  },

  update: async (input) => {
    await updateProvider(input);
    await get().load();
  },

  remove: async (id) => {
    await deleteProvider(id);
    await get().load();
  },

  switchTo: async (id) => {
    await switchProvider(id);
    await get().load();
  },

  useOfficial: async () => {
    await switchToOfficial();
    await get().load();
  },

  move: async (id, direction) => {
    const ordered = get().providers.map((p) => p.id);
    const i = ordered.indexOf(id);
    const j = i + direction;
    if (i < 0 || j < 0 || j >= ordered.length) return;
    [ordered[i], ordered[j]] = [ordered[j], ordered[i]];
    // Optimistic reorder + persist.
    const prev = get().providers;
    const reordered = [...prev];
    const tmp = reordered[i];
    reordered[i] = reordered[j];
    reordered[j] = tmp;
    set({ providers: reordered });
    try {
      await reorderProviders(ordered);
    } catch (e) {
      set({ providers: prev, error: errMsg(e) });
    }
  },

  importLive: async () => {
    await importLiveConfig();
    await get().load();
  },

  clearError: () => set({ error: null }),
}));

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
