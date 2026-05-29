import { create } from 'zustand'

export interface McpServer {
  name: string
  command: string | null
  args: string[]
  url: string | null
  env: Record<string, string>
  transport: string | null
}

interface McpState {
  servers: McpServer[]
  isLoading: boolean
  loadServers: () => Promise<void>
  addServer: (server: McpServer) => Promise<void>
  updateServer: (name: string, server: Partial<McpServer>) => Promise<void>
  removeServer: (name: string) => Promise<void>
}

export const useMcpStore = create<McpState>((set, get) => ({
  servers: [],
  isLoading: false,

  loadServers: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/mcp/servers')
      const data = await res.json()
      set({ servers: data.servers || [] })
    } catch {
      console.error('loadServers failed')
    } finally {
      set({ isLoading: false })
    }
  },

  addServer: async (server) => {
    const res = await fetch('/api/mcp/servers', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(server),
    })
    const data = await res.json()
    if (data.success) {
      await get().loadServers()
    } else {
      throw new Error(data.error || 'add failed')
    }
  },

  updateServer: async (name, server) => {
    const res = await fetch(`/api/mcp/servers/${encodeURIComponent(name)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(server),
    })
    const data = await res.json()
    if (data.success) {
      await get().loadServers()
    } else {
      throw new Error(data.error || 'update failed')
    }
  },

  removeServer: async (name) => {
    await fetch(`/api/mcp/servers/${encodeURIComponent(name)}`, {
      method: 'DELETE',
    })
    set((state) => ({
      servers: state.servers.filter((s) => s.name !== name),
    }))
  },
}))