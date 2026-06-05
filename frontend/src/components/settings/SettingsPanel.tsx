import { useEffect, useState } from 'react'
import { X, Settings, Loader2 } from 'lucide-react'
import { useConfigStore } from '@/stores/configStore'

interface SettingsPanelProps {
  isOpen: boolean
  onClose: () => void
}

type TabKey = 'main' | 'review'

export default function SettingsPanel({ isOpen, onClose }: SettingsPanelProps) {
  const {
    providerType,
    baseUrl,
    model,
    apiKey,
    availableProviders,
    reviewProviderType,
    reviewBaseUrl,
    reviewModel,
    reviewApiKey,
    reviewModelEnabled,
    isLoading,
    isSaving,
    loadConfig,
    updateConfig,
  } = useConfigStore()

  // ---- 主模型本地状态 ----
  const [localProvider, setLocalProvider] = useState('')
  const [localApiKey, setLocalApiKey] = useState('')
  const [localBaseUrl, setLocalBaseUrl] = useState('')
  const [localModel, setLocalModel] = useState('')

  // ---- 审查模型本地状态 ----
  const [activeTab, setActiveTab] = useState<TabKey>('main')
  const [localReviewProvider, setLocalReviewProvider] = useState('')
  const [localReviewApiKey, setLocalReviewApiKey] = useState('')
  const [localReviewBaseUrl, setLocalReviewBaseUrl] = useState('')
  const [localReviewModel, setLocalReviewModel] = useState('')
  const [localReviewEnabled, setLocalReviewEnabled] = useState(false)

  const [error, setError] = useState('')

  // 打开时加载配置
  useEffect(() => {
    if (!isOpen) return
    loadConfig()
  }, [isOpen, loadConfig])

  // 同步主模型到本地
  useEffect(() => {
    setLocalProvider(providerType)
    setLocalApiKey(apiKey)
    setLocalBaseUrl(baseUrl)
    setLocalModel(model)
    setError('')
  }, [providerType, apiKey, baseUrl, model])

  // 同步审查模型到本地
  useEffect(() => {
    setLocalReviewProvider(reviewProviderType)
    setLocalReviewApiKey(reviewApiKey)
    setLocalReviewBaseUrl(reviewBaseUrl)
    setLocalReviewModel(reviewModel)
    setLocalReviewEnabled(reviewModelEnabled)
  }, [reviewProviderType, reviewApiKey, reviewBaseUrl, reviewModel, reviewModelEnabled])

  // 关闭时重置 Tab
  useEffect(() => {
    if (!isOpen) setActiveTab('main')
  }, [isOpen])

  if (!isOpen) return null

  const selectedProvider = availableProviders.find((p) => p.key === localProvider)
  const selectedReviewProvider = availableProviders.find((p) => p.key === localReviewProvider)

  const handleProviderChange = (key: string) => {
    setLocalProvider(key)
    const p = availableProviders.find((opt) => opt.key === key)
    if (p) {
      setLocalBaseUrl(p.default_base_url)
      setLocalModel(p.default_model)
    }
  }

  const handleReviewProviderChange = (key: string) => {
    setLocalReviewProvider(key)
    const p = availableProviders.find((opt) => opt.key === key)
    if (p) {
      setLocalReviewBaseUrl(p.default_base_url)
      setLocalReviewModel(p.default_model)
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
        reviewProviderType: localReviewEnabled ? localReviewProvider : '',
        reviewBaseUrl: localReviewEnabled ? localReviewBaseUrl : '',
        reviewModel: localReviewEnabled ? localReviewModel : '',
        reviewApiKey: localReviewEnabled ? localReviewApiKey : '',
        reviewModelEnabled: localReviewEnabled,
      })
      onClose()
    } catch {
      setError('保存失败，请重试')
    }
  }

  const tabBtnCls = (tab: TabKey) =>
    `px-3 py-1.5 text-sm border-b-2 transition-colors cursor-pointer select-none ${
      activeTab === tab
        ? 'text-primary border-primary'
        : 'text-text-dim border-transparent hover:text-text'
    }`

  // 渲染 4 字段表单
  const renderFields = (
    provVal: string,
    onProvChange: (v: string) => void,
    keyVal: string,
    onKeyChange: (v: string) => void,
    urlVal: string,
    onUrlChange: (v: string) => void,
    mdlVal: string,
    onMdlChange: (v: string) => void,
    disabled: boolean,
    defModel: string,
  ) => (
    <div className={disabled ? 'opacity-50 pointer-events-none' : ''}>
      <div>
        <label className="text-text-dim text-xs block mb-1">Provider</label>
        <select
          value={provVal}
          onChange={(e) => onProvChange(e.target.value)}
          disabled={disabled}
          className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none focus:border-primary disabled:cursor-not-allowed"
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
          value={keyVal}
          onChange={(e) => onKeyChange(e.target.value)}
          disabled={disabled}
          placeholder="sk-..."
          className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary disabled:cursor-not-allowed"
        />
      </div>

      <div>
        <label className="text-text-dim text-xs block mb-1">Base URL</label>
        <input
          type="text"
          value={urlVal}
          onChange={(e) => onUrlChange(e.target.value)}
          disabled={disabled}
          placeholder="https://api.openai.com/v1"
          className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary disabled:cursor-not-allowed"
        />
      </div>

      <div>
        <label className="text-text-dim text-xs block mb-1">Model</label>
        <input
          type="text"
          value={mdlVal}
          onChange={(e) => onMdlChange(e.target.value)}
          disabled={disabled}
          placeholder={defModel || 'gpt-4o'}
          className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary disabled:cursor-not-allowed"
        />
      </div>
    </div>
  )

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[460px] max-w-[90vw] shadow-2xl">
        {/* 标题栏 */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Settings size={16} className="text-primary" />
          <span className="text-text text-sm font-medium">配置</span>
        </div>

        {/* Tab 栏 */}
        <div className="flex border-b border-border">
          <button className={tabBtnCls('main')} onClick={() => setActiveTab('main')}>
            主模型
          </button>
          <button className={tabBtnCls('review')} onClick={() => setActiveTab('review')}>
            审查模型 (Flow)
          </button>
        </div>

        {/* 内容区 */}
        <div className="px-4 py-4 space-y-4">
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 size={20} className="text-text-dim animate-spin" />
            </div>
          ) : activeTab === 'main' ? (
            renderFields(
              localProvider,
              handleProviderChange,
              localApiKey,
              setLocalApiKey,
              localBaseUrl,
              setLocalBaseUrl,
              localModel,
              setLocalModel,
              false,
              selectedProvider?.default_model || '',
            )
          ) : (
            <>
              <label className="flex items-center gap-2 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={localReviewEnabled}
                  onChange={(e) => setLocalReviewEnabled(e.target.checked)}
                  className="w-4 h-4 rounded border-border bg-surface-light accent-primary"
                />
                <span className="text-text text-sm">启用独立审查模型</span>
              </label>
              {renderFields(
                localReviewProvider,
                handleReviewProviderChange,
                localReviewApiKey,
                setLocalReviewApiKey,
                localReviewBaseUrl,
                setLocalReviewBaseUrl,
                localReviewModel,
                setLocalReviewModel,
                !localReviewEnabled,
                selectedReviewProvider?.default_model || '',
              )}
            </>
          )}

          {error && <p className="text-error text-xs">{error}</p>}
        </div>

        {/* 按钮栏 */}
        <div className="flex justify-end gap-2 px-4 py-3 border-t border-border">
          <button
            onClick={onClose}
            disabled={isSaving}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors disabled:opacity-50 cursor-pointer"
          >
            <X size={14} />
            取消
          </button>
          <button
            onClick={handleSave}
            disabled={isSaving || isLoading}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors disabled:opacity-50 cursor-pointer"
          >
            {isSaving ? <Loader2 size={14} className="animate-spin" /> : null}
            {isSaving ? '保存中...' : '保存'}
          </button>
        </div>
      </div>
    </div>
  )
}