import { create } from 'zustand'

export interface TaskInfo {
  id: string
  name: string
  status: 'running' | 'completed' | 'failed' | 'cancelled'
  progress: number
  createdAt: number
}

interface TaskStore {
  tasks: TaskInfo[]
  addTask: (task: TaskInfo) => void
  updateTask: (id: string, updates: Partial<TaskInfo>) => void
  removeTask: (id: string) => void
}

export const useTaskStore = create<TaskStore>((set) => ({
  tasks: [],
  addTask: (task) => set((s) => ({ tasks: [...s.tasks, task] })),
  updateTask: (id, updates) =>
    set((s) => ({
      tasks: s.tasks.map((t) => (t.id === id ? { ...t, ...updates } : t)),
    })),
  removeTask: (id) => set((s) => ({ tasks: s.tasks.filter((t) => t.id !== id) })),
}))