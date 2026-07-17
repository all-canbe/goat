// D3-T05: Skill store — 镜像后端 list_skills / read_skill

import { create } from "zustand";
import { tauriInvoke } from "../lib/tauri-bridge";

export interface SkillInfo {
  name: string;
  description: string;
  /** "global" | "project" */
  source: string;
  /** SKILL.md 完整路径 */
  path: string;
}

interface SkillState {
  skills: SkillInfo[];
  loaded: boolean;
  loading: boolean;
  loadSkills: () => Promise<void>;
  readSkill: (name: string) => Promise<string | null>;
}

export const useSkillStore = create<SkillState>((set, get) => ({
  skills: [],
  loaded: false,
  loading: false,
  loadSkills: async () => {
    if (get().loading) return;
    set({ loading: true });
    try {
      const skills = await tauriInvoke<SkillInfo[]>("list_skills");
      set({ skills: skills || [], loaded: true });
    } catch (err) {
      console.error("[skillStore] loadSkills failed:", err);
      set({ skills: [], loaded: true });
    } finally {
      set({ loading: false });
    }
  },
  readSkill: async (name: string) => {
    try {
      const content = await tauriInvoke<string | null>("read_skill", { name });
      return content;
    } catch (err) {
      console.error("[skillStore] readSkill failed:", err);
      return null;
    }
  },
}));
