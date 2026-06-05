import { useState, useEffect, useRef, useMemo } from 'react'
import { Search } from 'lucide-react'
import { useChatStore } from '@/stores/chatStore'
import { useConnectionStore } from '@/stores/connectionStore'
import { useSessionStore } from '@/stores/sessionStore'
import { useSkillStore } from '@/stores/skillStore'

interface CommandPaletteProps {
  onOpenFile?: () => void
  onNewFile?: () => void
  onEditFile?: (path: string, content: string) => void
}

interface Command {
  id: string
  label: string
  category: string
  action: () => void
}

export default function CommandPalette({ onOpenFile, onNewFile, onEditFile }: CommandPaletteProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const queryRef = useRef('')
  const listRef = useRef<HTMLDivElement>(null)
  const setMode = useConnectionStore((s) => s.setMode)

  function getWs() {
    return (window as any).__wsClient
  }

  function addSysMsg(content: string) {
    const sid = useSessionStore.getState().activeSessionId || 'default'
    useChatStore.getState().addMessage(sid, { id: crypto.randomUUID(), role: 'system', content, timestamp: Date.now() })
  }

  function extractArg(cmd: string): string {
    const q = queryRef.current
    const idx = q.indexOf(cmd)
    if (idx === -1) return ''
    return q.slice(idx + cmd.length).trim()
  }

  const commands: Command[] = useMemo(() => [
    { id: 'flow.plan_first', label: 'Flow: Plan First (计划优先)', category: '模式切换', action: () => {
      const ws = getWs()
      ws?.send('mode.change', { mode: 'flow' })
      setMode('flow')
      setTimeout(() => {
        ;(window as any).__setInputText?.('/flow_plan ')
        ;(window as any).__focusInput?.()
      }, 0)
    }},
    { id: 'mode.plan', label: 'Mode: Plan (安全模式)', category: '模式切换', action: () => {
      const ws = getWs()
      ws?.send('mode.change', { mode: 'plan' })
      setMode('plan')
    }},
    { id: 'mode.agent', label: 'Mode: Agent (代理模式)', category: '模式切换', action: () => {
      const ws = getWs()
      ws?.send('mode.change', { mode: 'agent' })
      setMode('agent')
    }},
    { id: 'mode.yolo', label: 'Mode: YOLO (自由模式)', category: '模式切换', action: () => {
      const ws = getWs()
      ws?.send('mode.change', { mode: 'yolo' })
      setMode('yolo')
    }},
    { id: 'mode.flow', label: 'Mode: Flow (流程模式)', category: '模式切换', action: () => {
      const ws = getWs()
      ws?.send('mode.change', { mode: 'flow' })
      setMode('flow')
    }},
    { id: 'session.new', label: '新建会话', category: '会话管理', action: () => {
      useSessionStore.getState().createSession()
    }},
    { id: 'session.rename', label: '重命名当前会话', category: '会话管理', action: () => {
      const id = useSessionStore.getState().activeSessionId
      if (!id) return
      const name = prompt('输入新名称:')
      if (name) {
        useSessionStore.getState().renameSession(id, name)
      }
    }},
    { id: 'file.new', label: '新建文件', category: '文件操作', action: () => {
      onNewFile?.()
    }},
    { id: 'file.open', label: '打开文件', category: '文件操作', action: () => {
      onOpenFile?.()
    }},
    { id: 'file.tree', label: '刷新文件树', category: '文件操作', action: () => {
      addSysMsg('请在左侧侧边栏点击「文件树」展开查看')
    }},
    { id: 'mcp.configure', label: '配置 MCP 服务器', category: 'MCP 管理', action: () => {
      const ws = getWs()
      ws?.send('mcp.configure', {})
    }},
    { id: 'mcp.connect', label: '接入 MCP 服务器', category: 'MCP 管理', action: () => {
      addSysMsg('请在左侧面板底部点击「MCP Servers」进行配置，或在终端使用 goat mcp add 命令')
    }},
    { id: 'session.delete', label: '删除当前会话', category: '会话管理', action: () => {
      const id = useSessionStore.getState().activeSessionId
      if (id) useSessionStore.getState().deleteSession(id)
    }},
    { id: 'session.fork', label: '分叉当前会话', category: '会话管理', action: async () => {
      const id = useSessionStore.getState().activeSessionId
      if (!id) return
      const res = await fetch(`/api/sessions/${id}/fork`, { method: 'POST', body: JSON.stringify({ turn: 10 }), headers: { 'Content-Type': 'application/json' } })
      const data = await res.json()
      if (data.forkedId) useSessionStore.getState().setActiveSession(data.forkedId)
      const sid = useSessionStore.getState().activeSessionId || 'default'
      useChatStore.getState().addMessage(sid, { id: crypto.randomUUID(), role: 'system', content: `已分叉新会话: ${data.title || data.forkedId}`, timestamp: Date.now() })
    }},
    { id: 'session.export', label: '导出当前会话 (JSON)', category: '会话管理', action: async () => {
      const id = useSessionStore.getState().activeSessionId
      if (!id) return
      const res = await fetch(`/api/sessions/${id}/export`)
      const data = await res.json()
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
      const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = `session-${id.slice(0,8)}.json`; a.click()
      const sid = useSessionStore.getState().activeSessionId || 'default'
      useChatStore.getState().addMessage(sid, { id: crypto.randomUUID(), role: 'system', content: `会话已导出为 session-${id.slice(0,8)}.json`, timestamp: Date.now() })
    }},
    { id: 'session.resume', label: '恢复最近会话', category: '会话管理', action: async () => {
      const res = await fetch('/api/sessions')
      const data = await res.json()
      const sessions = data.sessions || []
      if (sessions.length > 0) {
        useSessionStore.getState().setActiveSession(sessions[0].id)
      }
    }},
    { id: 'file.read', label: '读取文件 <path>', category: '文件操作', action: async () => {
      const path = extractArg('/read')
      if (!path) { addSysMsg('用法: /read <文件路径>'); return }
      try {
        const res = await fetch(`/api/files/read?path=${encodeURIComponent(path)}`)
        if (!res.ok) { addSysMsg(`读取失败: ${(await res.json()).detail || res.statusText}`); return }
        const data = await res.json()
        const preview = data.content.length > 1500 ? data.content.slice(0, 1500) + '\n\n... (共 ' + data.content.length + ' 字符)' : data.content
        addSysMsg(`📄 ${data.path} (${data.lineCount} 行)`)
        addSysMsg(preview)
      } catch { addSysMsg(`读取文件失败`) }
    }},
    { id: 'file.edit', label: '编辑文件 <path>', category: '文件操作', action: async () => {
      const path = extractArg('/edit')
      if (!path) { addSysMsg('用法: /edit <文件路径>'); return }
      try {
        const res = await fetch(`/api/files/read?path=${encodeURIComponent(path)}`)
        if (!res.ok) { addSysMsg(`读取失败: ${(await res.json()).detail || res.statusText}`); return }
        const data = await res.json()
        onEditFile?.(data.path, data.content)
      } catch { addSysMsg(`读取文件失败`) }
    }},
    { id: 'subagent.spawn', label: '创建子 Agent <角色> <任务>', category: '子 Agent', action: () => {
      addSysMsg('子 Agent 功能需要在 CLI 终端中使用，Web 模式暂不支持\n可用角色: general, explore, plan, implementer, review, verifier\n用法: /spawn <角色> <任务描述>')
    }},
    { id: 'subagent.list', label: '列出子 Agent', category: '子 Agent', action: () => {
      addSysMsg('子 Agent 状态请在 CLI 终端中使用 /list 查看')
    }},
    { id: 'subagent.collect', label: '收集子 Agent [ids]', category: '子 Agent', action: () => {
      addSysMsg('请在 CLI 终端中使用 /collect [ids] 收集子 Agent 结果')
    }},
    { id: 'subagent.cancel', label: '取消子 Agent <id>', category: '子 Agent', action: () => {
      addSysMsg('请在 CLI 终端中使用 /cancel <id> 取消子 Agent')
    }},
    { id: 'subagent.eval', label: '向子 Agent 发消息 <id> <msg>', category: '子 Agent', action: () => {
      addSysMsg('请在 CLI 终端中使用 /eval <id> <消息> 与子 Agent 通信')
    }},
    { id: 'chat.clear', label: '清空对话', category: '聊天操作', action: () => {
      const sid = useSessionStore.getState().activeSessionId || 'default'
      useChatStore.getState().clearSession(sid)
      addSysMsg('对话已清空')
    }},
    { id: 'info.help', label: '显示帮助', category: '信息查询', action: () => {
      const help = `可用命令:\n  /help — 显示帮助\n  /clear — 清空当前对话\n  /status — 显示系统状态\n\n审批模式:\n  /agent /plan /yolo /flow\n\n文件操作:\n  /read <path> — 读取文件\n  /edit <path> — 编辑文件\n  /tree — 文件树\n\n子 Agent:\n  /spawn /list /collect /cancel /eval\n\n会话管理:\n  /new, /fork, /export, /resume\n\n信息:\n  /status, /cost, /provider, /model, /roles\n\n技能:\n  /skills, /skills find, /skills install\n\n后台任务:\n  /task, /task_list, /task_cancel, /task_pause, /task_resume, /task_recover`
      addSysMsg(help)
    }},
    { id: 'info.status', label: '显示系统状态', category: '信息查询', action: () => {
      const s = useConnectionStore.getState()
      const chat = useChatStore.getState()
      const sid = useSessionStore.getState().activeSessionId || 'default'
      const sessionState = chat.sessions[sid]
      const msgCount = sessionState?.messages?.length ?? 0
      const lines = [
        `模式: ${s.mode}`,
        `连接: ${s.status}`,
        `Provider: ${s.providerName || '未设置'}`,
        `模型: ${s.modelName || '未设置'}`,
        `Token: ${s.tokenCount}`,
        `上下文: ${s.contextPct}%`,
        `消息数: ${msgCount}`,
      ]
      addSysMsg(lines.join('\n'))
    }},
    { id: 'info.cost', label: '显示 Token 成本统计', category: '信息查询', action: () => {
      const s = useConnectionStore.getState()
      addSysMsg(`Token 使用: ${s.tokenCount} | Provider: ${s.providerName || '未设置'} | 模型: ${s.modelName || '未设置'}`)
    }},
    { id: 'info.roles', label: '列出可用角色', category: '信息查询', action: () => {
      addSysMsg('可用角色类型:\n  general — 通用助手\n  explore — 代码探索专家 (只读工具)\n  plan — 任务规划专家 (只读 + write)\n  implementer — 代码实现专家 (读写 + 命令)\n  review — 代码审查专家 (只读工具)\n  verifier — 测试验证专家 (只读 + 命令)')
    }},
    { id: 'info.skills', label: '列出已安装技能', category: '技能管理', action: async () => {
      const store = useSkillStore.getState()
      await store.loadSkills()
      const skills = store.skills
      if (skills.length === 0) {
        addSysMsg('当前没有已安装的技能。\n可尝试「重新加载技能」命令，或在 CLI 终端中使用 /skills install <url> 安装。')
      } else {
        const lines = skills.map((s: any) => `  - ${s.name}: ${s.description || '无描述'}`)
        addSysMsg(`已安装技能 (${skills.length} 个):\n${lines.join('\n')}\n\nAgent 会自动使用已安装的技能。`)
      }
    }},
    { id: 'skills.reload', label: '重新加载技能', category: '技能管理', action: async () => {
      const store = useSkillStore.getState()
      await store.reloadSkills()
      const count = store.skills.length
      addSysMsg(`技能已重新加载，当前共 ${count} 个技能。`)
    }},
    { id: 'skills.find', label: '搜索并安装技能 (findskill)', category: '技能管理', action: () => {
      useSkillStore.getState().startSearch()
    }},
    { id: 'config.provider', label: '查看/切换 Provider', category: '网络配置', action: () => {
      const s = useConnectionStore.getState()
      addSysMsg(`当前 Provider: ${s.providerName || '未设置'} | 模型: ${s.modelName || '未设置'} | 请在设置面板中切换 (点击状态栏齿轮图标)`)
    }},
    { id: 'config.model', label: '查看/切换模型', category: '网络配置', action: () => {
      const s = useConnectionStore.getState()
      addSysMsg(`当前模型: ${s.modelName || '未设置'} | 请在设置面板中切换 (点击状态栏齿轮图标)`)
    }},
    { id: 'task.submit', label: '提交后台任务 <名称> [描述]', category: '后台任务', action: () => {
      addSysMsg('后台任务管理请在 CLI 终端中使用:\n  /task <名称> [描述] — 提交任务\n  /task_list [status] — 列出任务\n  /task_cancel <id> — 取消任务\n  /task_pause <id> — 暂停任务\n  /task_resume <id> — 恢复任务\n  /task_recover — 恢复中断任务')
    }},
    { id: 'task.list', label: '列出后台任务', category: '后台任务', action: () => {
      addSysMsg('请在 CLI 终端中使用 /task_list [status] 查看后台任务')
    }},
    { id: 'task.cancel', label: '取消任务 <id>', category: '后台任务', action: () => {
      addSysMsg('请在 CLI 终端中使用 /task_cancel <id> 取消任务')
    }},
    { id: 'task.pause', label: '暂停任务 <id>', category: '后台任务', action: () => {
      addSysMsg('请在 CLI 终端中使用 /task_pause <id> 暂停任务')
    }},
    { id: 'task.resume', label: '恢复任务 <id>', category: '后台任务', action: () => {
      addSysMsg('请在 CLI 终端中使用 /task_resume <id> 恢复任务')
    }},
    { id: 'task.recover', label: '恢复中断任务', category: '后台任务', action: () => {
      addSysMsg('请在 CLI 终端中使用 /task_recover 恢复中断的任务')
    }},
  ], [setMode, onOpenFile, onNewFile, onEditFile])

  // 动态从已加载技能列表生成 `/技能名` 触发命令
  const loadedSkills = useSkillStore((s) => s.skills)
  const skillCommands: Command[] = useMemo(() => {
    return loadedSkills.map((s) => ({
      id: `skill.${s.name}`,
      label: `/${s.name} — ${s.description ? s.description.slice(0, 40) : '应用该技能'}`,
      category: '技能触发',
      action: () => {
        setTimeout(() => {
          ;(window as any).__addSkillToInput?.(s.name)
        }, 0)
      },
    }))
  }, [loadedSkills])

  // 合并静态命令 + 动态技能命令
  const allCommands = useMemo(() => [...commands, ...skillCommands], [commands, skillCommands])

  useEffect(() => {
    const win = window as any
    win.__commandPaletteOpen = (q: string) => {
      setIsOpen(true)
      setQuery(q.startsWith('/') ? q.slice(1) : q)
    }
    return () => {
      delete win.__commandPaletteOpen
    }
  }, [])

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === '/' && !e.ctrlKey && !e.metaKey && !(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) {
        e.preventDefault()
        setIsOpen(true)
        setQuery('')
      }
      if (e.key === 'Escape' && isOpen) {
        setIsOpen(false)
        setQuery('')
        ;(window as any).__commandPaletteClose?.()
      }
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [isOpen])

  useEffect(() => {
    if (isOpen) {
      inputRef.current?.focus()
      setSelectedIndex(0)
    }
  }, [isOpen])

  const filtered = useMemo(() => {
    const q = query.toLowerCase()
    return q
      ? allCommands.filter((c) => c.label.toLowerCase().includes(q) || c.category.toLowerCase().includes(q))
      : allCommands
  }, [query, allCommands])

  useEffect(() => {
    if (!listRef.current || filtered.length === 0) return
    const el = listRef.current.querySelector(`[data-index="${selectedIndex}"]`)
    if (el instanceof HTMLElement) {
      el.scrollIntoView({ block: 'nearest' })
    }
  }, [selectedIndex, filtered.length])

  const execute = (cmd: Command) => {
    cmd.action()
    setIsOpen(false)
    setQuery('')
    ;(window as any).__clearInput?.()
  }

  if (!isOpen) return null

  const categories = [...new Set(filtered.map((c) => c.category))]

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh] bg-black/60"
      onClick={() => { setIsOpen(false); setQuery(''); (window as any).__commandPaletteClose?.() }}
    >
      <div
        className="bg-surface border border-border rounded-lg w-[520px] max-w-[90vw] shadow-2xl overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 px-3 py-2 border-b border-border">
          <Search size={16} className="text-text-dim flex-shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => { setQuery(e.target.value); setSelectedIndex(0); queryRef.current = e.target.value }}
            onKeyDown={(e) => {
              if (e.key === 'ArrowDown') {
                e.preventDefault()
                setSelectedIndex((prev) => (prev + 1) % filtered.length)
              } else if (e.key === 'ArrowUp') {
                e.preventDefault()
                setSelectedIndex((prev) => (prev - 1 + filtered.length) % filtered.length)
              } else if (e.key === 'Enter') {
                e.preventDefault()
                if (filtered[selectedIndex]) {
                  execute(filtered[selectedIndex])
                } else {
                  // fallback: `/技能名 [query]` 触发
                  const m = query.match(/^\/([a-z][a-z0-9_-]+)\s*([\s\S]*)$/i)
                  if (m) {
                    const skillName = m[1].toLowerCase()
                    const hit = useSkillStore.getState().skills.find(
                      (s) => s.name.toLowerCase() === skillName
                    )
                    if (hit) {
                      setTimeout(() => {
                        ;(window as any).__addSkillToInput?.(hit.name)
                      }, 0)
                      setIsOpen(false)
                      setQuery('')
                      ;(window as any).__clearInput?.()
                    }
                  }
                }
              } else if (e.key === 'Escape') {
                setIsOpen(false)
                setQuery('')
                ;(window as any).__commandPaletteClose?.()
              }
            }}
            placeholder="输入命令..."
            className="flex-1 bg-transparent text-sm text-text outline-none placeholder:text-text-darker"
          />
        </div>
        <div ref={listRef} className="max-h-64 overflow-y-auto">
          {filtered.length === 0 && (
            <div className="px-4 py-6 text-center text-text-darker text-sm">
              无匹配结果
            </div>
          )}
          {categories.map((cat) => (
            <div key={cat}>
              <div className="px-4 py-1.5 text-xs text-text-darker font-medium">{cat}</div>
              {filtered.filter((c) => c.category === cat).map((cmd) => {
                const actualIndex = filtered.indexOf(cmd)
                return (
                  <div
                    key={cmd.id}
                    data-index={actualIndex}
                    onClick={() => execute(cmd)}
                    className={`px-4 py-2 text-sm cursor-pointer transition-colors ${
                      actualIndex === selectedIndex
                        ? 'bg-primary-dim text-text-bright'
                        : 'text-text hover:bg-surface-light'
                    }`}
                  >
                    {cmd.label}
                  </div>
                )
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}