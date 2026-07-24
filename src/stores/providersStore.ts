import { create } from "zustand";
import type { Provider, ProviderInput, ProviderTarget } from "@/types/backend";
import {
  createProvider,
  deleteProvider,
  importLiveConfig,
  listProviders,
  reorderProviders,
  switchProvider,
  switchToOfficial,
  updateProvider,
} from "@/services/api";

interface ProvidersState {
  providers: Provider[];
  loading: boolean;
  error: string | null;
  target: ProviderTarget;

  load: (target: ProviderTarget) => Promise<void>;
  create: (input: ProviderInput) => Promise<void>;
  update: (input: ProviderInput) => Promise<void>;
  remove: (id: string) => Promise<void>;
  switchTo: (id: string) => Promise<void>;
  useOfficial: () => Promise<void>;
  move: (id: string, direction: -1 | 1) => Promise<void>;
  importLive: () => Promise<void>;
  clearError: () => void;
}

export const useProvidersStore = create<ProvidersState>((set, get) => ({
  providers: [],
  loading: false,
  error: null,
  target: "claude_code",

  load: async (target) => {
    set({ loading: true, error: null, target });
    try {
      set({ providers: await listProviders(target), loading: false });
    } catch (e) {
      set({ loading: false, error: errMsg(e) });
    }
  },

  create: async (input) => {
    await createProvider({ ...input, targetApp: get().target });
    await get().load(get().target);
  },

  update: async (input) => {
    await updateProvider({ ...input, targetApp: get().target });
    await get().load(get().target);
  },

  remove: async (id) => {
    await deleteProvider(id);
    await get().load(get().target);
  },

  switchTo: async (id) => {
    await switchProvider(id);
    await get().load(get().target);
  },

  useOfficial: async () => {
    await switchToOfficial(get().target);
    await get().load(get().target);
  },

  move: async (id, direction) => {
    const ordered = get().providers.map((provider) => provider.id);
    const current = ordered.indexOf(id);
    const next = current + direction;
    if (current < 0 || next < 0 || next >= ordered.length) return;
    [ordered[current], ordered[next]] = [ordered[next], ordered[current]];
    const previous = get().providers;
    const reordered = [...previous];
    [reordered[current], reordered[next]] = [reordered[next], reordered[current]];
    set({ providers: reordered });
    try {
      await reorderProviders(ordered, get().target);
    } catch (e) {
      set({ providers: previous, error: errMsg(e) });
    }
  },

  importLive: async () => {
    await importLiveConfig(get().target);
    await get().load(get().target);
  },

  clearError: () => set({ error: null }),
}));

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
