import { useEffect, useState } from 'react'
import { X, Settings, Loader2 } from 'lucide-react'
import { useConfigStore } from '@/stores/configStore'

interface SettingsPanelProps {
  isOpen: boolean
  onClose: () => void
}

export default function SettingsPanel({ isOpen, onClose }: SettingsPanelProps) {
  const {
    providerType,
    baseUrl,
    model,
    apiKey,
    availableProviders,
    isLoading,
    isSaving,
    loadConfig,
    updateConfig,
  } = useConfigStore()

  const [localProvider, setLocalProvider] = useState('')
  const [localApiKey, setLocalApiKey] = useState('')
  const [localBaseUrl, setLocalBaseUrl] = useState('')
  const [localModel, setLocalModel] = useState('')
  const [error, setError] = useState('')

  useEffect(() => {
    if (!isOpen) return
    loadConfig()
  }, [isOpen, loadConfig])

  useEffect(() => {
    setLocalProvider(providerType)
    setLocalApiKey(apiKey)
    setLocalBaseUrl(baseUrl)
    setLocalModel(model)
    setError('')
  }, [providerType, apiKey, baseUrl, model])

  if (!isOpen) return null

  const selectedProvider = availableProviders.find((p) => p.key === localProvider)

  const handleProviderChange = (key: string) => {
    setLocalProvider(key)
    const p = availableProviders.find((opt) => opt.key === key)
    if (p) {
      setLocalBaseUrl(p.default_base_url)
      setLocalModel(p.default_model)
    }
  }

  const handleSave = async () => {
    setError('')
    try {
      await updateConfig({
        providerType: localProvider,
        apiKey: localApiKey,
        baseUrl: localBaseUrl,
        model: localModel,
      })
      onClose()
    } catch {
      setError('保存失败，请重试')
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[460px] max-w-[90vw] shadow-2xl">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Settings size={16} className="text-primary" />
          <span className="text-text text-sm font-medium">配置</span>
        </div>

        <div className="px-4 py-4 space-y-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 size={20} className="text-text-dim animate-spin" />
            </div>
          ) : (
            <>
              <div>
                <label className="text-text-dim text-xs block mb-1">Provider</label>
                <select
                  value={localProvider}
                  onChange={(e) => handleProviderChange(e.target.value)}
                  className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none focus:border-primary"
                >
                  <option value="" disabled>选择 Provider</option>
                  {availableProviders.map((p) => (
                    <option key={p.key} value={p.key}>
                      {p.display}
                    </option>
                  ))}
                </select>
              </div>

              <div>
                <label className="text-text-dim text-xs block mb-1">API Key</label>
                <input
                  type="password"
                  value={localApiKey}
                  onChange={(e) => setLocalApiKey(e.target.value)}
                  placeholder="sk-..."
                  className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary"
                />
              </div>

              <div>
                <label className="text-text-dim text-xs block mb-1">Base URL</label>
                <input
                  type="text"
                  value={localBaseUrl}
                  onChange={(e) => setLocalBaseUrl(e.target.value)}
                  placeholder="https://api.openai.com/v1"
                  className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary"
                />
              </div>

              <div>
                <label className="text-text-dim text-xs block mb-1">Model</label>
                <input
                  type="text"
                  value={localModel}
                  onChange={(e) => setLocalModel(e.target.value)}
                  placeholder={selectedProvider?.default_model || 'gpt-4o'}
                  className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary"
                />
              </div>

              {error && (
                <p className="text-error text-xs">{error}</p>
              )}
            </>
          )}
        </div>

        <div className="flex justify-end gap-2 px-4 py-3 border-t border-border">
          <button
            onClick={onClose}
            disabled={isSaving}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors disabled:opacity-50"
          >
            <X size={14} />
            取消
          </button>
          <button
            onClick={handleSave}
            disabled={isSaving || isLoading}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors disabled:opacity-50"
          >
            {isSaving ? <Loader2 size={14} className="animate-spin" /> : null}
            {isSaving ? '保存中...' : '保存'}
          </button>
        </div>
      </div>
    </div>
  )
}