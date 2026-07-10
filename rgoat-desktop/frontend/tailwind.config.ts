import type { Config } from "tailwindcss";

export default {
  content: ["src/**/*.{js,ts,jsx,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        bg: "var(--bg)",
        surface: "var(--surface)",
        surfaceLight: "var(--surface-light)",
        primary: "var(--primary)",
        text: "var(--text)",
        textMuted: "var(--text-muted)",
        success: "var(--success)",
        warning: "var(--warning)",
        error: "var(--error)",
        border: "var(--border)",
      },
    },
  },
  plugins: [],
} satisfies Config;
