import { useState, useEffect } from 'react'
import { Settings, Sun, Moon } from 'lucide-react'
import { useConnectionStore } from '@/stores/connectionStore'
import { useChatStore } from '@/stores/chatStore'
import type { PermissionMode } from '@/types'
import SettingsPanel from '@/components/settings/SettingsPanel'

const MODE_COLORS: Record<PermissionMode, string> = {
  plan: 'text-mode-plan',
  agent: 'text-mode-agent',
  yolo: 'text-mode-yolo',
  flow: 'text-primary-light',
}

const MODE_NEXT: Record<PermissionMode, PermissionMode> = {
  plan: 'agent',
  agent: 'yolo',
  yolo: 'flow',
  flow: 'plan',
}

function getWsClient() {
  return (window as any).__wsClient
}

function getInitialTheme(): boolean {
  try {
    return localStorage.getItem('theme') === 'light'
  } catch {
    return false
  }
}

export default function StatusBar() {
  const status = useConnectionStore((s) => s.status)
  const mode = useConnectionStore((s) => s.mode)
  const setMode = useConnectionStore((s) => s.setMode)
  const tokenCount = useConnectionStore((s) => s.tokenCount)
  const contextPct = useConnectionStore((s) => s.contextPct)
  const isThinking = useConnectionStore((s) => s.isThinking)
  const isToolRunning = useConnectionStore((s) => s.isToolRunning)
  const toolCommand = useConnectionStore((s) => s.toolCommand)
  const hasPendingApproval = useConnectionStore((s) => s.hasPendingApproval)
  const notificationText = useConnectionStore((s) => s.notificationText)
  const providerName = useConnectionStore((s) => s.providerName)
  const modelName = useConnectionStore((s) => s.modelName)
  const tokenInput = useConnectionStore((s) => s.tokenInput)
  const tokenOutput = useConnectionStore((s) => s.tokenOutput)
  const cost = useConnectionStore((s) => s.cost)
  const messageCount = useConnectionStore((s) => s.messageCount)
  const activeState = useChatStore((s) => s.getActiveState())
  const isStreaming = activeState?.isStreaming ?? false
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [isLight, setIsLight] = useState(getInitialTheme)

  useEffect(() => {
    document.documentElement.classList.toggle('light', isLight)
  }, [])

  const statusColors: Record<string, string> = {
    connected: 'text-success',
    connecting: 'text-warning',
    disconnected: 'text-error',
  }

  const handleModeClick = () => {
    const next = MODE_NEXT[mode]
    setMode(next)
    const ws = getWsClient()
    if (ws) {
      ws.send('mode.change', { mode: next })
    }
  }

  const toggleTheme = () => {
    const next = !isLight
    setIsLight(next)
    document.documentElement.classList.toggle('light', next)
    try {
      localStorage.setItem('theme', next ? 'light' : 'dark')
    } catch {}
  }

  const statusText = notificationText
    ? notificationText
    : hasPendingApproval
      ? '\u23F3 \u7B49\u5F85\u5BA1\u6279'
      : isToolRunning
        ? `\u25B6 \u5DE5\u5177\u8FD0\u884C\u4E2D: ${toolCommand.length > 40 ? toolCommand.slice(0, 40) + '...' : toolCommand}`
        : isThinking
          ? '\u27D0 \u601D\u8003\u4E2D...'
          : isStreaming
            ? '\u25B6 \u6D41\u5F0F\u8F93\u51FA\u4E2D'
            : '\u23F8 \u7A7A\u95F2'

  const statusTextColor = notificationText
    ? 'text-warning'
    : hasPendingApproval
      ? 'text-error'
      : isToolRunning
        ? 'text-success'
        : isThinking
          ? 'text-mode-plan'
          : isStreaming
            ? 'text-error'
            : 'text-text-darker'

  return (
    <div className="h-7 bg-surface border-t border-border flex items-center px-3 text-xs text-text-dim gap-4">
      <span className="text-text-darker select-none">{'\uD83D\uDC10'} GOAT</span>

      {(providerName || modelName) && (
        <span className="text-text-darker">
          {providerName && <span>{providerName}</span>}
          {providerName && modelName && <span> </span>}
          {modelName && <span>{modelName}</span>}
        </span>
      )}

      <button
        onClick={handleModeClick}
        className={`${MODE_COLORS[mode]} hover:opacity-80 transition-opacity uppercase font-medium cursor-pointer select-none`}
        title="\u70B9\u51FB\u5207\u6362\u6A21\u5F0F (Tab)"
      >
        {mode}
      </button>

      <span className={statusColors[status] || 'text-text-dim'}>
        {'\u25CF'} {status}
      </span>

      <span className={statusTextColor}>{statusText}</span>

      {contextPct > 0 && <span className="text-text-darker">上下文: {contextPct}%</span>}
      {(tokenInput > 0 || tokenOutput > 0) ? (
        <>
          {tokenInput > 0 && <span className="text-text-darker">IN: {tokenInput}</span>}
          {tokenOutput > 0 && <span className="text-text-darker">OUT: {tokenOutput}</span>}
        </>
      ) : (
        tokenCount > 0 ? <span className="text-text-darker">Tokens: {tokenCount}</span> : null
      )}
      {cost > 0 && <span className="text-text-darker">${cost.toFixed(4)}</span>}
      {messageCount > 0 && <span className="text-text-darker">MSG: {messageCount}</span>}

      <button
        onClick={toggleTheme}
        className="text-text-darker hover:text-text transition-colors cursor-pointer"
        title={isLight ? '\u5207\u6362\u5230\u6697\u8272\u4E3B\u9898' : '\u5207\u6362\u5230\u4EAE\u8272\u4E3B\u9898'}
      >
        {isLight ? <Moon size={13} /> : <Sun size={13} />}
      </button>

      <button
        onClick={() => setSettingsOpen(true)}
        className="ml-auto flex items-center gap-1 text-text-darker hover:text-text transition-colors cursor-pointer"
        title="\u8BBE\u7F6E"
      >
        <Settings size={13} />
      </button>

      <span className="text-text-darker">v0.1.0</span>

      <SettingsPanel isOpen={settingsOpen} onClose={() => setSettingsOpen(false)} />
    </div>
  )
}