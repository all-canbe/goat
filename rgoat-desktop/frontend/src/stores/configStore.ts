import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface ProviderInfo {
  name: string;
  model: string;
  provider_type: string;
  is_current: boolean;
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
      const providers = await tauriInvoke<ProviderInfo[]>("list_providers");
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
}));
