"""find_skills 工具 — 按 find-skills SKILL.md 工作流封装为 BaseTool。

行为：抓取 https://skills.sh/?q={query} 解析候选 skill 列表；
       抓取失败时退回到 SKILL.md 已知优质源的关键词匹配。
输出：JSON 字符串 {query, count, skills: [{name, description, install_url, source}]}。
"""
from __future__ import annotations

import json
import re
from typing import Any
from urllib.parse import quote

import requests
from langchain_core.tools import tool
from pydantic import BaseModel, Field


class FindSkillsInput(BaseModel):
    query: str = Field(..., description="要搜索的 skill 关键词或能力描述（如「React 性能」「流程图」）。")
    limit: int = Field(5, description="返回结果数上限 (1-10)。", ge=1, le=10)


# SKILL.md Step 4 提及的优质官方源（owner/repo）
_OFFICIAL_SOURCES = {
    "vercel-labs/agent-skills",
    "anthropics/skills",
    "microsoft/skills",
    "ComposioHQ/awesome-claude-skills",
    "github/awesome-copilot",
}

# 关键词 -> 推荐源（兜底推荐表）
_KEYWORD_HINTS: list[tuple[set[str], str, str]] = [
    ({"react", "next", "nextjs", "前端", "frontend"}, "vercel-labs/agent-skills@react-best-practices",
     "React / Next.js 性能与最佳实践（来自 Vercel 工程团队，10K+ 安装）"),
    ({"design", "ui", "ux", "设计", "美化"}, "anthropics/skills@frontend-design",
     "前端设计 / UI 美化指南（来自 Anthropic，10K+ 安装）"),
    ({"test", "测试", "playwright", "e2e", "jest"}, "microsoft/skills@playwright",
     "Playwright E2E 测试集成（来自 Microsoft）"),
    ({"doc", "文档", "readme", "changelog", "api-docs"}, "ComposioHQ/awesome-claude-skills@doc-coauthoring",
     "技术文档协作写作工作流（来自 Anthropic）"),
    ({"review", "审查", "pr", "lint"}, "github/awesome-copilot@pull-request-review",
     "Pull Request 审查清单与提示词（来自 GitHub）"),
    ({"docker", "deploy", "部署", "ci", "ci-cd", "devops"}, "github/awesome-copilot@ci-cd-pipeline",
     "CI/CD 流水线设计指南（来自 GitHub）"),
    ({"flow", "diagram", "流程图", "mermaid"}, "anthropics/skills@diagram-generation",
     "基于 Mermaid 的图表与流程图生成（来自 Anthropic）"),
    ({"git", "branch", "commit", "版本"}, "github/awesome-copilot@git-workflow",
     "Git 工作流与提交规范（来自 GitHub）"),
    ({"python", "pytest", "pyright", "ruff"}, "microsoft/skills@python-best-practices",
     "Python 项目结构与测试最佳实践（来自 Microsoft）"),
    ({"typescript", "tsc", "type"}, "vercel-labs/agent-skills@typescript",
     "TypeScript 类型体操与项目配置（来自 Vercel）"),
]


def _fetch_skills_sh(query: str, limit: int) -> list[dict[str, str]]:
    """极简抓取 skills.sh 搜索结果。失败返回 []。"""
    try:
        url = f"https://skills.sh/?q={quote(query)}"
        resp = requests.get(url, timeout=10, headers={"User-Agent": "Mozilla/5.0"})
        if resp.status_code != 200:
            return []
        html_text = resp.text
    except (requests.RequestException, OSError):
        return []

    pattern = re.compile(
        r'<a[^>]+href="(/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@?[A-Za-z0-9_.-]*)"[^>]*>(.*?)</a>',
        re.DOTALL,
    )
    results: list[dict[str, str]] = []
    for m in pattern.finditer(html_text):
        slug = m.group(1).lstrip("/")
        if "@" not in slug:
            continue
        owner_repo, skill_name = slug.split("@", 1)
        # 保留官方源 + 包含查询词的
        repo_key = owner_repo.replace("-", "/", 0) if "/" in owner_repo else owner_repo
        if not (repo_key in _OFFICIAL_SOURCES or query.lower() in skill_name.lower()):
            continue
        raw_desc = re.sub(r"<[^>]+>", "", m.group(2)).strip()
        results.append({
            "name": skill_name,
            "description": raw_desc[:200] or f"来自 {owner_repo}",
            "install_url": f"npx skills add {slug} -g -y",
            "source": f"https://github.com/{owner_repo}",
        })
        if len(results) >= limit:
            break
    return results


def _fallback_recommendations(query: str, limit: int) -> list[dict[str, str]]:
    """关键词兜底：从 SKILL.md 已知的优质源映射。"""
    q_lower = query.lower()
    scored: list[tuple[int, dict[str, str]]] = []
    for keywords, slug, desc in _KEYWORD_HINTS:
        score = sum(1 for k in keywords if k in q_lower)
        if score == 0:
            continue
        owner_repo, skill_name = slug.split("@", 1)
        scored.append((score, {
            "name": skill_name,
            "description": desc,
            "install_url": f"npx skills add {slug} -g -y",
            "source": f"https://github.com/{owner_repo}",
        }))
    scored.sort(key=lambda x: x[0], reverse=True)
    return [item for _, item in scored[:limit]]


@tool("find_skills", args_schema=FindSkillsInput)
def find_skills_tool(query: str, limit: int = 5) -> str:
    """搜索可安装的 AI 辅助编程 skill。返回 JSON 字符串，含 name/description/install_url/source。
    用于「帮我找一个能 ... 的 skill」「how do I do X」「is there a skill for X」类问题。
    """
    results = _fetch_skills_sh(query, limit)
    if not results:
        results = _fallback_recommendations(query, limit)
    if not results:
        results = [{
            "name": "n/a",
            "description": f"未找到与「{query}」直接匹配的 skill。可在 https://skills.sh/ 手动搜索。",
            "install_url": "",
            "source": "https://skills.sh/",
        }]
    return json.dumps(
        {"query": query, "count": len(results), "skills": results},
        ensure_ascii=False,
    )


def find_skills_invoke(query: str, limit: int = 5) -> dict[str, Any]:
    """供 LangChain 工具调用入口使用的同步包装（非 tool 装饰器，方便后端直接调用）。"""
    raw = find_skills_tool.invoke({"query": query, "limit": limit})
    try:
        return json.loads(raw)
    except (json.JSONDecodeError, TypeError):
        return {"query": query, "count": 0, "skills": []}


FIND_SKILLS_TOOL = find_skills_tool
