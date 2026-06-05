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
  reviewProviderType: string
  reviewBaseUrl: string
  reviewModel: string
  reviewApiKey: string
  reviewModelEnabled: boolean
  loadConfig: () => Promise<void>
  updateConfig: (config: {
    providerType?: string
    baseUrl?: string
    model?: string
    apiKey?: string
    reviewProviderType?: string
    reviewBaseUrl?: string
    reviewModel?: string
    reviewApiKey?: string
    reviewModelEnabled?: boolean
  }) => Promise<void>
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
  reviewProviderType: '',
  reviewBaseUrl: '',
  reviewModel: '',
  reviewApiKey: '',
  reviewModelEnabled: false,

  loadConfig: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/config')
      if (!res.ok) throw new Error(`Failed to load config: ${res.status}`)
      const data = await res.json()

      const rvProviderType = data.review_provider_type ?? ''
      const rvApiKey = data.review_api_key ?? ''
      const rvModel = data.review_model ?? ''

      set({
        providerType: data.providerType ?? '',
        baseUrl: data.baseUrl ?? '',
        model: data.model ?? '',
        apiKey: data.apiKey ?? '',
        availableProviders: data.availableProviders ?? [],
        reviewProviderType: rvProviderType,
        reviewBaseUrl: data.review_base_url ?? '',
        reviewModel: rvModel,
        reviewApiKey: rvApiKey,
        reviewModelEnabled: !!(rvProviderType || rvApiKey || rvModel),
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
      const body: Record<string, unknown> = {
        provider_type: config.providerType ?? current.providerType,
        base_url: config.baseUrl ?? current.baseUrl,
        model: config.model ?? current.model,
        api_key: config.apiKey ?? current.apiKey,
      }

      // review 字段
      if (config.reviewModelEnabled !== undefined) {
        body.review_model_enabled = config.reviewModelEnabled
      }
      if (config.reviewProviderType !== undefined) {
        body.review_provider_type = config.reviewProviderType
      }
      if (config.reviewBaseUrl !== undefined) {
        body.review_base_url = config.reviewBaseUrl
      }
      if (config.reviewModel !== undefined) {
        body.review_model = config.reviewModel
      }
      if (config.reviewApiKey !== undefined) {
        body.review_api_key = config.reviewApiKey
      }

      const res = await fetch('/api/config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })
      if (!res.ok) throw new Error(`Failed to update config: ${res.status}`)

      set({
        providerType: config.providerType ?? current.providerType,
        baseUrl: config.baseUrl ?? current.baseUrl,
        model: config.model ?? current.model,
        apiKey: config.apiKey ?? current.apiKey,
        reviewProviderType: config.reviewProviderType ?? current.reviewProviderType,
        reviewBaseUrl: config.reviewBaseUrl ?? current.reviewBaseUrl,
        reviewModel: config.reviewModel ?? current.reviewModel,
        reviewApiKey: config.reviewApiKey ?? current.reviewApiKey,
        reviewModelEnabled: config.reviewModelEnabled ?? current.reviewModelEnabled,
      })
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