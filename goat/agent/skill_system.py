from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from langchain_core.tools import BaseTool


@dataclass
class Skill:
    name: str
    description: str
    tools: list[BaseTool] = field(default_factory=list)
    system_prompt_template: str = ""
    metadata: dict = field(default_factory=dict)

    path: Path | None = None
    agents: dict[str, str] = field(default_factory=dict)
    references: dict[str, str] = field(default_factory=dict)
    scripts: list[Path] = field(default_factory=list)

    SKILL_DIR_REQUIRED_FILES: set[str] = field(default_factory=lambda: {"SKILL.md"})

    def to_prompt(self) -> str:
        parts = ["=" * 40,
                 f"技能: {self.name}",
                 self.description,
                 "=" * 40,
                 self.system_prompt_template]

        # Agent 文件
        if self.agents:
            parts.append("\n可用 Agent 文件（可通过 read_file 读取）:")
            for name, content in self.agents.items():
                summary = content.strip().split('\n')[0][:80] if content else ""
                rel_path = f"agents/{name}.md"
                parts.append(f"  - {rel_path}  # {summary}")

        # 参考文档
        if self.references:
            parts.append("\n可用参考文档（可通过 read_file 读取）:")
            for name, content in self.references.items():
                summary = content.strip().split('\n')[0][:80] if content else ""
                parts.append(f"  - references/{name}.md  # {summary}")

        # 脚本
        if self.scripts and self.path:
            parts.append("\n可用脚本（可通过 execute_command 运行）:")
            for script in self.scripts:
                rel = script.relative_to(self.path)
                parts.append(f"  - {rel}")

        parts.append("=" * 40)
        return "\n".join(parts)

    def has_valid_content(self) -> bool:
        return bool(self.name and (self.description or self.system_prompt_template))

    def get_tools(self) -> list[BaseTool]:
        return self.tools

    def get_agent_prompt(self, agent_name: str) -> str | None:
        return self.agents.get(agent_name)

    def get_reference(self, ref_name: str) -> str | None:
        return self.references.get(ref_name)

    def resolve_script_path(self, script_name: str) -> Path | None:
        if not self.path:
            return None
        for script in self.scripts:
            if script.name == script_name:
                return script
        candidate = self.path / "scripts" / script_name
        return candidate if candidate.exists() else None


def _parse_yaml_frontmatter(text: str) -> tuple[dict[str, Any], str]:
    text = text.lstrip("\ufeff")
    match = re.match(r"^---\s*\n(.*?)\n---\s*\n(.*)", text, re.DOTALL)
    if not match:
        return {}, text

    raw_yaml = match.group(1)
    body = match.group(2).strip()

    parsed: dict[str, Any] = {}
    current_key: str | None = None
    for line in raw_yaml.split("\n"):
        line_stripped = line.strip()
        if not line_stripped:
            continue

        list_match = re.match(r"^\s*-\s+(.+)$", line_stripped)
        if list_match and current_key is not None:
            if not isinstance(parsed[current_key], list):
                parsed[current_key] = []
            parsed[current_key].append(list_match.group(1).strip())
            continue

        kv_match = re.match(r"^(\w+):\s*(.*)", line_stripped)
        if kv_match:
            current_key = kv_match.group(1)
            value = kv_match.group(2).strip()
            if value == "":
                parsed[current_key] = ""
            elif value.startswith('"') and value.endswith('"'):
                parsed[current_key] = value[1:-1]
            elif value.startswith("'") and value.endswith("'"):
                parsed[current_key] = value[1:-1]
            else:
                parsed[current_key] = value

    return parsed, body


def load_skill_from_directory(skill_dir: str | Path) -> Skill | None:
    skill_dir = Path(skill_dir)
    skill_md_path = skill_dir / "SKILL.md"
    if not skill_md_path.exists():
        return None

    content = skill_md_path.read_text(encoding="utf-8")
    frontmatter, body = _parse_yaml_frontmatter(content)

    name = frontmatter.get("name", skill_dir.name)
    description = frontmatter.get("description", "")

    agents_dir = skill_dir / "agents"
    agents: dict[str, str] = {}
    if agents_dir.exists():
        for agent_file in sorted(agents_dir.iterdir()):
            if agent_file.suffix in (".md", ".txt"):
                agents[agent_file.stem] = agent_file.read_text(encoding="utf-8")

    refs_dir = skill_dir / "references"
    references: dict[str, str] = {}
    if refs_dir.exists():
        for ref_file in sorted(refs_dir.iterdir()):
            if ref_file.suffix in (".md", ".txt", ".json", ".yaml", ".yml"):
                references[ref_file.stem] = ref_file.read_text(encoding="utf-8")

    scripts_dir = skill_dir / "scripts"
    scripts: list[Path] = []
    if scripts_dir.exists():
        for script_file in sorted(scripts_dir.iterdir()):
            if script_file.suffix == ".py" or (
                script_file.suffix == "" and script_file.stat().st_mode & 0o100
            ):
                scripts.append(script_file)

    assets_dir = skill_dir / "assets"
    if assets_dir.exists():
        for asset in assets_dir.iterdir():
            pass

    return Skill(
        name=name,
        description=description,
        system_prompt_template=body,
        path=skill_dir.resolve(),
        agents=agents,
        references=references,
        scripts=scripts,
        metadata={
            "frontmatter": frontmatter,
            "path": str(skill_dir.resolve()),
            "has_agents": bool(agents),
            "has_references": bool(references),
            "has_scripts": bool(scripts),
        },
    )


def discover_skill_directories(skills_root: str | Path) -> list[Path]:
    skills_root = Path(skills_root)
    if not skills_root.exists():
        return []

    result: list[Path] = []
    for entry in sorted(skills_root.iterdir()):
        if entry.is_dir():
            skill_md = entry / "SKILL.md"
            if skill_md.exists():
                result.append(entry)
    return result


class SkillRegistry:
    def __init__(self):
        self._skills: dict[str, Skill] = {}

    def register(self, skill: Skill) -> None:
        self._skills[skill.name] = skill

    def unregister(self, name: str) -> None:
        self._skills.pop(name, None)

    def get(self, name: str) -> Skill | None:
        return self._skills.get(name)

    def list_all(self) -> list[Skill]:
        return list(self._skills.values())

    def get_tools_for_skill(self, name: str) -> list[BaseTool]:
        skill = self._skills.get(name)
        if skill is None:
            return []
        return skill.get_tools()

    def get_or_create(self, name: str, description: str = "",
                      tools: list[BaseTool] | None = None,
                      system_prompt: str = "") -> Skill:
        if name in self._skills:
            return self._skills[name]
        skill = Skill(
            name=name, description=description,
            tools=tools or [], system_prompt_template=system_prompt,
        )
        self._skills[name] = skill
        return skill

    def load_skills_from_directory(self, skills_root: str | Path) -> list[Skill]:
        loaded: list[Skill] = []
        for skill_dir in discover_skill_directories(skills_root):
            skill = load_skill_from_directory(skill_dir)
            if skill is not None:
                if skill.name in self._skills:
                    existing = self._skills[skill.name]
                    skill.tools = existing.tools
                self.register(skill)
                loaded.append(skill)
        return loaded

    def to_prompt_block(self) -> str:
        skills = sorted(self.list_all(), key=lambda s: s.name)
        if not skills:
            return ""
        blocks = [s.to_prompt() for s in skills]
        return "\n\n".join(blocks)
