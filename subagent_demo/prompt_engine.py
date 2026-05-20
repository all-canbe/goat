from __future__ import annotations

import os as _os
import platform as _platform
import re as _re
from pathlib import Path
from typing import Any

from .prompt_templates import TEMPLATES


class PromptEngine:
    def __init__(self, templates: dict | None = None):
        self._templates = templates or TEMPLATES
        self._version = self._templates.get("version", 1)
        self._tool_descriptions = self._templates.get("tool_descriptions", {})
        self._language = self._templates.get("language", "")

    @property
    def version(self) -> int:
        return self._version

    def list_roles(self) -> list[dict]:
        roles = self._templates.get("roles", {})
        return [
            {
                "name": key,
                "display": info.get("display", key),
                "icon": info.get("icon", ""),
            }
            for key, info in roles.items()
        ]

    def get_role_info(self, role: str) -> dict | None:
        return self._templates.get("roles", {}).get(role)

    def get_version(self) -> int:
        return self._version

    def render_system(self, role: str, **kwargs: Any) -> str:
        roles = self._templates.get("roles", {})
        role_info = roles.get(role)
        if role_info is None:
            return ""

        template = role_info.get("system", "")

        base_vars = self._build_base_vars()
        base_vars.update(kwargs)

        rendered = self._render(template, base_vars)

        suffix_template = self._templates.get("agent_suffix", "")
        if suffix_template:
            suffix = self._render(suffix_template, base_vars)
            if suffix.strip():
                rendered += "\n" + suffix

        return rendered

    def render_main_system(self, role: str = "general",
                           skills: list[str] | None = None,
                           **kwargs: Any) -> str:
        rendered = self.render_system(role, **kwargs)

        skills_header = self._templates.get("skills_header", "")
        if skills and skills_header:
            skills_text = "\n".join(f"  - {s}" for s in skills)
            rendered += self._render(skills_header, {"skills": skills_text})

        return rendered

    def render_with_tools(self, role: str, tools: list[str],
                          **kwargs: Any) -> str:
        rendered = self.render_system(role, **kwargs)
        if tools:
            tool_lines = []
            for t in tools:
                desc = self._tool_descriptions.get(t, "")
                if desc:
                    tool_lines.append(f"  - {t}: {desc}")
                else:
                    tool_lines.append(f"  - {t}")
            tool_block = "\n".join(tool_lines)
            rendered += f"\n可用工具:\n{tool_block}\n"
        return rendered

    def _build_base_vars(self) -> dict:
        return {
            "cwd": str(Path.cwd()),
            "os": _platform.system(),
            "language": self._language,
            "agent_id": "",
            "agent_name": "",
            "depth": "0",
            "skills": "",
            "model": "",
            "tools": "",
        }

    def _render(self, template: str, vars: dict) -> str:
        def replace_match(m: _re.Match) -> str:
            key = m.group(1)
            return str(vars.get(key, m.group(0)))

        return _re.sub(r"\{(\w+)\}", replace_match, template)

    def get_raw_template(self, role: str, part: str = "system") -> str | None:
        role_info = self._templates.get("roles", {}).get(role)
        if role_info is None:
            return None
        return role_info.get(part)

    def build_tools_prompt(self, tool_names: list[str]) -> str:
        if not tool_names:
            return ""
        lines = ["可用工具:"]
        for t in tool_names:
            desc = self._tool_descriptions.get(t, t)
            lines.append(f"  - {t}: {desc}")
        return "\n".join(lines)


engine = PromptEngine()
