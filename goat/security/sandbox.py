from __future__ import annotations

import abc
import os
import sys
from dataclasses import dataclass
from typing import Optional


@dataclass
class SandboxResult:
    returncode: int
    stdout: str
    stderr: str
    sandbox_type: str = "none"
    terminated: bool = False


class BaseSandbox(abc.ABC):
    def __init__(self, workspace: str | None = None):
        self.workspace = workspace or os.getcwd()

    @abc.abstractmethod
    async def run(
        self,
        command: str,
        working_dir: str = ".",
        timeout: int = 60,
        shell: bool = True,
    ) -> SandboxResult: ...

    @abc.abstractmethod
    def terminate(self) -> None: ...

    @abc.abstractmethod
    def get_status(self) -> dict: ...


def create_sandbox(workspace: str | None = None) -> BaseSandbox:
    platform = sys.platform
    if platform == "win32":
        from goat.security.sandbox_windows import WindowsJobSandbox
        return WindowsJobSandbox(workspace)
    elif platform == "darwin":
        from goat.security.sandbox_macos import MacOSSandbox
        return MacOSSandbox(workspace)
    else:
        from goat.security.sandbox_linux import LinuxSandbox
        return LinuxSandbox(workspace)