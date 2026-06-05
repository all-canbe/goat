"""CLI 输入处理 — 基于 prompt_toolkit 的终端输入封装。

提供:
- 多行输入 (Enter 提交, Esc+Enter/Alt+Enter 换行)
- 按目录持久化命令历史
- Ctrl+R 反向搜索
- 文件路径 Tab 补全
- 语法高亮提示符
- Vim 模式 (可选)
"""

from __future__ import annotations

import os
from pathlib import Path

from prompt_toolkit import PromptSession
from prompt_toolkit.auto_suggest import AutoSuggestFromHistory
from prompt_toolkit.completion import Completer, PathCompleter
from prompt_toolkit.history import FileHistory
from prompt_toolkit.key_binding import KeyBindings, KeyPressEvent
from prompt_toolkit.keys import Keys
from prompt_toolkit.lexers import SimpleLexer
from prompt_toolkit.styles import Style

from goat.core.workspace import get_goat_home

# ── 样式定义 ──
CLI_STYLE = Style.from_dict({
    "prompt.agent": "bold #00ff87",
    "prompt.command": "bold #ffaa00",
    "separator": "#666666",
})

# ── 键绑定 ──
def _create_key_bindings(vim_mode: bool = False) -> KeyBindings:
    """创建键绑定。

    Enter → 提交当前输入
    Esc+Enter / Alt+Enter → 插入换行（多行模式）
    """
    kb = KeyBindings()

    @kb.add("enter", eager=True)
    def _(event: KeyPressEvent) -> None:
        """Enter: 提交当前输入"""
        event.current_buffer.validate_and_handle()

    @kb.add("escape", "enter", eager=True)
    def _(event: KeyPressEvent) -> None:
        """Esc+Enter: 插入换行符"""
        event.current_buffer.insert_text("\n")

    @kb.add("c-c", eager=True)
    def _(event: KeyPressEvent) -> None:
        """Ctrl+C: 取消当前输入，清空 buffer"""
        event.current_buffer.reset()

    if vim_mode:
        @kb.add("escape", "escape", eager=True)
        def _(event):
            """Esc Esc: 退出 vim 模式的操作提示"""
            pass

    return kb


# ── 文件路径补全 ──
class FilePathCompleter(Completer):
    """文件路径 Tab 补全，仅对非命令输入生效。"""

    def __init__(self) -> None:
        self._path_completer = PathCompleter(
            only_directories=False,
            expanduser=True,
        )

    def get_completions(self, document, complete_event):
        text = document.text_before_cursor
        # 命令输入不补全文件路径
        if text.lstrip().startswith("/"):
            return
        # 查找最后一个 @ 后的路径进行补全
        at_idx = text.rfind("@")
        if at_idx >= 0:
            path_part = text[at_idx + 1:]
            from prompt_toolkit.document import Document
            path_doc = Document(path_part, len(path_part))
            yield from self._path_completer.get_completions(path_doc, complete_event)
        else:
            yield from self._path_completer.get_completions(document, complete_event)


# ── 提示符 Lexer（斜杠命令高亮） ──
_PROMPT_LEXER = SimpleLexer("class:prompt.agent")


# ── 主类 ──
class CLIInputHandler:
    """终端输入处理器，封装 prompt_toolkit 的异步 prompt。

    用法:
        handler = CLIInputHandler(workspace_dir="/path/to/project")
        text = await handler.aprompt(agent_mode=True)
    """

    def __init__(
        self,
        workspace_dir: str | None = None,
        *,
        history_dir: str | None = None,
        vim_mode: bool = False,
    ) -> None:
        self._vim_mode = vim_mode
        self._workspace = workspace_dir or os.getcwd()

        # 历史文件路径: ~/.goat/history/<workspace_hash>.txt
        if history_dir is None:
            history_dir = str(get_goat_home() / "history")
        Path(history_dir).mkdir(parents=True, exist_ok=True)
        self._history_file = str(
            Path(history_dir) / f"{abs(hash(self._workspace)) % 100000}.txt"
        )

        self._session: PromptSession | None = None
        self._completer = FilePathCompleter()

    @property
    def vim_mode(self) -> bool:
        return self._vim_mode

    @vim_mode.setter
    def vim_mode(self, value: bool) -> None:
        self._vim_mode = value
        self._session = None  # 重建 session

    def _get_session(self) -> PromptSession:
        if self._session is None:
            kb = _create_key_bindings(vim_mode=self._vim_mode)
            self._session = PromptSession(
                key_bindings=kb,
                history=FileHistory(self._history_file),
                auto_suggest=AutoSuggestFromHistory(),
                completer=self._completer,
                style=CLI_STYLE,
                vi_mode=self._vim_mode,
                complete_while_typing=False,
                multiline=True,
            )
        return self._session

    async def aprompt(self, agent_mode: bool = True) -> str:
        """异步获取用户输入。

        Args:
            agent_mode: True 显示 Agent 提示符，False 显示命令提示符。

        Returns:
            用户输入的文本（已 strip）。
        """
        session = self._get_session()

        if agent_mode:
            prompt_text = [
                ("class:prompt.agent", "🤖 > "),
            ]
        else:
            prompt_text = [
                ("class:prompt.command", "🔧 > "),
            ]

        try:
            text = await session.prompt_async(
                prompt_text,
                lexer=_PROMPT_LEXER if agent_mode else None,
            )
        except (EOFError, KeyboardInterrupt):
            raise

        return text.strip()

    def clear_history(self) -> None:
        """清空当前会话的历史记录。"""
        if self._session is not None:
            self._session.history.clear()