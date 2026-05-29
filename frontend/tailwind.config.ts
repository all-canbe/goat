import type { Config } from 'tailwindcss'

const config: Config = {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        bg: 'var(--color-bg)',
        surface: 'var(--color-surface)',
        'surface-light': 'var(--color-surface-light)',
        'surface-lighter': 'var(--color-surface-lighter)',
        primary: 'var(--color-primary)',
        'primary-light': 'var(--color-primary-light)',
        'primary-dim': 'var(--color-primary-dim)',
        'primary-glow': 'var(--color-primary-glow)',
        text: 'var(--color-text)',
        'text-dim': 'var(--color-text-dim)',
        'text-darker': 'var(--color-text-darker)',
        'text-bright': 'var(--color-text-bright)',
        success: 'var(--color-success)',
        warning: 'var(--color-warning)',
        error: 'var(--color-error)',
        border: 'var(--color-border)',
        'border-light': 'var(--color-border-light)',
        'border-focus': 'var(--color-border-focus)',
        scrollbar: 'var(--color-scrollbar)',
        'scrollbar-hover': 'var(--color-scrollbar-hover)',
        'selection-bg': 'var(--color-selection-bg)',
        'selection-fg': 'var(--color-selection-fg)',
        'mode-plan': 'var(--color-mode-plan)',
        'mode-agent': 'var(--color-mode-agent)',
        'mode-yolo': 'var(--color-mode-yolo)',
      },
      fontFamily: {
        mono: ['JetBrains Mono', 'Fira Code', 'Consolas', 'monospace'],
      },
      animation: {
        'fade-in': 'fadeIn 0.3s ease-out',
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0', transform: 'translateY(4px)' },
          '100%': { opacity: '1', transform: 'translateY(0)' },
        },
      },
    },
  },
  plugins: [],
}

export default config