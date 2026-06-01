from __future__ import annotations

import asyncio
import os
import shlex
import subprocess
import sys
from typing import Optional

from goat.security.sandbox import BaseSandbox, SandboxResult


def _decode_output_safe(raw: bytes) -> str:
    if not raw:
        return ""
    import locale
    import sys
    for enc in ("utf-8",):
        try:
            return raw.decode(enc)
        except (UnicodeDecodeError, LookupError):
            pass
    if sys.platform == "win32":
        preferred = locale.getpreferredencoding(False)
        for enc in (preferred, "gbk", "cp936"):
            if enc:
                try:
                    return raw.decode(enc)
                except (UnicodeDecodeError, LookupError):
                    continue
    for enc in ("latin-1",):
        try:
            return raw.decode(enc)
        except (UnicodeDecodeError, LookupError):
            pass
    return raw.decode("utf-8", errors="replace")


class WindowsJobSandbox(BaseSandbox):
    def __init__(self, workspace: str | None = None):
        super().__init__(workspace)
        self._proc: subprocess.Popen | None = None
        self._job_handle: int | None = None
        self._terminated = False
        self._create_job()

    def _create_job(self):
        try:
            import ctypes
            from ctypes import wintypes

            kernel32 = ctypes.windll.kernel32

            kernel32.CreateJobObjectW.restype = wintypes.HANDLE
            kernel32.AssignProcessToJobObject.restype = wintypes.BOOL
            kernel32.TerminateJobObject.restype = wintypes.BOOL
            kernel32.CloseHandle.restype = wintypes.BOOL

            JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS = 9
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 0x00000008
            JOB_OBJECT_LIMIT_JOB_MEMORY = 0x00000200
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000

            class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
                _fields_ = [
                    ("PerProcessUserTimeLimit", wintypes.LARGE_INTEGER),
                    ("PerJobUserTimeLimit", wintypes.LARGE_INTEGER),
                    ("LimitFlags", wintypes.DWORD),
                    ("MinimumWorkingSetSize", ctypes.c_size_t),
                    ("MaximumWorkingSetSize", ctypes.c_size_t),
                    ("ActiveProcessLimit", wintypes.DWORD),
                    ("Affinity", ctypes.c_size_t),
                    ("ChildProcessRestrictionFlags", wintypes.DWORD),
                ]

            class IO_COUNTERS(ctypes.Structure):
                _fields_ = [
                    ("ReadOperationCount", wintypes.ULARGE_INTEGER),
                    ("WriteOperationCount", wintypes.ULARGE_INTEGER),
                    ("OtherOperationCount", wintypes.ULARGE_INTEGER),
                    ("ReadTransferCount", wintypes.ULARGE_INTEGER),
                    ("WriteTransferCount", wintypes.ULARGE_INTEGER),
                    ("OtherTransferCount", wintypes.ULARGE_INTEGER),
                ]

            class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
                _fields_ = [
                    ("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION),
                    ("IoInfo", IO_COUNTERS),
                    ("ProcessMemoryLimit", ctypes.c_size_t),
                    ("JobMemoryLimit", ctypes.c_size_t),
                    ("PeakProcessMemoryUsed", ctypes.c_size_t),
                    ("PeakJobMemoryUsed", ctypes.c_size_t),
                ]

            job_name = f"goat_sandbox_{os.getpid()}"
            self._job_handle = kernel32.CreateJobObjectW(None, job_name)

            if self._job_handle:
                info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
                info.BasicLimitInformation.LimitFlags = (
                    JOB_OBJECT_LIMIT_ACTIVE_PROCESS |
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                )
                info.BasicLimitInformation.ActiveProcessLimit = 3
                info.JobMemoryLimit = 2 * 1024 * 1024 * 1024

                kernel32.SetInformationJobObject(
                    self._job_handle,
                    JOBOBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                    ctypes.byref(info),
                    ctypes.sizeof(info),
                )
        except Exception:
            self._job_handle = None

    async def run(
        self,
        command: str,
        working_dir: str = ".",
        timeout: int = 60,
        shell: bool = True,
    ) -> SandboxResult:
        wd = self._resolve_wd(working_dir)

        def _sync_run() -> SandboxResult:
            try:
                proc = subprocess.Popen(
                    command if shell else shlex.split(command),
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    cwd=wd,
                    env={**os.environ, "PAGER": "cat"},
                    shell=shell,
                )
            except FileNotFoundError as e:
                return SandboxResult(-1, "", f"命令未找? {e}", sandbox_type="windows_job")
            except Exception as e:
                return SandboxResult(-1, "", f"启动失败: {e}", sandbox_type="windows_job")

            self._proc = proc

            if self._job_handle and proc.pid:
                try:
                    import ctypes
                    kernel32 = ctypes.windll.kernel32
                    kernel32.AssignProcessToJobObject(self._job_handle, proc.pid)
                except Exception:
                    pass

            try:
                stdout_data, stderr_data = proc.communicate(timeout=timeout)
                return SandboxResult(
                    proc.returncode or 0,
                    _decode_output_safe(stdout_data),
                    _decode_output_safe(stderr_data),
                    sandbox_type="windows_job",
                )
            except subprocess.TimeoutExpired:
                proc.kill()
                stdout_data, stderr_data = proc.communicate()
                return SandboxResult(
                    -1,
                    _decode_output_safe(stdout_data),
                    "执行超时",
                    sandbox_type="windows_job", terminated=True,
                )

        loop = asyncio.get_event_loop()
        try:
            result = await loop.run_in_executor(None, _sync_run)
            return result
        except asyncio.CancelledError:
            self.terminate()
            return SandboxResult(-1, "", "执行被取消", sandbox_type="windows_job", terminated=True)

    def terminate(self) -> None:
        if self._terminated:
            return
        self._terminated = True
        if self._proc and self._proc.returncode is None:
            try:
                self._proc.kill()
            except ProcessLookupError:
                pass

        if self._job_handle:
            try:
                import ctypes
                kernel32 = ctypes.windll.kernel32
                kernel32.TerminateJobObject(self._job_handle, 1)
                kernel32.CloseHandle(self._job_handle)
            except Exception:
                pass
            self._job_handle = None

    def get_status(self) -> dict:
        return {
            "type": "windows_job",
            "enabled": self._job_handle is not None,
            "terminated": self._terminated,
            "pid": self._proc.pid if self._proc else None,
        }

    def _resolve_wd(self, path: str) -> str:
        expanded = os.path.expanduser(path)
        resolved = os.path.abspath(expanded)
        return resolved if os.path.isdir(resolved) else os.getcwd()

    def __del__(self):
        self.terminate()