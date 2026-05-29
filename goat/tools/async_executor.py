from __future__ import annotations

import asyncio
import os
import subprocess
import sys
import time
from typing import Callable, Optional


async def execute_command_async(
    command: str,
    working_dir: str = ".",
    timeout: int = 60,
    *,
    on_stdout: Optional[Callable[[str], None]] = None,
    on_stderr: Optional[Callable[[str], None]] = None,
    shell: bool = True,
) -> tuple[int, str, str]:
    wd = _resolve_wd(working_dir)

    def _run():
        proc = subprocess.Popen(
            command if shell else command.split(),
            shell=shell,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=wd,
            env={**os.environ, "PAGER": "cat"},
        )
        try:
            stdout, stderr = proc.communicate(timeout=timeout)
            return (proc.returncode or 0, True, stdout, stderr)
        except subprocess.TimeoutExpired:
            _kill_proc_tree(proc)
            try:
                leftover, _ = proc.communicate(timeout=10)
                return (proc.returncode or -1, False, leftover or b"", b"")
            except subprocess.TimeoutExpired:
                _kill_proc_tree(proc)
                return (proc.returncode or -1, False, b"", b"")

    loop = asyncio.get_running_loop()
    returncode, completed, raw_stdout, raw_stderr = await loop.run_in_executor(
        None, _run
    )

    stdout_text = raw_stdout.decode("utf-8", errors="replace") if raw_stdout else ""
    stderr_text = raw_stderr.decode("utf-8", errors="replace") if raw_stderr else ""

    if on_stdout and stdout_text:
        for line in stdout_text.splitlines():
            stripped = line.strip("\r\n")
            if stripped:
                on_stdout(stripped)

    if on_stderr and stderr_text:
        for line in stderr_text.splitlines():
            stripped = line.strip("\r\n")
            if stripped:
                on_stderr(stripped)

    if not completed:
        return (-1, stdout_text, f"错误: 执行超时 ({timeout}秒)")

    return (returncode, stdout_text, stderr_text)


def _kill_proc_tree(proc: subprocess.Popen) -> None:
    if sys.platform == "win32":
        subprocess.run(
            ["taskkill", "/F", "/T", "/PID", str(proc.pid)],
            capture_output=True,
            timeout=5,
        )
    else:
        proc.terminate()
        time.sleep(0.1)
        if proc.poll() is None:
            proc.kill()


def _resolve_wd(path: str) -> str:
    expanded = os.path.expanduser(path)
    resolved = os.path.abspath(expanded)
    if not os.path.isdir(resolved):
        resolved = os.getcwd()
    return resolved