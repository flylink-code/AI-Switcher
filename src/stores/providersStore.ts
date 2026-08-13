import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import type {
  Provider,
  ProviderHealthUpdated,
  ProviderInput,
  ProviderTarget,
  SwitchProviderResult,
} from "@/types/backend";
import {
  createProvider,
  deleteProvider,
  importLiveConfig,
  reorderProviders,
  switchProvider,
  switchToOfficial,
  updateProvider,
} from "@/services/api";
import { providerListOptions } from "@/lib/appQueries";
import { queryClient } from "@/lib/queryClient";

interface ProvidersState {
  providers: Provider[];
  loading: boolean;
  error: string | null;
  target: ProviderTarget;

  load: (target: ProviderTarget) => Promise<void>;
  create: (input: ProviderInput) => Promise<void>;
  update: (input: ProviderInput) => Promise<void>;
  remove: (id: string) => Promise<void>;
  switchTo: (id: string) => Promise<SwitchProviderResult>;
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
    const options = providerListOptions(target);
    const cached = queryClient.getQueryData<Provider[]>(options.queryKey);
    set({
      providers: cached ?? [],
      loading: !cached,
      error: null,
      target,
    });
    try {
      const providers = await queryClient.fetchQuery(options);
      set({ providers, loading: false });
    } catch (e) {
      set({ loading: false, error: errMsg(e) });
    }
  },

  create: async (input) => {
    await createProvider({ ...input, targetApp: get().target });
    await queryClient.invalidateQueries({ queryKey: providerListOptions(get().target).queryKey });
    await get().load(get().target);
  },

  update: async (input) => {
    await updateProvider({ ...input, targetApp: get().target });
    await queryClient.invalidateQueries({ queryKey: providerListOptions(get().target).queryKey });
    await get().load(get().target);
  },

  remove: async (id) => {
    await deleteProvider(id);
    await queryClient.invalidateQueries({ queryKey: providerListOptions(get().target).queryKey });
    await get().load(get().target);
  },

  switchTo: async (id) => {
    const result = await switchProvider(id);
    const provider = result.provider;
    set({
      providers: get().providers.map((item) => ({
        ...item,
        isCurrent: item.id === provider.id,
      })),
    });
    queryClient.setQueryData<Provider[]>(
      providerListOptions(get().target).queryKey,
      (current = []) =>
        current.map((item) => ({ ...item, isCurrent: item.id === provider.id })),
    );
    await queryClient.invalidateQueries({ queryKey: ["proxy-status", get().target] });
    return result;
  },

  useOfficial: async () => {
    await switchToOfficial(get().target);
    set({
      providers: get().providers.map((provider) => ({
        ...provider,
        isCurrent: false,
      })),
    });
    queryClient.setQueryData<Provider[]>(
      providerListOptions(get().target).queryKey,
      (current = []) => current.map((provider) => ({ ...provider, isCurrent: false })),
    );
    await queryClient.invalidateQueries({ queryKey: ["proxy-status", get().target] });
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
      queryClient.setQueryData(providerListOptions(get().target).queryKey, reordered);
    } catch (e) {
      set({ providers: previous, error: errMsg(e) });
    }
  },

  importLive: async () => {
    await importLiveConfig(get().target);
    await queryClient.invalidateQueries({ queryKey: providerListOptions(get().target).queryKey });
    await get().load(get().target);
  },

  clearError: () => set({ error: null }),
}));

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

let healthEventsInitialized = false;

export function initializeProviderHealthEvents(): void {
  if (healthEventsInitialized) return;
  healthEventsInitialized = true;
  void listen<ProviderHealthUpdated>("provider-health-updated", ({ payload }) => {
    queryClient.setQueryData<Provider[]>(
      providerListOptions(payload.targetApp).queryKey,
      (current = []) =>
        current.map((provider) =>
          provider.id === payload.providerId
            ? {
                ...provider,
                healthStatus: payload.ok ? "healthy" : "error",
                healthCheckedAt: payload.checkedAt,
                healthLatencyMs: payload.latencyMs ?? null,
              }
            : provider,
        ),
    );
    useProvidersStore.setState((state) => {
      if (state.target !== payload.targetApp) return state;
      return {
        providers: state.providers.map((provider) =>
          provider.id === payload.providerId
            ? {
                ...provider,
                healthStatus: payload.ok ? "healthy" : "error",
                healthCheckedAt: payload.checkedAt,
                healthLatencyMs: payload.latencyMs ?? null,
              }
            : provider,
        ),
      };
    });
  });
}
