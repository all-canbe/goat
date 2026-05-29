from __future__ import annotations

import asyncio
import os
import platform
import shlex
import sys

from goat.security.sandbox import BaseSandbox, SandboxResult


class MacOSSandbox(BaseSandbox):
    def __init__(self, workspace: str | None = None):
        super().__init__(workspace)
        self._proc: asyncio.subprocess.Process | None = None
        self._terminated = False
        self._has_sandbox_exec = self._check_sandbox_exec()

    def _check_sandbox_exec(self) -> bool:
        try:
            import subprocess
            result = subprocess.run(
                ["which", "sandbox-exec"],
                capture_output=True, text=True, timeout=5,
            )
            return result.returncode == 0
        except Exception:
            return False

    def _build_seatbelt_profile(self, working_dir: str) -> str:
        safe_dir = os.path.abspath(working_dir)
        lines = [
            "(version 1)",
            "(deny default)",
            "(allow file-read*)",
            f'(allow file-write* (subpath "{safe_dir}"))',
            "(allow network* (local ip *:*))",
            "(allow process* (literal \"/bin/*\") (literal \"/usr/bin/*\"))",
            "(allow sysctl*)",
            "(allow signal*)",
        ]
        return " ".join(lines)

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

        if self._has_sandbox_exec:
            profile = self._build_seatbelt_profile(wd)
            wrapped_cmd = f"sandbox-exec -p {shlex.quote(profile)} {command}"
        else:
            wrapped_cmd = command

        try:
            self._proc = await asyncio.wait_for(
                asyncio.create_subprocess_shell(
                    wrapped_cmd,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                    cwd=wd,
                    env={**os.environ, "PAGER": "cat"},
                ) if shell else asyncio.create_subprocess_exec(
                    *wrapped_cmd.split(),
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                    cwd=wd,
                    env={**os.environ, "PAGER": "cat"},
                ),
                timeout=timeout,
            )
        except asyncio.TimeoutError:
            return SandboxResult(-1, "", "进程启动超时", sandbox_type="macos")
        except FileNotFoundError as e:
            return SandboxResult(-1, "", f"命令未找�? {e}", sandbox_type="macos")
        except Exception as e:
            return SandboxResult(-1, "", f"启动失败: {e}", sandbox_type="macos")

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
                sandbox_type="macos",
            )
        except asyncio.TimeoutError:
            self.terminate()
            return SandboxResult(
                -1, "\n".join(stdout_lines), "执行超时",
                sandbox_type="macos", terminated=True,
            )
        except asyncio.CancelledError:
            self.terminate()
            return SandboxResult(
                -1, "\n".join(stdout_lines), "执行被取消",
                sandbox_type="macos", terminated=True,
            )

    def terminate(self) -> None:
        if self._terminated:
            return
        self._terminated = True
        if self._proc and self._proc.returncode is None:
            try:
                self._proc.terminate()
                try:
                    self._proc.kill()
                except ProcessLookupError:
                    pass
            except ProcessLookupError:
                pass

    def get_status(self) -> dict:
        return {
            "type": "macos",
            "sandbox_exec_available": self._has_sandbox_exec,
            "terminated": self._terminated,
            "pid": self._proc.pid if self._proc else None,
        }

    def _resolve_wd(self, path: str) -> str:
        expanded = os.path.expanduser(path)
        resolved = os.path.abspath(expanded)
        return resolved if os.path.isdir(resolved) else os.getcwd()

    def __del__(self):
        self.terminate()