from __future__ import annotations

import asyncio


class CancellationToken:
    """级联取消树 — Python asyncio 实现，等价于 tokio CancellationToken::child_token()"""

    def __init__(self, parent: CancellationToken | None = None):
        self._event = asyncio.Event()
        self._parent = parent
        self._children: list[CancellationToken] = []

    def cancel(self) -> None:
        self._event.set()
        for child in self._children:
            child.cancel()

    def is_cancelled(self) -> bool:
        if self._event.is_set():
            return True
        if self._parent is not None and self._parent.is_cancelled():
            self._event.set()
            return True
        return False

    async def wait(self) -> None:
        await self._event.wait()

    def child_token(self) -> CancellationToken:
        child = CancellationToken(parent=self)
        self._children.append(child)
        return child

    def detached(self) -> CancellationToken:
        return CancellationToken(parent=None)