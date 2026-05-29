from __future__ import annotations

import os
import re
import subprocess
import tempfile
from typing import Type

from pydantic import BaseModel, Field
from langchain_core.tools import BaseTool


class ApplyPatchInput(BaseModel):
    patch: str = Field(description="unified diff 格式的补丁内容")
    target_file: str = Field(
        default="",
        description="补丁目标文件路径。留空则从补丁头（+++ 行）自动解析",
    )
    strip: int = Field(
        default=1,
        description="补丁路径前缀要去掉的层级数（相当于 patch -pN），默认 1",
    )


class ApplyPatchTool(BaseTool):
    name: str = "apply_patch"
    description: str = (
        "应用 unified diff 格式的补丁到文件。优先使用 git apply 执行。\n"
        "当补丁包含上下文匹配信息时会自动校验上下文是否匹配。\n"
        "典型用法: 生成代码变更后，用此工具将 diff 直接应用到目标文件。"
    )
    args_schema: Type[BaseModel] = ApplyPatchInput
    return_direct: bool = False

    def _run(self, patch: str, target_file: str = "", strip: int = 1) -> str:
        return self._apply(patch, target_file, strip)

    async def _arun(self, patch: str, target_file: str = "", strip: int = 1) -> str:
        return self._apply(patch, target_file, strip)

    def _apply(self, patch: str, target_file: str = "", strip: int = 1) -> str:
        patch = patch.strip("\n\r")

        result = self._try_git_apply(patch, strip)
        if result is not None:
            return result

        result = self._try_system_patch(patch, strip)
        if result is not None:
            return result

        return self._apply_python(patch, target_file, strip)

    def _try_git_apply(self, patch: str, strip: int) -> str | None:
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", suffix=".patch", delete=False, encoding="utf-8"
            ) as f:
                f.write(patch)
                patch_file = f.name

            result = subprocess.run(
                ["git", "apply", f"-p{strip}", patch_file],
                capture_output=True, text=True, timeout=30,
                errors="replace",
            )
            os.unlink(patch_file)
            if result.returncode == 0:
                return "补丁已通过 git apply 成功应用"
        except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
            pass
        except Exception:
            pass

        try:
            os.unlink(patch_file)
        except Exception:
            pass
        return None

    def _try_system_patch(self, patch: str, strip: int) -> str | None:
        try:
            with tempfile.NamedTemporaryFile(
                mode="w", suffix=".patch", delete=False, encoding="utf-8"
            ) as f:
                f.write(patch)
                patch_file = f.name

            result = subprocess.run(
                ["patch", f"-p{strip}", "-i", patch_file],
                capture_output=True, text=True, timeout=30,
                errors="replace",
            )
            os.unlink(patch_file)
            if result.returncode == 0:
                output = result.stdout.strip()
                return f"补丁已通过 patch 命令成功应用" + (f"\n{output}" if output else "")
        except (FileNotFoundError, subprocess.TimeoutExpired, OSError):
            pass
        except Exception:
            pass

        try:
            os.unlink(patch_file)
        except Exception:
            pass
        return None

    def _apply_python(self, patch: str, target_file: str, strip: int) -> str:
        target = target_file or self._parse_target(patch, strip)
        if not target:
            return "错误: 无法确定目标文件，请在 target_file 参数中指定"

        file_path = os.path.abspath(os.path.expanduser(target))
        if not os.path.isfile(file_path):
            return f"错误: 目标文件不存在: {file_path}"

        try:
            with open(file_path, "r", encoding="utf-8") as f:
                original = f.read()
        except Exception as e:
            return f"错误: 读取文件失败: {e}"

        new_content = self._patch_content(original, patch)
        if new_content is None:
            return "错误: 补丁应用失败，无法匹配上下文（hunk 上下文不匹配）"

        try:
            with open(file_path, "w", encoding="utf-8") as f:
                f.write(new_content)
        except Exception as e:
            return f"错误: 写入文件失败: {e}"

        return f"补丁已成功应用到 {os.path.basename(file_path)}"

    @staticmethod
    def _parse_target(patch: str, strip: int) -> str | None:
        for line in patch.splitlines():
            if line.startswith("+++ "):
                path = line[4:].strip()
                parts = path.replace("\\", "/").split("/")
                if strip > 0 and len(parts) > strip:
                    return "/".join(parts[strip:])
                return path
            if line.startswith("--- "):
                path = line[4:].strip()
                if path in ("/dev/null",):
                    return None
                parts = path.replace("\\", "/").split("/")
                if strip > 0 and len(parts) > strip:
                    return "/".join(parts[strip:])
                return path
        return None

    @staticmethod
    def _patch_content(original: str, patch: str) -> str | None:
        lines = original.splitlines(keepends=True)
        patch_lines = patch.splitlines(keepends=False)

        hunks = []
        current_hunk = None
        hunk_header_re = re.compile(r"^@@ -(\d+),?(\d*) \+(\d+),?(\d*) @@")

        for line in patch_lines:
            m = hunk_header_re.match(line)
            if m:
                if current_hunk:
                    hunks.append(current_hunk)
                old_s = int(m.group(1))
                old_c = int(m.group(2)) if m.group(2) else 1
                new_s = int(m.group(3))
                new_c = int(m.group(4)) if m.group(4) else 1
                current_hunk = {
                    "old_start": old_s,
                    "old_count": old_c,
                    "new_start": new_s,
                    "new_count": new_c,
                    "old_lines": [],
                    "new_lines": [],
                }
            elif current_hunk is not None:
                if line.startswith("+"):
                    current_hunk["new_lines"].append(line[1:])
                elif line.startswith("-"):
                    current_hunk["old_lines"].append(line[1:])
                elif line.startswith(" "):
                    ctx = line[1:]
                    current_hunk["old_lines"].append(ctx)
                    current_hunk["new_lines"].append(ctx)
        if current_hunk:
            hunks.append(current_hunk)

        if not hunks:
            return None

        for hunk in reversed(hunks):
            old_start = hunk["old_start"] - 1
            old_lines = hunk["old_lines"]
            new_lines = hunk["new_lines"]

            content_slice = lines[old_start:old_start + len(old_lines)]

            if len(content_slice) < len(old_lines):
                matched = False
                for offset in range(max(0, old_start - 10), min(len(lines) - len(old_lines), old_start + 10)):
                    if offset == old_start:
                        continue
                    candidate = lines[offset:offset + len(old_lines)]
                    if all(
                        a.rstrip("\n\r") == b.rstrip("\n\r")
                        for a, b in zip(candidate, old_lines)
                    ):
                        old_start = offset
                        content_slice = candidate
                        matched = True
                        break
                if not matched:
                    return None

            for expected, actual in zip(old_lines, content_slice):
                if expected.rstrip("\n\r") != actual.rstrip("\n\r"):
                    return None

            lines[old_start:old_start + len(old_lines)] = [
                l if l.endswith("\n") else l + "\n"
                for l in new_lines
            ]

        result = "".join(lines)
        if result == original:
            return None
        return result


APPLY_PATCH_TOOL = ApplyPatchTool()