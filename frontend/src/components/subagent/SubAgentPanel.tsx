import { useEffect, useState } from 'react'
import { ChevronDown, ChevronRight, Loader2, CheckCircle, XCircle, Minus } from 'lucide-react'
import { useSubAgentStore } from '@/stores/subagentStore'
import type { SubAgentInfo } from '@/stores/subagentStore'

function StatusIcon({ status }: { status: SubAgentInfo['status'] }) {
  switch (status) {
    case 'running':
      return <Loader2 size={12} className="text-primary-light animate-spin flex-shrink-0" />
    case 'completed':
      return <CheckCircle size={12} className="text-success flex-shrink-0" />
    case 'error':
      return <XCircle size={12} className="text-error flex-shrink-0" />
    default:
      return <Minus size={12} className="text-text-darker flex-shrink-0" />
  }
}

function AgentItem({ agent }: { agent: SubAgentInfo }) {
  return (
    <div className="flex items-start gap-2 px-2 py-1.5 rounded bg-surface-light/50">
      <StatusIcon status={agent.status} />
      <div className="flex-1 min-w-0">
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-text truncate">{agent.name}</span>
          <span className="text-[10px] text-text-darker flex-shrink-0">{agent.progress}%</span>
        </div>
        {agent.task && (
          <p className="text-[10px] text-text-darker truncate mt-0.5">{agent.task}</p>
        )}
        <div className="mt-1 h-1 bg-surface-lighter rounded-full overflow-hidden">
          <div
            className="h-full bg-primary rounded-full transition-all duration-300"
            style={{ width: `${agent.progress}%` }}
          />
        </div>
      </div>
    </div>
  )
}

export default function SubAgentPanel() {
  const [collapsed, setCollapsed] = useState(true)
  const agents = useSubAgentStore((s) => s.agents)
  const addAgent = useSubAgentStore((s) => s.addAgent)
  const updateAgent = useSubAgentStore((s) => s.updateAgent)

  useEffect(() => {
    const ws = (window as any).__wsClient
    if (!ws) return

    const unsubStart = ws.on('subagent.start', (msg: any) => {
      const p = msg.payload || {}
      addAgent({
        id: p.id || crypto.randomUUID(),
        name: p.name || 'Sub-Agent',
        status: 'running',
        progress: 0,
        task: p.task,
      })
    })

    const unsubComplete = ws.on('subagent.complete', (msg: any) => {
      const p = msg.payload || {}
      updateAgent(p.id, { status: 'completed', progress: 100 })
    })

    const unsubProgress = ws.on('subagent.progress', (msg: any) => {
      const p = msg.payload || {}
      updateAgent(p.id, { progress: p.progress ?? 0 })
    })

    return () => {
      unsubStart()
      unsubComplete()
      unsubProgress()
    }
  }, [addAgent, updateAgent])

  const activeAgents = agents.filter((a) => a.status === 'running' || a.status === 'idle' || a.status === 'error')
  const completedAgents = agents.filter((a) => a.status === 'completed')

  return (
    <div className="border-t border-border">
      <button
        onClick={() => setCollapsed(!collapsed)}
        className="w-full flex items-center gap-2 px-3 py-2 text-xs font-medium text-text-dim hover:text-text hover:bg-surface-light transition-colors"
      >
        {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
        子 Agent
        {agents.length > 0 && (
          <span className="ml-auto text-[10px] text-text-darker">{agents.length}</span>
        )}
      </button>
      {!collapsed && (
        <div className="px-2 pb-2 space-y-1">
          {agents.length === 0 && (
            <p className="text-text-darker text-xs text-center py-2">无活跃子 Agent</p>
          )}
          {activeAgents.length > 0 && (
            <div className="space-y-1">
              {activeAgents.map((agent) => (
                <AgentItem key={agent.id} agent={agent} />
              ))}
            </div>
          )}
          {completedAgents.length > 0 && (
            <>
              <p className="text-[10px] text-text-darker px-1 pt-1">已完成</p>
              {completedAgents.map((agent) => (
                <AgentItem key={agent.id} agent={agent} />
              ))}
            </>
          )}
        </div>
      )}
    </div>
  )
}