// P2: 主题切换 store — 暗色/亮色/跟随系统
import { create } from "zustand";

export type Theme = "dark" | "light" | "system";

const THEME_STORAGE_KEY = "theme";

/** 计算实际生效的主题（system → 根据系统偏好） */
function resolveEffectiveTheme(theme: Theme): "dark" | "light" {
  if (theme === "system") {
    if (typeof window !== "undefined" && window.matchMedia) {
      return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    }
    return "dark";
  }
  return theme;
}

/** 应用主题到 document.documentElement */
function applyThemeToDom(theme: Theme) {
  const effective = resolveEffectiveTheme(theme);
  const root = document.documentElement;
  if (effective === "light") {
    root.classList.add("light");
    root.classList.remove("dark");
  } else {
    root.classList.add("dark");
    root.classList.remove("light");
  }
}

interface ThemeState {
  theme: Theme;
  /** 应用主题并持久化 */
  setTheme: (t: Theme) => void;
  /** 初始化：读取 localStorage + 注册系统主题变化监听 */
  init: () => () => void;
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  theme: (() => {
    try {
      return (localStorage.getItem(THEME_STORAGE_KEY) as Theme) || "dark";
    } catch {
      return "dark";
    }
  })(),

  setTheme: (t) => {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, t);
    } catch {
      // localStorage 不可用时静默忽略
    }
    set({ theme: t });
    applyThemeToDom(t);
  },

  init: () => {
    const { theme } = get();
    applyThemeToDom(theme);

    // 监听系统主题变化（仅 system 模式下需要重新应用）
    const mql = window.matchMedia("(prefers-color-scheme: dark)");
    const handleChange = () => {
      if (get().theme === "system") {
        applyThemeToDom("system");
      }
    };
    mql.addEventListener("change", handleChange);
    return () => mql.removeEventListener("change", handleChange);
  },
}));
