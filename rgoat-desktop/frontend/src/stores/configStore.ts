import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export type ProviderSource = "settings" | "env" | "fallback";

export interface ProviderInfo {
  name: string;
  model: string;
  provider_type: string;
  is_current: boolean;
  enabled: boolean;
  source: ProviderSource;
}

interface ConfigState {
  providers: ProviderInfo[];
  currentProvider: string;
  hasConfiguredProvider: boolean;
  loading: boolean;
  checkConfigured: () => Promise<void>;
  loadProviders: () => Promise<void>;
  switchProvider: (name: string) => Promise<void>;
  configureProvider: (
    base_url: string,
    api_key: string,
    model: string,
    name: string
  ) => Promise<void>;
  setProviderEnabled: (name: string, enabled: boolean) => Promise<void>;
  deleteProvider: (name: string) => Promise<void>;
}

function normalizeProvider(p: Partial<ProviderInfo>): ProviderInfo {
  return {
    name: p.name ?? "",
    model: p.model ?? "",
    provider_type: p.provider_type ?? "",
    is_current: Boolean(p.is_current),
    enabled: p.enabled ?? true,
    source: p.source ?? "settings",
  };
}

export const useConfigStore = create<ConfigState>((set, get) => ({
  providers: [],
  currentProvider: "",
  hasConfiguredProvider: false,
  loading: false,

  checkConfigured: async () => {
    try {
      const result = await tauriInvoke<boolean>("has_configured_provider");
      set({ hasConfiguredProvider: result });
    } catch {
      set({ hasConfiguredProvider: false });
    }
  },

  loadProviders: async () => {
    try {
      const raw = await tauriInvoke<Partial<ProviderInfo>[]>("list_providers");
      const providers = raw.map(normalizeProvider);
      const current = providers.find((p) => p.is_current);
      set({
        providers,
        currentProvider: current?.name || "",
      });
    } catch {
      // silently fail
    }
  },

  switchProvider: async (name) => {
    try {
      await tauriInvoke("switch_provider", { name });
      set({ currentProvider: name });
      await get().loadProviders();
    } catch {
      // silently fail
    }
  },

  configureProvider: async (base_url, api_key, model, name) => {
    await tauriInvoke("configure_provider", {
      base_url,
      api_key,
      model,
      name,
    });
    set({ hasConfiguredProvider: true, currentProvider: name });
    await get().loadProviders();
  },

  setProviderEnabled: async (name, enabled) => {
    await tauriInvoke("set_provider_enabled", { name, enabled });
    await get().loadProviders();
  },

  deleteProvider: async (name) => {
    await tauriInvoke("delete_provider", { name });
    await get().loadProviders();
  },
}));
