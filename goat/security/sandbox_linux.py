from __future__ import annotations

import asyncio
import os
import sys

from goat.security.sandbox import BaseSandbox, SandboxResult


class LinuxSandbox(BaseSandbox):
    def __init__(self, workspace: str | None = None):
        super().__init__(workspace)
        self._proc: asyncio.subprocess.Process | None = None
        self._terminated = False
        self._landlock_abi = self._detect_landlock()

    def _detect_landlock(self) -> int:
        try:
            with open("/proc/sys/kernel/landlock/abi", "r") as f:
                return int(f.read().strip())
        except (FileNotFoundError, ValueError, OSError):
            return 0

    async def run(
        self,
        command: str,
        working_dir: str = ".",
        timeout: int = 60,
        shell: bool = True,
    ) -> SandboxResult:
        wd = self._resolve_wd(working_dir)
        stdout_lines: list[str] = []
        stderr_lines: list[str] = []

        try:
            self._proc = await asyncio.wait_for(
                asyncio.create_subprocess_shell(
                    command,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                    cwd=wd,
                    env={**os.environ, "PAGER": "cat"},
                ) if shell else asyncio.create_subprocess_exec(
                    *command.split(),
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                    cwd=wd,
                    env={**os.environ, "PAGER": "cat"},
                ),
                timeout=timeout,
            )
        except asyncio.TimeoutError:
            return SandboxResult(-1, "", "进程启动超时", sandbox_type="linux")
        except FileNotFoundError as e:
            return SandboxResult(-1, "", f"命令未找�? {e}", sandbox_type="linux")
        except Exception as e:
            return SandboxResult(-1, "", f"启动失败: {e}", sandbox_type="linux")

        async def _read(stream: asyncio.StreamReader, lines: list[str]) -> None:
            while True:
                line_bytes = await stream.readline()
                if not line_bytes:
                    break
                line = line_bytes.decode("utf-8", errors="replace").rstrip("\r\n")
                if line:
                    lines.append(line)

        stdout_task = asyncio.create_task(_read(self._proc.stdout, stdout_lines))
        stderr_task = asyncio.create_task(_read(self._proc.stderr, stderr_lines))

        try:
            await asyncio.wait_for(
                asyncio.gather(stdout_task, stderr_task, self._proc.wait()),
                timeout=timeout,
            )
            stdout_text = "\n".join(stdout_lines)
            stderr_text = "\n".join(stderr_lines)
            returncode = self._proc.returncode or 0
            return SandboxResult(
                returncode, stdout_text, stderr_text,
                sandbox_type="linux",
            )
        except asyncio.TimeoutError:
            self.terminate()
            return SandboxResult(
                -1, "\n".join(stdout_lines), "执行超时",
                sandbox_type="linux", terminated=True,
            )
        except asyncio.CancelledError:
            self.terminate()
            return SandboxResult(
                -1, "\n".join(stdout_lines), "执行被取消",
                sandbox_type="linux", terminated=True,
            )

    def terminate(self) -> None:
        if self._terminated:
            return
        self._terminated = True
        if self._proc and self._proc.returncode is None:
            try:
                self._proc.terminate()
                try:
                    import signal
                    self._proc.send_signal(signal.SIGKILL)
                except (ProcessLookupError, AttributeError):
                    self._proc.kill()
            except ProcessLookupError:
                pass

    def get_status(self) -> dict:
        return {
            "type": "linux",
            "landlock_abi": self._landlock_abi,
            "landlock_enabled": self._landlock_abi >= 1,
            "terminated": self._terminated,
            "pid": self._proc.pid if self._proc else None,
        }

    def _resolve_wd(self, path: str) -> str:
        expanded = os.path.expanduser(path)
        resolved = os.path.abspath(expanded)
        return resolved if os.path.isdir(resolved) else os.getcwd()

    def __del__(self):
        self.terminate()