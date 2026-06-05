import { create } from 'zustand'

export interface SkillInfo {
  name: string
  description: string
}

export type FindSkillPhase = 'idle' | 'input'

interface SkillState {
  skills: SkillInfo[]
  isLoading: boolean
  phase: FindSkillPhase
  loadSkills: () => Promise<void>
  reloadSkills: () => Promise<void>
  startSearch: () => void
  cancelFlow: () => void
}

export const useSkillStore = create<SkillState>((set) => ({
  skills: [],
  isLoading: false,
  phase: 'idle',

  loadSkills: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/skills')
      const data = await res.json()
      set({ skills: data.skills || [] })
    } catch {
      set({ skills: [] })
    } finally {
      set({ isLoading: false })
    }
  },

  reloadSkills: async () => {
    set({ isLoading: true })
    try {
      const res = await fetch('/api/skills/reload', { method: 'POST' })
      const data = await res.json()
      if (data.success) {
        const res2 = await fetch('/api/skills')
        const data2 = await res2.json()
        set({ skills: data2.skills || [] })
      }
    } catch {
      set({ skills: [] })
    } finally {
      set({ isLoading: false })
    }
  },

  startSearch: () => {
    set({ phase: 'input' })
  },

  cancelFlow: () => {
    set({ phase: 'idle' })
  },
}))
