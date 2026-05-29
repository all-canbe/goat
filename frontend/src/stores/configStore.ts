import { create } from 'zustand'

interface ProviderOption {
  key: string
  display: string
  short: string
  default_model: string
  default_base_url: string
}

interface ConfigState {
  providerType: string
  baseUrl: string
  model: string
  apiKey: string
  availableProviders: ProviderOption[]
  isLoading: boolean
  isSaving: boolean
  workspace: string
  loadConfig: () => Promise<void>
  updateConfig: (config: { providerType?: string; baseUrl?: string; model?: string; apiKey?: string }) => Promise<void>
  loadWorkspace: () => Promise<void>
  setWorkspace: (path: string) => Promise<boolean>
}

export const useConfigStore = create<ConfigState>((set, get) => ({
  providerType: '',
  baseUrl: '',
  model: '',
  apiKey: '',
  availableProviders: [],
  isLoading: false,
  isSaving: false,
  workspace: '',

  loadConfig: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/config')
      if (!res.ok) throw new Error(`Failed to load config: ${res.status}`)
      const data = await res.json()
      set({
        providerType: data.providerType ?? '',
        baseUrl: data.baseUrl ?? '',
        model: data.model ?? '',
        apiKey: data.apiKey ?? '',
        availableProviders: data.availableProviders ?? [],
      })
    } catch (e) {
      console.error('loadConfig error:', e)
    } finally {
      set({ isLoading: false })
    }
    get().loadWorkspace()
  },

  updateConfig: async (config) => {
    set({ isSaving: true })
    try {
      const current = get()
      const body = {
        provider_type: config.providerType ?? current.providerType,
        base_url: config.baseUrl ?? current.baseUrl,
        model: config.model ?? current.model,
        api_key: config.apiKey ?? current.apiKey,
      }
      const res = await fetch('/api/config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      if (!res.ok) throw new Error(`Failed to update config: ${res.status}`)
      if (config.providerType) set({ providerType: config.providerType })
      if (config.baseUrl) set({ baseUrl: config.baseUrl })
      if (config.model) set({ model: config.model })
      if (config.apiKey) set({ apiKey: config.apiKey })
    } catch (e) {
      console.error('updateConfig error:', e)
      throw e
    } finally {
      set({ isSaving: false })
    }
  },

  loadWorkspace: async () => {
    try {
      const res = await fetch('/api/workspace')
      if (!res.ok) return
      const data = await res.json()
      set({ workspace: data.workspace ?? '' })
    } catch (e) {
      console.error('loadWorkspace error:', e)
    }
  },

  setWorkspace: async (path) => {
    try {
      const res = await fetch('/api/workspace', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path }),
      })
      if (!res.ok) {
        const err = await res.json()
        console.error('setWorkspace error:', err.detail || res.statusText)
        return false
      }
      const data = await res.json()
      set({ workspace: data.workspace })
      return true
    } catch (e) {
      console.error('setWorkspace error:', e)
      return false
    }
  },
}))