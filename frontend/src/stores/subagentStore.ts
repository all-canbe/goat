import { create } from 'zustand'

export interface SubAgentInfo {
  id: string
  name: string
  status: 'running' | 'completed' | 'error' | 'idle'
  progress: number
  task?: string
}

interface SubAgentStore {
  agents: SubAgentInfo[]
  addAgent: (agent: SubAgentInfo) => void
  updateAgent: (id: string, updates: Partial<SubAgentInfo>) => void
  removeAgent: (id: string) => void
}

export const useSubAgentStore = create<SubAgentStore>((set) => ({
  agents: [],
  addAgent: (agent) =>
    set((state) => ({ agents: [...state.agents, agent] })),
  updateAgent: (id, updates) =>
    set((state) => ({
      agents: state.agents.map((a) =>
        a.id === id ? { ...a, ...updates } : a
      ),
    })),
  removeAgent: (id) =>
    set((state) => ({
      agents: state.agents.filter((a) => a.id !== id),
    })),
}))