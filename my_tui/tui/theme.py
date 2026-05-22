from dataclasses import dataclass, field


@dataclass
class RedBlackTheme:
    bg: str = "#0a0a0a"
    surface: str = "#1a1a1a"
    surface_light: str = "#2a2a2a"
    surface_lighter: str = "#3a3a3a"

    primary: str = "#cc0000"
    primary_light: str = "#ff1a1a"
    primary_dim: str = "#880000"
    primary_glow: str = "#ff3333"

    text: str = "#e0e0e0"
    text_dim: str = "#888888"
    text_darker: str = "#555555"
    text_bright: str = "#ffffff"

    success: str = "#00cc66"
    warning: str = "#ff6600"
    error: str = "#ff0000"

    border: str = "#3a0a0a"
    border_light: str = "#5a1a1a"
    border_focus: str = "#cc0000"

    scrollbar: str = "#4a0a0a"
    scrollbar_hover: str = "#6a1a1a"

    selection_bg: str = "#330000"
    selection_fg: str = "#ff6666"

    mode_plan: str = "#ff6600"
    mode_agent: str = "#cc0000"
    mode_yolo: str = "#ff0000"

    def css_vars(self) -> dict[str, str]:
        return {
            "bg": self.bg,
            "surface": self.surface,
            "surface-light": self.surface_light,
            "surface-lighter": self.surface_lighter,
            "primary": self.primary,
            "primary-light": self.primary_light,
            "primary-dim": self.primary_dim,
            "primary-glow": self.primary_glow,
            "text": self.text,
            "text-dim": self.text_dim,
            "text-darker": self.text_darker,
            "text-bright": self.text_bright,
            "success": self.success,
            "warning": self.warning,
            "error": self.error,
            "border": self.border,
            "border-light": self.border_light,
            "border-focus": self.border_focus,
            "scrollbar": self.scrollbar,
            "scrollbar-hover": self.scrollbar_hover,
            "selection-bg": self.selection_bg,
            "selection-fg": self.selection_fg,
            "mode-plan": self.mode_plan,
            "mode-agent": self.mode_agent,
            "mode-yolo": self.mode_yolo,
        }

    def to_css(self) -> str:
        v = self.css_vars()
        return f"""
$bg: {v['bg']};
$surface: {v['surface']};
$surface-light: {v['surface-light']};
$surface-lighter: {v['surface-lighter']};
$primary: {v['primary']};
$primary-light: {v['primary-light']};
$primary-dim: {v['primary-dim']};
$primary-glow: {v['primary-glow']};
$text: {v['text']};
$text-dim: {v['text-dim']};
$text-darker: {v['text-darker']};
$text-bright: {v['text-bright']};
$success: {v['success']};
$warning: {v['warning']};
$error: {v['error']};
$border: {v['border']};
$border-light: {v['border-light']};
$border-focus: {v['border-focus']};
$scrollbar: {v['scrollbar']};
$scrollbar-hover: {v['scrollbar-hover']};
$selection-bg: {v['selection-bg']};
$selection-fg: {v['selection-fg']};
$mode-plan: {v['mode-plan']};
$mode-agent: {v['mode-agent']};
$mode-yolo: {v['mode-yolo']};
"""


RED_BLACK_THEME = RedBlackTheme()