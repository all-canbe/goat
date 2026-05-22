from __future__ import annotations

import difflib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path
from typing import Any, Optional, Type

import requests
from langchain_core.tools import BaseTool, tool
from pydantic import BaseModel, Field


# ============================================================================
# 辅助函数
# ============================================================================

def _safe_path(path: str) -> Path:
    return Path(path).expanduser().resolve()


def _read_file_content(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _write_file_content(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _build_diff(file_path: str, old_content: str, new_content: str, context: int = 3) -> str:
    old_lines = old_content.splitlines(keepends=True)
    new_lines = new_content.splitlines(keepends=True)
    diff = difflib.unified_diff(
        old_lines, new_lines,
        fromfile=f'a/{os.path.basename(file_path)}',
        tofile=f'b/{os.path.basename(file_path)}',
        n=context,
    )
    result = ''.join(diff)
    return result if result else "(no change)"


def _normalize_smart_quotes(text: str) -> str:
    replacements = {
        '\u201c': '"', '\u201d': '"',
        '\u2018': "'", '\u2019': "'",
        '\u2013': '-', '\u2014': '-',
        '\u00a0': ' ',
    }
    for old, new in replacements.items():
        text = text.replace(old, new)
    return text


# ============================================================================
# 命令安全：黑名单 + 逃逸检测 + 环境变量过滤 (严格模式)
# ============================================================================

# 严格模式黑名单：匹配即拒绝，YOLO 模式也不可绕过
COMMAND_BLACKLIST: list[tuple[re.Pattern, str]] = [
    (re.compile(r'\brm\s+(-rf?|--recursive)\s+[/~]'), "禁止递归删除根目录 / 或家目录 ~"),
    (re.compile(r'\bsudo\b'), "禁止使用 sudo（权限提升）"),
    (re.compile(r'\bdd\s+(if=|of=)'), "禁止直接块设备读写 (dd)"),
    (re.compile(r'\bchmod\s+(-R\s+)?777\b'), "禁止设置或递归设置 777 权限"),
    (re.compile(r'\bchown\b'), "禁止变更文件所有者"),
    (re.compile(r':\(\)\s*\{'), "禁止 fork 炸弹"),
    (re.compile(r'\bmkfs\b'), "禁止格式化文件系统"),
    (re.compile(r'\bfdisk\b'), "禁止分区操作"),
    (re.compile(r'\bformat\b'), "禁止格式化操作"),
    (re.compile(r'\bdiskpart\b'), "禁止磁盘分区操作"),
    (re.compile(r'\b(wget|curl)\s+.*[\|;]\s*(bash|sh|zsh|pwsh|powershell)\b'), "禁止远程脚本管道到 shell 执行"),
    (re.compile(r'\b(wget|curl)\s+.*-O\s+[/\\]'), "禁止下载文件到根目录"),
    (re.compile(r'>\s*[/\\]dev[/\\](sda|sdb|sdc|nvme|mmc)'), "禁止直接写入块设备文件"),
    (re.compile(r'\b(mount|umount)\b'), "禁止挂载/卸载文件系统"),
    (re.compile(r'\b(reboot|shutdown|halt|poweroff|init)\b'), "禁止系统关停/重启命令"),
    (re.compile(r'\bpasswd\b'), "禁止修改用户密码"),
    (re.compile(r'\bkill(?:all)?\s+-?9\b'), "禁止强制终止进程 (kill -9)"),
    (re.compile(r'\bpfexec\b'), "禁止权限提升 (pfexec)"),
    (re.compile(r'\bdoas\b'), "禁止权限提升 (doas)"),
]

# 严格模式白名单：非白名单命令在 Plan 模式下被阻止
PLAN_MODE_ALLOWED_COMMANDS: set[str] = {
    "ls", "cat", "head", "tail", "wc", "grep", "find", "echo",
    "pwd", "which", "whoami", "id", "date", "uname", "type", "dir",
    "more", "less", "tree", "print", "python", "node", "deno",
    "git", "rg", "grep", "findstr", "select-string",
}

# 敏感环境变量名包含的关键词（匹配即过滤）
SENSITIVE_ENV_PATTERNS: list[re.Pattern] = [
    re.compile(r'(?i)(secret|token|key|password|credential|auth|certificate|cert)'),
    re.compile(r'(?i)(api[_-]?key|access[_-]?key|secret[_-]?key|private[_-]?key)'),
]


def _check_command_blacklist(command: str) -> Optional[str]:
    """检查命令是否匹配黑名单。返回 None 表示安全，返回 str 为拒绝原因。"""
    for pattern, reason in COMMAND_BLACKLIST:
        if pattern.search(command):
            return (
                f"安全拒绝: {reason}\n"
                f"命令: {command[:200]}"
            )
    return None


def _detect_escape_attempt(command: str) -> Optional[str]:
    """检测命令中是否包含工作目录逃逸操作。"""
    import shlex
    cd_patterns = [
        (r'\bcd\s+[/\\]', "禁止 cd 到系统根目录"),
        (r'\bcd\s+~', "禁止 cd 到家目录（使用绝对路径或相对路径）"),
        (r'\bcd\s+\.\.(\\|/)\.\.', "禁止连续向上跳转目录 (cd ../..)"),
    ]
    for pattern, reason in cd_patterns:
        if re.search(pattern, command):
            return (
                f"安全拒绝: 工作目录逃逸 - {reason}\n"
                f"命令: {command[:200]}"
            )
    return None


def _sanitize_environment(env: dict[str, str] | None = None) -> dict[str, str]:
    """过滤环境变量中的敏感值，返回安全的环境变量副本。"""
    original = dict(os.environ if env is None else env)
    filtered = {}
    removed = []
    for key, value in original.items():
        is_sensitive = any(p.search(key) for p in SENSITIVE_ENV_PATTERNS)
        if is_sensitive:
            removed.append(key)
            filtered[key] = "***FILTERED***"
        else:
            filtered[key] = value
    filtered["PAGER"] = "cat"
    return filtered


def _try_split_command(command: str) -> Optional[list[str]]:
    """尝试将命令分割成参数列表（无 shell 元字符时可用）。

    如果命令包含 shell 元字符（|><$`;&(){}!），返回 None，
    否则返回分割后的参数列表。
    """
    shell_metachars = set('|><$`;&(){}!')
    cmd_stripped = command.strip()

    # 检查是否包含 shell 元字符
    for ch in cmd_stripped:
        if ch in shell_metachars:
            # 排除文件名中的括号（合法的 glob 字符）
            if ch in '()' and not any(
                keyword in cmd_stripped for keyword in
                ['$(', '$((', '()', '() {', 'if ', 'for ', 'while ', 'case ']
            ):
                continue
            return None

    return cmd_stripped.split()


# ============================================================================
# 1. 基础文件操作
# ============================================================================

@tool
def list_files(directory: str = ".", pattern: str = "*", recursive: bool = False) -> str:
    """列出目录中的文件和子目录。

    Args:
        directory: 要列出的目录路径，默认为当前目录
        pattern: glob 匹配模式，默认为 *
        recursive: 是否递归列出子目录，默认 False
    """
    p = _safe_path(directory)
    if not p.exists():
        return f"错误: 目录不存在: {directory}"
    if not p.is_dir():
        return f"错误: 不是目录: {directory}"

    results = []
    glob_pattern = "**/*" if recursive else pattern
    for item in sorted(p.glob(glob_pattern)):
        if not recursive and item.is_relative_to(p) and item.parent != p:
            continue
        if item.name.startswith('.'):
            continue
        prefix = "[D]" if item.is_dir() else "[F]"
        size = ""
        if item.is_file():
            try:
                size = f" ({item.stat().st_size} bytes)"
            except OSError:
                pass
        rel = item.relative_to(p)
        results.append(f"{prefix} {rel}{size}")

    if not results:
        return f"目录为空或没有匹配: {directory}/{pattern}"
    summary = f"--- {directory} ({'递归' if recursive else '仅当前层'}) 共 {len(results)} 项 ---\n"
    return summary + "\n".join(results)


@tool
def read_file(filepath: str, start_line: int = 1, end_line: int = -1) -> str:
    """读取文件内容。

    Args:
        filepath: 文件路径
        start_line: 起始行号 (1-based)，默认 1
        end_line: 结束行号 (1-based)，-1 表示读到末尾
    """
    p = _safe_path(filepath)
    if not p.exists():
        return f"错误: 文件不存在: {filepath}"
    if not p.is_file():
        return f"错误: 不是文件: {filepath}"

    try:
        content = _read_file_content(p)
    except UnicodeDecodeError:
        try:
            content = p.read_text(encoding="utf-16")
        except Exception:
            return f"错误: 无法解码文件: {filepath}"
    except Exception as e:
        return f"错误: 读取文件失败: {e}"

    lines = content.split("\n")
    total = len(lines)

    if end_line == -1:
        end_line = total
    start_line = max(1, start_line)
    end_line = min(total, end_line)

    selected = lines[start_line - 1: end_line]
    header = f"--- {filepath} (行 {start_line}-{end_line} / 共 {total} 行) ---"
    return header + "\n" + "\n".join(selected)


@tool
def write_file(filepath: str, content: str) -> str:
    """写入内容到文件（覆盖模式）。

    Args:
        filepath: 文件路径
        content: 要写入的内容
    """
    p = _safe_path(filepath)
    try:
        _write_file_content(p, content)
        return f"写入成功: {filepath} ({len(content)} 字符)"
    except Exception as e:
        return f"错误: 写入文件失败: {e}"


@tool
def delete_file(filepath: str, recursive: bool = False) -> str:
    """删除文件或目录。

    Args:
        filepath: 要删除的文件或目录路径
        recursive: 如果目标是目录，是否递归删除，默认 False
    """
    p = _safe_path(filepath)
    if not p.exists():
        return f"错误: 路径不存在: {filepath}"

    try:
        if p.is_dir():
            if recursive:
                shutil.rmtree(p)
                return f"已递归删除目录: {filepath}"
            else:
                return f"错误: '{filepath}' 是目录，请设置 recursive=True 来递归删除"
        else:
            p.unlink()
            return f"已删除文件: {filepath}"
    except Exception as e:
        return f"错误: 删除失败: {e}"


@tool
def move_file(source: str, destination: str) -> str:
    """移动或重命名文件/目录。

    Args:
        source: 源路径
        destination: 目标路径
    """
    src = _safe_path(source)
    dst = _safe_path(destination)

    if not src.exists():
        return f"错误: 源路径不存在: {source}"

    try:
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(src), str(dst))
        return f"已移动: {source} -> {destination}"
    except Exception as e:
        return f"错误: 移动失败: {e}"


@tool
def copy_file(source: str, destination: str) -> str:
    """复制文件或目录。

    Args:
        source: 源路径
        destination: 目标路径
    """
    src = _safe_path(source)
    dst = _safe_path(destination)

    if not src.exists():
        return f"错误: 源路径不存在: {source}"

    try:
        dst.parent.mkdir(parents=True, exist_ok=True)
        if src.is_dir():
            shutil.copytree(src, dst, dirs_exist_ok=True)
            return f"已复制目录: {source} -> {destination}"
        else:
            shutil.copy2(src, dst)
            return f"已复制文件: {source} -> {destination}"
    except Exception as e:
        return f"错误: 复制失败: {e}"


@tool
def get_file_info(filepath: str) -> str:
    """获取文件或目录的详细信息。

    Args:
        filepath: 文件或目录路径
    """
    p = _safe_path(filepath)
    if not p.exists():
        return f"错误: 路径不存在: {filepath}"

    try:
        stat_info = p.stat()
        info = {
            "路径": str(p),
            "类型": "目录" if p.is_dir() else "文件" if p.is_file() else "其他",
            "大小": f"{stat_info.st_size} bytes" if p.is_file() else "-",
            "创建时间": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(stat_info.st_ctime)),
            "修改时间": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(stat_info.st_mtime)),
            "访问时间": time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(stat_info.st_atime)),
            "权限": oct(stat.S_IMODE(stat_info.st_mode)),
        }
        if p.is_file():
            info["行数"] = len(p.read_text(encoding="utf-8", errors="ignore").splitlines())
        return json.dumps(info, ensure_ascii=False, indent=2)
    except Exception as e:
        return f"错误: 获取信息失败: {e}"


@tool
def glob_search(pattern: str, directory: str = ".", max_results: int = 200) -> str:
    """使用 glob 模式搜索文件。

    类似 Claude Code 的 Glob 工具和 Codex CLI 的 glob 功能。
    根据 pattern 搜索文件名，支持通配符。

    Args:
        pattern: glob 搜索模式，如 "**/*.py", "src/**/*.ts"
        directory: 搜索的根目录，默认为当前目录
        max_results: 最大返回结果数，默认 200
    """
    p = _safe_path(directory)
    if not p.exists():
        return f"错误: 目录不存在: {directory}"

    try:
        results = []
        for item in sorted(p.glob(pattern)):
            if item.is_file() or item.is_dir():
                results.append(str(item.relative_to(p)) if item.is_relative_to(p) else str(item))
                if len(results) >= max_results:
                    break

        if not results:
            return f"未找到匹配: {directory}/{pattern}"

        summary = f"--- 匹配结果 ({len(results)} 项, 显示前 {min(len(results), max_results)} 项) ---\n"
        return summary + "\n".join(results)
    except Exception as e:
        return f"错误: 搜索失败: {e}"


# ============================================================================
# 2. FileEditTool — SearchReplace 精确编辑
# ============================================================================

class FileEditInput(BaseModel):
    file_path: str = Field(
        description="要编辑的文件绝对路径"
    )
    old_string: str = Field(
        description="要查找并替换的原文。必须是文件中连续存在的文本块"
    )
    new_string: str = Field(
        description="替换后的新文本。留空则删除 old_string"
    )
    partial: bool = Field(
        default=False,
        description="如果为 True，old_string 只需是匹配内容的子串，不要求完全匹配"
    )
    fuzz: bool = Field(
        default=False,
        description="如果为 True，容忍前导空白差异和智能引号变体"
    )


class FileEditTool(BaseTool):
    """Search-and-Replace 文件编辑器。

    类似于 Claude Code 的 Edit 工具、DeepSeek TUI 的 edit_file 工具。
    通过内容而非行号精确定位代码，确保 old_string 唯一匹配。

    特性:
    - exact 模式: old_string 必须完全且唯一匹配
    - partial 模式: old_string 可作为子串匹配
    - fuzz 模式: 容忍缩进差异和智能引号
    - 自动生成 diff 输出
    - 多层安全校验
    """
    name: str = "file_edit"
    description: str = """通过内容搜索替换编辑文件 - 类似 Claude Code 的 Edit。

精确匹配 old_string 并替换为 new_string，适用于修改已有文件的特定部分。
old_string 必须在文件中唯一出现。创建新文件请用 write_file。

示例:
  - 修改函数: old_string="def old_name():" new_string="def new_name():"
  - 修复拼写: old_string="teh" new_string="the"
  - 删除一行: old_string="print('debug')\\n" new_string=""
"""
    args_schema: Type[BaseModel] = FileEditInput
    return_direct: bool = False

    @staticmethod
    def _check_safety(file_path: str, old_string: str, new_string: str) -> Optional[str]:
        if old_string == new_string and new_string:
            return "错误: old_string 和 new_string 完全相同，无需修改"

        protected_patterns = [r'\.git/', r'\.ssh/', r'node_modules/']
        for pattern in protected_patterns:
            if re.search(pattern, file_path):
                return f"错误: 不允许编辑受保护的路径: {file_path}"
        return None

    def _try_fuzzy_match(self, content: str, old_string: str) -> Optional[tuple[int, int, str]]:
        old_stripped = old_string.strip()
        if not old_stripped:
            return None

        idx = content.find(old_stripped)
        if idx != -1:
            start = idx - (len(old_string) - len(old_string.lstrip()))
            if start < 0:
                start = 0
            end = start + len(old_string)
            return start, end, content[start:end]

        norm_body = _normalize_smart_quotes(old_stripped)
        norm_content = _normalize_smart_quotes(content)
        idx = norm_content.find(norm_body)
        if idx != -1:
            start = idx - (len(old_string) - len(old_string.lstrip()))
            if start < 0:
                start = 0
            end = start + len(old_string)
            return start, end, content[max(0, start):end]

        return None

    def _run(self, file_path: str, old_string: str, new_string: str,
             partial: bool = False, fuzz: bool = False) -> str:
        file_path = os.path.abspath(os.path.expanduser(file_path))

        safety_error = self._check_safety(file_path, old_string, new_string)
        if safety_error:
            return safety_error

        if not os.path.isfile(file_path):
            return f"错误: 文件不存在: {file_path}"

        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                content = f.read()
        except UnicodeDecodeError:
            return f"错误: 文件 '{file_path}' 不是 UTF-8 编码"
        except Exception as e:
            return f"错误: 读取文件失败: {e}"

        old_string_to_match = old_string

        if partial:
            if old_string_to_match in content:
                new_content = content.replace(old_string_to_match, new_string, 1)
            elif fuzz:
                match = self._try_fuzzy_match(content, old_string_to_match)
                if match:
                    start, end, matched_text = match
                    new_content = content[:start] + new_string + content[end:]
                else:
                    return f"错误: 在 {file_path} 中未找到匹配文本（含模糊匹配）"
            else:
                return f"错误: 在 {file_path} 中未找到匹配文本"
        else:
            count = content.count(old_string_to_match)
            if count == 0:
                if fuzz:
                    match = self._try_fuzzy_match(content, old_string_to_match)
                    if match:
                        start, end, matched_text = match
                        new_content = content[:start] + new_string + content[end:]
                        diff = _build_diff(file_path, content, new_content)
                        return (
                            f"已编辑 {os.path.basename(file_path)} (模糊匹配)\n"
                            f"匹配: {repr(matched_text[:80])}\n{diff}"
                        )
                return f"错误: 在 {file_path} 中未找到匹配文本"

            if count > 1:
                return (
                    f"错误: old_string 在 {file_path} 中出现 {count} 次。"
                    f"请在 old_string 中包含更多上下文以唯一匹配"
                )

            new_content = content.replace(old_string_to_match, new_string, 1)

        try:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(new_content)
        except Exception as e:
            return f"错误: 写入文件失败: {e}"

        diff = _build_diff(file_path, content, new_content)
        lines_changed = len(content.splitlines()) - len(new_content.splitlines())
        line_word = "行" if abs(lines_changed) <= 1 else "行"
        direction = "删除" if lines_changed > 0 else "增加"

        return (
            f"已编辑 {os.path.basename(file_path)}: "
            f"{abs(lines_changed)} {line_word} {direction}\n{diff}"
        )

    async def _arun(self, file_path: str, old_string: str, new_string: str,
                    partial: bool = False, fuzz: bool = False) -> str:
        return self._run(file_path, old_string, new_string, partial, fuzz)


# ============================================================================
# 3. FileGrepTool — 文件内容搜索 (ripgrep / Python fallback)
# ============================================================================

class FileGrepInput(BaseModel):
    pattern: str = Field(
        description="要搜索的正则表达式模式"
    )
    path: str = Field(
        default=".",
        description="要搜索的目录或文件路径，默认为当前目录"
    )
    glob: Optional[str] = Field(
        default=None,
        description="文件过滤 glob 模式，如 '*.py', 'src/**/*.ts'"
    )
    output_mode: str = Field(
        default="content",
        description="输出模式: 'content' (显示匹配行+上下文), "
                    "'files_with_matches' (仅显示文件路径), "
                    "'count' (显示每个文件的匹配数)"
    )
    context_lines: int = Field(
        default=2,
        description="匹配行前后的上下文行数，仅 content 模式有效"
    )
    max_results: int = Field(
        default=100,
        description="最大返回结果数"
    )
    case_sensitive: bool = Field(
        default=True,
        description="是否区分大小写"
    )
    include_line_numbers: bool = Field(
        default=True,
        description="是否显示行号，仅 content 模式有效"
    )
    multiline: bool = Field(
        default=False,
        description="是否启用多行模式（. 匹配换行符）"
    )


class FileGrepTool(BaseTool):
    """文件内容搜索工具。

    类似 Claude Code 的 Grep、Codex CLI 的 grep_files、DeepSeek TUI 的 grep_search。
    优先使用 ripgrep（更快），回退到 Python re。
    支持多种输出模式、上下文行、glob 过滤。
    """
    name: str = "file_grep"
    description: str = """在文件内容中搜索文本模式 - 类似 ripgrep/grep。

用于查找代码定义、引用、TODO、错误信息等文本。
支持正则表达式、glob 过滤、多种输出格式。

示例:
  - 查找函数定义: pattern="def \\w+\\("
  - 查找 TODO: pattern="TODO|FIXME"
  - 统计匹配数: pattern="import" output_mode="count"
"""
    args_schema: Type[BaseModel] = FileGrepInput
    return_direct: bool = False

    def _run(self, pattern: str, path: str = ".", glob: Optional[str] = None,
             output_mode: str = "content", context_lines: int = 2,
             max_results: int = 100, case_sensitive: bool = True,
             include_line_numbers: bool = True, multiline: bool = False) -> str:
        if self._has_ripgrep():
            result = self._run_ripgrep(
                pattern, path, glob, output_mode,
                context_lines, max_results, case_sensitive,
                include_line_numbers, multiline,
            )
            if result is not None:
                return result

        return self._run_python_fallback(
            pattern, path, glob, output_mode,
            context_lines, max_results, case_sensitive,
            include_line_numbers,
        )

    @staticmethod
    def _has_ripgrep() -> bool:
        try:
            subprocess.run(["rg", "--version"], capture_output=True, timeout=5)
            return True
        except (FileNotFoundError, subprocess.TimeoutExpired):
            return False

    def _run_ripgrep(
        self, pattern: str, path: str, glob: Optional[str],
        output_mode: str, context_lines: int, max_results: int,
        case_sensitive: bool, include_line_numbers: bool,
        multiline: bool,
    ) -> Optional[str]:
        cmd = ["rg"]

        if output_mode == "files_with_matches":
            cmd.extend(["-l"])
        elif output_mode == "count":
            cmd.extend(["-c"])
        else:
            if context_lines > 0:
                cmd.extend(["-C", str(context_lines)])
            if include_line_numbers:
                cmd.append("-n")

        if not case_sensitive:
            cmd.append("-i")
        if multiline:
            cmd.append("-U")
            cmd.append("--multiline-dotall")
        if glob:
            cmd.extend(["-g", glob])
        if max_results:
            cmd.extend(["--max-count", str(max_results)])

        cmd.extend(["--", pattern, path])

        try:
            result = subprocess.run(
                cmd, capture_output=True, text=True, timeout=60,
                env={**os.environ, "PAGER": "cat"},
            )
        except subprocess.TimeoutExpired:
            return f"搜索超时 (60秒): {pattern}"
        except Exception:
            return None

        if result.returncode == 2:
            error_msg = result.stderr.strip()
            if "error" in error_msg.lower() or "invalid" in error_msg.lower():
                return f"rg 错误: {error_msg}"

        output = result.stdout
        if not output.strip():
            return f"未找到匹配: {pattern}"

        if output_mode == "content":
            lines = output.splitlines()
            if len(lines) > max_results:
                output = "\n".join(lines[:max_results])
                output += f"\n... (显示前 {max_results} 行，共 {len(lines)} 行)"

        return output

    def _run_python_fallback(
        self, pattern: str, path: str, glob: Optional[str],
        output_mode: str, context_lines: int, max_results: int,
        case_sensitive: bool, include_line_numbers: bool,
    ) -> str:
        try:
            flags = 0 if case_sensitive else re.IGNORECASE
            regex = re.compile(pattern, flags)
        except re.error as e:
            return f"无效的正则表达式: {e}"

        p = _safe_path(path)
        if p.is_file():
            files = [p]
        else:
            files = list(p.rglob(glob or "*"))

        matches = []
        match_count = 0

        for file_path in files:
            if not file_path.is_file():
                continue

            try:
                with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                    lines = f.readlines()
            except Exception:
                continue

            file_matches = []
            for i, line in enumerate(lines, 1):
                if regex.search(line.rstrip('\n\r')):
                    file_matches.append(i)
                    match_count += 1

            if not file_matches:
                continue

            if output_mode == "files_with_matches":
                matches.append(str(file_path))
            elif output_mode == "count":
                matches.append(f"{file_path}:{len(file_matches)}")
            else:
                for ln in file_matches[:max_results]:
                    start = max(0, ln - 1 - context_lines)
                    end = min(len(lines), ln + context_lines)
                    for ctx_ln in range(start, end):
                        prefix = f"{str(ctx_ln + 1).rjust(4)}│" if include_line_numbers else " "
                        marker = ">" if ctx_ln + 1 == ln else " "
                        matches.append(f"{file_path}:{prefix}{marker}{lines[ctx_ln].rstrip()}")
                    if ln != file_matches[-1]:
                        matches.append("--")

            if match_count >= max_results:
                break

        if not matches:
            return f"未找到匹配: {pattern}"

        result = "\n".join(matches[:max_results + context_lines * 2])
        if match_count > max_results:
            result += f"\n... (显示前 {max_results} 个匹配，共 {match_count} 个)"

        return result

    async def _arun(self, **kwargs) -> str:
        return self._run(**kwargs)


# ============================================================================
# 4. 代码搜索 (增强版)
# ============================================================================

@tool
def search_code(directory: str, query: str, file_pattern: str = "*",
                context_lines: int = 2, max_results: int = 50) -> str:
    """在代码文件中搜索指定内容。

    增强版: 支持 ripgrep（自动检测），Python 原生回退，更多上下文控制。

    Args:
        directory: 搜索目录
        query: 搜索关键词或正则表达式
        file_pattern: 文件名模式，默认为 *
        context_lines: 上下文行数，默认 2
        max_results: 最大结果数，默认 50
    """
    p = _safe_path(directory)
    if not p.exists():
        return f"错误: 目录不存在: {directory}"

    try:
        rg_result = subprocess.run(
            ["rg", "-n", "-C", str(context_lines), "--max-count", str(max_results),
             "--", query, str(p)],
            capture_output=True, text=True, timeout=30,
            env={**os.environ, "PAGER": "cat"},
        )
        if rg_result.returncode == 0 and rg_result.stdout.strip():
            output = rg_result.stdout
            lines = output.splitlines()
            if len(lines) > max_results:
                output = "\n".join(lines[:max_results])
                output += f"\n... (显示前 {max_results} 行，共 {len(lines)} 行)"
            return output
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    try:
        result = subprocess.run(
            ["grep", "-rn", "--include=" + file_pattern, query, str(p)],
            capture_output=True, text=True, timeout=30,
        )
        output = result.stdout.strip()
        if not output:
            return f"未找到匹配: {query}"
        lines = output.split("\n")
        if len(lines) > max_results:
            output = "\n".join(lines[:max_results])
            output += f"\n... (显示前 {max_results} 个匹配，共 {len(lines)} 个)"
        return output
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass

    try:
        flags = re.IGNORECASE
        regex = re.compile(query, flags)
    except re.error as e:
        return f"无效的正则表达式: {e}"

    search_path = _safe_path(directory)
    files = list(search_path.rglob(file_pattern)) if search_path.is_dir() else [search_path]

    matches = []
    match_count = 0

    for file_path in files:
        if not file_path.is_file():
            continue

        try:
            with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
                lines = f.readlines()
        except Exception:
            continue

        file_matches = []
        for i, line in enumerate(lines, 1):
            if regex.search(line.rstrip('\n\r')):
                file_matches.append(i)
                match_count += 1

        if not file_matches:
            continue

        for ln in file_matches[:max_results]:
            start = max(0, ln - 1 - context_lines)
            end = min(len(lines), ln + context_lines)
            for ctx_ln in range(start, end):
                prefix = f"{str(ctx_ln + 1).rjust(4)}│"
                marker = ">" if ctx_ln + 1 == ln else " "
                matches.append(f"{file_path}:{prefix}{marker}{lines[ctx_ln].rstrip()}")
            if ln != file_matches[-1]:
                matches.append("--")

        if match_count >= max_results:
            break

    if not matches:
        return f"未找到匹配: {query}"

    result = "\n".join(matches[:max_results + context_lines * 2])
    if match_count > max_results:
        result += f"\n... (显示前 {max_results} 个匹配，共 {match_count} 个)"

    return result


# ============================================================================
# 5. Shell 命令执行
# ============================================================================

@tool
def execute_command(command: str, working_dir: str = ".", timeout: int = 60) -> str:
    """执行 shell 命令并返回输出。

    安全特性：
    - 严格模式命令黑名单（YOLO 模式下也不可绕过）
    - 工作目录逃逸检测
    - 敏感环境变量自动过滤
    - shell 注入防护（优先 shell=False 无元字符执行）

    Args:
        command: 要执行的命令
        working_dir: 工作目录
        timeout: 超时时间（秒），默认 60
    """
    wd = _safe_path(working_dir)
    cmd_stripped = command.strip()
    if not cmd_stripped:
        return "错误: 命令为空"

    # 1. 命令黑名单检查（最高优先级，YOLO 模式也不可绕过）
    blacklist_error = _check_command_blacklist(command)
    if blacklist_error:
        return blacklist_error

    # 2. 工作目录逃逸检测
    escape_error = _detect_escape_attempt(command)
    if escape_error:
        return escape_error

    # 3. 环境变量过滤（移除敏感键值）
    safe_env = _sanitize_environment()

    # 4. 尝试 shell=False 执行（优先防注入）
    args = _try_split_command(command)
    if args is not None:
        use_shell = False
    else:
        use_shell = True

    try:
        result = subprocess.run(
            args if not use_shell else command,
            shell=use_shell,
            capture_output=True, text=True,
            timeout=timeout, cwd=str(wd),
            env=safe_env,
        )
        output = result.stdout.strip()
        if result.stderr.strip():
            output += "\n[stderr]\n" + result.stderr.strip()
        if not output:
            output = f"命令执行成功 (exit code: {result.returncode})"

        if len(output) > 50000:
            output = output[:50000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output
    except subprocess.TimeoutExpired:
        return f"错误: 命令执行超时 ({timeout}秒)"
    except Exception as e:
        return f"错误: 命令执行失败: {e}"


# ============================================================================
# 6. Web 工具
# ============================================================================

@tool
def web_search(query: str, max_results: int = 5) -> str:
    """搜索互联网获取最新信息。

    类似 Claude Code 的 WebSearch、Codex CLI 的 web_search。
    使用 DuckDuckGo 的免费搜索 API。

    Args:
        query: 搜索关键词
        max_results: 返回结果数，默认 5，最大 10
    """
    max_results = min(max_results, 10)
    try:
        url = "https://api.duckduckgo.com/"
        params = {
            "q": query,
            "format": "json",
            "no_html": "1",
            "skip_disambig": "1",
        }
        resp = requests.get(url, params=params, timeout=15)
        data = resp.json()

        results = []

        abstract = data.get("AbstractText", "")
        if abstract:
            source = data.get("AbstractSource", "")
            link = data.get("AbstractURL", "")
            results.append(f"[摘要] {abstract}")
            if source:
                results.append(f"  来源: {source} ({link})")

        related = data.get("RelatedTopics", [])
        for topic in related[:max_results]:
            if "Text" in topic:
                text = topic["Text"]
                first_url = topic.get("FirstURL", "")
                results.append(f"• {text}")
                if first_url:
                    results.append(f"  {first_url}")
            elif "Topics" in topic:
                for sub in topic["Topics"][:3]:
                    if "Text" in sub:
                        results.append(f"• {sub['Text']}")
                        if "FirstURL" in sub:
                            results.append(f"  {sub['FirstURL']}")

        if not results:
            fallback_url = "https://duckduckgo.com/html/"
            params = {"q": query}
            resp = requests.get(fallback_url, params=params, timeout=15)
            resp.encoding = 'utf-8'
            import html
            snippets = re.findall(
                r'<a[^>]+class="result__a"[^>]*>(.*?)</a>',
                resp.text, re.DOTALL
            )
            for s in snippets[:max_results]:
                results.append(f"• {html.unescape(re.sub(r'<[^>]+>', '', s)).strip()}")

        if not results:
            return f"未找到 '{query}' 的相关结果"

        return f"--- 搜索结果: {query} ---\n" + "\n".join(results)

    except ImportError:
        return "错误: 需要安装 requests 库 (pip install requests)"
    except Exception as e:
        return f"错误: 搜索失败: {e}"


@tool
def web_fetch(url: str, max_length: int = 10000) -> str:
    """获取网页内容并转换为 Markdown 格式。

    类似 Claude Code 的 WebFetch、Codex CLI 的 web_fetch。
    用于读取在线文档、API 响应等。

    Args:
        url: 网页 URL
        max_length: 最大返回字符数，默认 10000
    """
    try:
        headers = {
            "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
                          "(KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
        }
        resp = requests.get(url, headers=headers, timeout=30)
        resp.raise_for_status()
        resp.encoding = resp.apparent_encoding or 'utf-8'

        content = resp.text

        title_match = re.search(r'<title>(.*?)</title>', content, re.IGNORECASE | re.DOTALL)
        title = title_match.group(1).strip() if title_match else ""

        for tag in ['script', 'style', 'nav', 'footer', 'header', 'aside']:
            content = re.sub(
                rf'<{tag}[^>]*>.*?</{tag}>', '',
                content, flags=re.DOTALL | re.IGNORECASE
            )

        text = re.sub(r'<br\s*/?>', '\n', content)
        text = re.sub(r'</p>', '\n\n', text)
        text = re.sub(r'</(h[1-6]|li|tr|div)>', '\n', text)
        text = re.sub(r'<[^>]+>', '', text)
        text = re.sub(r'&nbsp;', ' ', text)
        text = re.sub(r'&amp;', '&', text)
        text = re.sub(r'&lt;', '<', text)
        text = re.sub(r'&gt;', '>', text)
        text = re.sub(r'&quot;', '"', text)
        text = re.sub(r'\n{3,}', '\n\n', text)
        text = '\n'.join(line.strip() for line in text.split('\n'))
        text = text.strip()

        if title:
            text = f"# {title}\n\n{text}"

        if len(text) > max_length:
            text = text[:max_length] + f"\n\n[内容截断，原文共 {len(text)} 字符]"

        return text if text else f"错误: 无法提取 '{url}' 的内容"

    except requests.exceptions.Timeout:
        return f"错误: 请求超时: {url}"
    except requests.exceptions.HTTPError as e:
        return f"错误: HTTP {e.response.status_code}: {url}"
    except ImportError:
        return "错误: 需要安装 requests 库 (pip install requests)"
    except Exception as e:
        return f"错误: 获取失败: {e}"


# ============================================================================
# 7. Git 工具
# ============================================================================

@tool
def git_status(path: str = "") -> str:
    """查看 Git 工作区状态（相当于 git status）。

    Args:
        path: 可选的子目录或文件路径，只查看指定范围的状态
    """
    try:
        cmd = ["git", "status", "--porcelain=v1", "-b"]
        if path:
            cmd.extend(["--", path])
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return f"git status 失败:\n{result.stderr.strip()}"
        return result.stdout.strip() or "工作区干净，无变动"
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH 中"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_diff(path: str = "", cached: bool = False, unified: int = 3) -> str:
    """查看 Git 工作区差异（相当于 git diff）。

    Args:
        path: 可选的子目录或文件路径
        cached: 是否查看已暂存的变更 (--cached)
        unified: 上下文行数，默认 3，范围 0-50
    """
    try:
        unified = max(0, min(unified, 50))
        cmd = ["git", "diff", "--no-color", "--no-ext-diff", f"--unified={unified}"]
        if cached:
            cmd.append("--cached")
        if path:
            cmd.extend(["--", path])
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        output = result.stdout.strip()
        if not output:
            return "没有差异"
        if len(output) > 20000:
            output = output[:20000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH 中"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_log(max_count: int = 20, path: str = "", author: str = "",
            since: str = "") -> str:
    """查看 Git 提交历史（相当于 git log）。

    Args:
        max_count: 最大返回提交数，默认 20，最大 200
        path: 可选的子目录或文件路径，只查看指定文件的变更历史
        author: 按作者筛选（如 "author:张三"）
        since: 起始时间（如 "2 weeks ago" 或 "2025-01-01"）
    """
    try:
        max_count = max(1, min(max_count, 200))
        cmd = [
            "git", "log", "--no-color",
            f"--max-count={max_count}",
            "--date=iso-strict",
            "--pretty=format:%H%nAuthor: %an <%ae>%nDate: %ad%nSubject: %s%n",
        ]
        if author:
            cmd.append(f"--author={author}")
        if since:
            cmd.append(f"--since={since}")
        if path:
            cmd.extend(["--", path])
        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return f"git log 失败:\n{result.stderr.strip()}"
        output = result.stdout.strip()
        if not output:
            return "没有提交记录"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH 中"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_commit(message: str, add_all: bool = True) -> str:
    """创建 Git 提交（先自动暂存变动，再提交）。

    Args:
        message: 提交信息
        add_all: 是否自动暂存所有变动（git add --all），默认 True
    """
    try:
        if add_all:
            add_result = subprocess.run(
                ["git", "add", "--all"],
                capture_output=True, text=True, timeout=30,
            )
            if add_result.returncode != 0:
                return f"git add 失败:\n{add_result.stderr.strip()}"

        commit_result = subprocess.run(
            ["git", "commit", "-m", message],
            capture_output=True, text=True, timeout=30,
        )
        output = commit_result.stdout.strip()
        if commit_result.returncode != 0:
            error = commit_result.stderr.strip()
            if "nothing to commit" in error:
                return "没有需要提交的变更"
            return f"git commit 失败:\n{error}"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH 中"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


# ============================================================================
# 工具注册表
# ============================================================================

FILE_EDIT_TOOL = FileEditTool()
FILE_GREP_TOOL = FileGrepTool()

BUILTIN_TOOLS = {
    "list_files": list_files,
    "read_file": read_file,
    "write_file": write_file,
    "delete_file": delete_file,
    "move_file": move_file,
    "copy_file": copy_file,
    "get_file_info": get_file_info,
    "glob_search": glob_search,
    "file_edit": FILE_EDIT_TOOL,
    "file_grep": FILE_GREP_TOOL,
    "search_code": search_code,
    "execute_command": execute_command,
    "web_search": web_search,
    "web_fetch": web_fetch,
    "git_status": git_status,
    "git_diff": git_diff,
    "git_log": git_log,
    "git_commit": git_commit,
}

try:
    from my_tui.memory.remember_tool import REMEMBER_TOOL
    BUILTIN_TOOLS["remember"] = REMEMBER_TOOL
except ImportError:
    pass


def get_all_tools() -> list:
    return list(BUILTIN_TOOLS.values())


def get_tools_by_names(names: list[str]) -> list:
    return [BUILTIN_TOOLS[n] for n in names if n in BUILTIN_TOOLS]