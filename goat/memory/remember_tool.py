from __future__ import annotations

import json
from typing import Type

from pydantic import BaseModel, Field
from langchain_core.tools import BaseTool

from . import MemoryManager


class RememberInput(BaseModel):
    action: str = Field(
        description="操作类型: 'read' 读取所有记忆, 'write' 写入一条记忆, 'clear' 清除记忆"
    )
    key: str = Field(
        default="",
        description="记忆的键名。write 和 clear(指定清除) 时必填"
    )
    content: str = Field(
        default="",
        description="记忆内容。write 时必填"
    )


class RememberTool(BaseTool):
    name: str = "remember"
    description: str = (
        "跨会话记忆管理工具。用于在对话之间持久保存和读取信息。\n"
        "读取: action='read' 返回所有保存的记忆\n"
        "写入: action='write', key='主题', content='要记住的内容'\n"
        "清除: action='clear', key='主题' 删除指定记忆；不指定 key 则清空全部记忆\n"
        "典型用法: 用户说'记住我喜欢简洁的代码'或'我之前说过什么偏好'时使用"
    )
    args_schema: Type[BaseModel] = RememberInput
    return_direct: bool = False

    def _run(self, action: str, key: str = "", content: str = "") -> str:
        mm = MemoryManager()

        if action == "read":
            all_memories = mm.get_all()
            if not all_memories:
                return "暂无跨会话记忆"
            lines = ["跨会话记忆:"]
            for k, v in all_memories.items():
                preview = v[:200] + ("..." if len(v) > 200 else "")
                lines.append(f"  - {k}: {preview}")
            return "\n".join(lines)

        elif action == "write":
            if not key:
                return "错误: write 操作需要提供 key"
            if not content:
                return "错误: write 操作需要提供 content"
            mm.set(key, content)
            return f"已记住: {key}"

        elif action == "clear":
            if key:
                deleted = mm.delete(key)
                return f"已删除记忆: {key}" if deleted else f"记忆不存在: {key}"
            else:
                count = mm.clear_all()
                return f"已清除所有记忆 (共 {count} 条)"

        else:
            return f"错误: 未知操作 '{action}'，支持 read/write/clear"

    async def _arun(self, action: str, key: str = "", content: str = "") -> str:
        return self._run(action, key, content)


REMEMBER_TOOL = RememberTool()