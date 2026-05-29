import { useEffect, useState } from 'react'
import {
  X,
  Plus,
  Server,
  Trash2,
  Plug,
  Loader2,
  ChevronDown,
  ChevronUp,
  Pencil,
} from 'lucide-react'
import { useMcpStore, type McpServer } from '@/stores/mcpStore'

interface McpConfigPageProps {
  isOpen: boolean
  onClose: () => void
}

export default function McpConfigPage({ isOpen, onClose }: McpConfigPageProps) {
  const { servers, isLoading, loadServers, addServer, updateServer, removeServer } = useMcpStore()

  const [showJsonInput, setShowJsonInput] = useState(false)
  const [jsonInput, setJsonInput] = useState('')
  const [jsonError, setJsonError] = useState('')
  const [parsedServers, setParsedServers] = useState<McpServer[]>([])
  const [saving, setSaving] = useState(false)
  const [editName, setEditName] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [testing, setTesting] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<{ name: string; ok: boolean; msg: string } | null>(null)
  const [expanded, setExpanded] = useState<string | null>(null)

  useEffect(() => {
    if (isOpen) loadServers()
  }, [isOpen, loadServers])

  if (!isOpen) return null

  const resetJsonInput = () => {
    setJsonInput('')
    setJsonError('')
    setParsedServers([])
    setShowJsonInput(false)
    setEditName(null)
  }

  const startEdit = (server: McpServer) => {
    const obj: Record<string, any> = { mcpServers: {} }
    obj.mcpServers[server.name] = { command: server.command, args: server.args }
    if (server.url) obj.mcpServers[server.name].url = server.url
    if (server.transport && server.transport !== 'stdio') {
      obj.mcpServers[server.name].transport = server.transport
    }
    if (Object.keys(server.env).length > 0) {
      obj.mcpServers[server.name].env = server.env
    }
    setJsonInput(JSON.stringify(obj, null, 2))
    setEditName(server.name)
    setJsonError('')
    setParsedServers([])
    setShowJsonInput(true)
  }

  const parseJsonConfig = () => {
    setJsonError('')
    setParsedServers([])

    const trimmed = jsonInput.trim()
    if (!trimmed) {
      setJsonError('Please paste your MCP configuration JSON')
      return
    }

    let parsed: any
    try {
      parsed = JSON.parse(trimmed)
    } catch (e) {
      setJsonError('Invalid JSON: ' + (e as Error).message)
      return
    }

    if (!parsed.mcpServers || typeof parsed.mcpServers !== 'object' || Array.isArray(parsed.mcpServers)) {
      setJsonError('JSON must contain a "mcpServers" object at the top level')
      return
    }

    const entries = Object.entries(parsed.mcpServers)
    if (entries.length === 0) {
      setJsonError('No servers found in mcpServers')
      return
    }

    const servers: McpServer[] = []
    for (const [name, config] of entries) {
      const cfg = config as any
      if (!cfg.command && !cfg.url) {
        setJsonError(`Server "${name}" must have a "command" or "url"`)
        return
      }
      servers.push({
        name,
        command: cfg.command || null,
        args: Array.isArray(cfg.args) ? cfg.args : [],
        url: cfg.url || null,
        env: cfg.env || {},
        transport: cfg.transport || 'stdio',
      })
    }
    setParsedServers(servers)
  }

  const handleSave = async () => {
    if (parsedServers.length === 0) return
    setSaving(true)
    try {
      for (const server of parsedServers) {
        if (editName) {
          await updateServer(editName, server)
        } else {
          await addServer(server)
        }
      }
      resetJsonInput()
    } catch (e: any) {
      setJsonError(e.message || 'Save failed')
    } finally {
      setSaving(false)
    }
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    await removeServer(deleteTarget)
    setDeleteTarget(null)
  }

  const handleTest = async (server: McpServer) => {
    setTesting(server.name)
    setTestResult(null)
    try {
      const res = await fetch(`/api/mcp/servers/${encodeURIComponent(server.name)}/test`, { method: 'POST' })
      const data = await res.json()
      setTestResult({ name: server.name, ok: data.ok ?? false, msg: data.message || (data.ok ? 'Connected' : 'Failed') })
    } catch {
      setTestResult({ name: server.name, ok: false, msg: 'Connection error' })
    } finally {
      setTesting(null)
    }
  }

  const toggleExpanded = (name: string) => {
    setExpanded(expanded === name ? null : name)
    setTestResult(null)
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-surface border border-border rounded-lg w-[600px] max-w-[95vw] max-h-[85vh] flex flex-col shadow-2xl">
        <div className="flex items-center justify-between px-4 py-3 border-b border-border shrink-0">
          <div className="flex items-center gap-2">
            <Server size={16} className="text-primary" />
            <span className="text-text text-sm font-medium">MCP Servers</span>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded hover:bg-surface-light text-text-dim hover:text-text transition-colors"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto px-4 py-3 space-y-3">
          {isLoading ? (
            <div className="flex items-center justify-center py-8">
              <Loader2 size={20} className="text-text-dim animate-spin" />
            </div>
          ) : servers.length === 0 && !showJsonInput ? (
            <p className="text-text-darker text-sm text-center py-8">No MCP servers configured</p>
          ) : null}

          {servers.map((server) => (
            <div key={server.name} className="border border-border rounded-lg overflow-hidden">
              <div className="flex items-center justify-between px-3 py-2 bg-surface-light">
                <button
                  onClick={() => toggleExpanded(server.name)}
                  className="flex items-center gap-2 text-sm text-text font-medium flex-1 text-left"
                >
                  {expanded === server.name ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
                  {server.name}
                </button>
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => handleTest(server)}
                    disabled={testing === server.name}
                    className="p-1 rounded hover:bg-surface-lighter text-text-dim hover:text-text transition-colors disabled:opacity-50"
                    title="Test connection"
                  >
                    {testing === server.name ? <Loader2 size={14} className="animate-spin" /> : <Plug size={14} />}
                  </button>
                  <button
                    onClick={() => startEdit(server)}
                    className="p-1 rounded hover:bg-surface-lighter text-text-dim hover:text-text transition-colors"
                    title="Edit"
                  >
                    <Pencil size={14} />
                  </button>
                  <button
                    onClick={() => setDeleteTarget(server.name)}
                    className="p-1 rounded hover:bg-surface-lighter text-text-dim hover:text-error transition-colors"
                    title="Delete"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
              {expanded === server.name && (
                <div className="px-3 py-2 space-y-1 text-xs text-text-dim">
                  {server.command && <div>Command: <span className="text-text">{server.command} {server.args.join(' ')}</span></div>}
                  {server.url && <div>URL: <span className="text-text">{server.url}</span></div>}
                  {server.transport && <div>Transport: <span className="text-text">{server.transport}</span></div>}
                  {Object.keys(server.env).length > 0 && (
                    <div>
                      <span>Env: </span>
                      {Object.entries(server.env).map(([k, v]) => (
                        <span key={k} className="text-text">{k}={v} </span>
                      ))}
                    </div>
                  )}
                  {testResult && testResult.name === server.name && (
                    <div className={testResult.ok ? 'text-success' : 'text-error'}>
                      {testResult.ok ? '✓' : '✗'} {testResult.msg}
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}

          {showJsonInput && (
            <div className="border border-border rounded-lg p-4 space-y-3">
              <div>
                <label className="text-text-dim text-xs block mb-1">
                  {editName ? `Edit Server: ${editName}` : 'Paste MCP Configuration (JSON)'}
                </label>
                <textarea
                  value={jsonInput}
                  onChange={(e) => {
                    setJsonInput(e.target.value)
                    if (jsonError || parsedServers.length > 0) {
                      setJsonError('')
                      setParsedServers([])
                    }
                  }}
                  onPaste={() => setTimeout(parseJsonConfig, 0)}
                  placeholder='{\n  "mcpServers": {\n    "example-server": {\n      "command": "npx",\n      "args": ["-y", "@modelcontextprotocol/server-filesystem"]\n    }\n  }\n}'
                  rows={8}
                  className="w-full bg-surface-light border border-border rounded px-3 py-1.5 text-sm text-text outline-none placeholder:text-text-darker focus:border-primary font-mono resize-y"
                  spellCheck={false}
                />
                <p className="text-text-darker text-xs mt-1">
                  Paste a JSON with the standard <code className="text-text-dim bg-surface-light px-1 rounded">mcpServers</code> format. It will be parsed automatically on paste.
                </p>
              </div>

              {jsonInput && parsedServers.length === 0 && !jsonError && (
                <button
                  onClick={parseJsonConfig}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors"
                >
                  Validate & Parse
                </button>
              )}

              {jsonError && (
                <div className="flex items-start gap-2 text-error text-xs p-2 bg-error/10 rounded">
                  <span>✗ {jsonError}</span>
                </div>
              )}

              {parsedServers.length > 0 && (
                <div className="space-y-2">
                  <div className="flex items-center gap-1.5 text-success text-xs">
                    <span>✓</span>
                    <span>Parsed {parsedServers.length} server configuration(s)</span>
                  </div>
                  {parsedServers.map((s) => (
                    <div key={s.name} className="text-xs text-text bg-surface-light rounded p-2 border border-border">
                      <span className="text-primary-light font-medium">{s.name}</span>
                      {s.command && <span className="text-text-dim ml-2">→ {s.command} {s.args.join(' ')}</span>}
                      {s.url && <span className="text-text-dim ml-2">→ {s.url}</span>}
                      {s.transport && s.transport !== 'stdio' && (
                        <span className="text-text-dim ml-2">({s.transport})</span>
                      )}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        <div className="flex justify-between items-center px-4 py-3 border-t border-border shrink-0">
          <div className="flex gap-2">
            {!showJsonInput ? (
              <button
                onClick={() => { resetJsonInput(); setShowJsonInput(true) }}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors"
              >
                <Plus size={14} />
                Add Server
              </button>
            ) : null}
          </div>
          <div className="flex gap-2">
            {showJsonInput && (
              <>
                <button
                  onClick={resetJsonInput}
                  disabled={saving}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors disabled:opacity-50"
                >
                  <X size={14} />
                  Cancel
                </button>
                <button
                  onClick={handleSave}
                  disabled={saving || parsedServers.length === 0}
                  className="flex items-center gap-1.5 px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors disabled:opacity-50"
                >
                  {saving ? <Loader2 size={14} className="animate-spin" /> : null}
                  {saving ? 'Saving...' : editName ? 'Update' : 'Save'}
                </button>
              </>
            )}
          </div>
        </div>
      </div>

      {deleteTarget && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center bg-black/60">
          <div className="bg-surface border border-border rounded-lg w-[320px] shadow-2xl p-4">
            <p className="text-text text-sm mb-4">
              Delete MCP server <span className="text-primary-light font-medium">{deleteTarget}</span>?
            </p>
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setDeleteTarget(null)}
                className="px-3 py-1.5 rounded text-sm bg-surface-light text-text-dim hover:bg-surface-lighter hover:text-text transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleDelete}
                className="px-3 py-1.5 rounded text-sm bg-primary text-white hover:bg-primary-light transition-colors"
              >
                Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}