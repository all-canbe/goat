from __future__ import annotations

from dataclasses import dataclass, field
from langchain_core.tools import BaseTool


@dataclass
class Skill:
    name: str
    description: str
    tools: list[BaseTool] = field(default_factory=list)
    system_prompt_template: str = ""
    metadata: dict = field(default_factory=dict)

    def to_prompt(self) -> str:
        return self.system_prompt_template

    def get_tools(self) -> list[BaseTool]:
        return self.tools


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