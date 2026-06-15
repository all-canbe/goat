from __future__ import annotations

import asyncio
import difflib
import json
import locale
import os
import re
import shlex
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

from .retry import retry_sync, RetryConfig

_exec_workspace: str | None = None
_plan_workspace: Path | None = None

def set_plan_workspace(workspace: Path) -> None:
    global _plan_workspace
    _plan_workspace = workspace

def get_plan_workspace() -> Path:
    global _plan_workspace
    if _plan_workspace is not None:
        return _plan_workspace
    return Path.cwd()

_RETRYABLE_NET = RetryConfig(
    max_retries=3,
    base_delay=1.0,
    max_delay=8.0,
    retryable_exceptions=(
        requests.exceptions.ConnectionError,
        requests.exceptions.Timeout,
        OSError,
    ),
)


# ============================================================================
# 辅助函数
# ============================================================================

def _safe_path(path: str) -> Path:
    p = Path(path).expanduser()
    if not p.is_absolute() and _exec_workspace is not None:
        return (Path(_exec_workspace) / p).resolve()
    return p.resolve()


def _detect_subprocess_encoding() -> str:
    if sys.platform == "win32":
        preferred = locale.getpreferredencoding(False)
        if preferred and preferred.lower() not in ("utf-8", "utf8"):
            return preferred
        try:
            import ctypes
            codepage = ctypes.windll.kernel32.GetConsoleOutputCP()
            if codepage and codepage != 65001:
                return f"cp{codepage}"
        except Exception:
            pass
        return "gbk"
    return "utf-8"


def _decode_output(raw: bytes) -> str:
    if not raw:
        return ""
    for enc in ("utf-8", _detect_subprocess_encoding(), "gbk", "latin-1"):
        try:
            return raw.decode(enc)
        except (UnicodeDecodeError, LookupError):
            continue
    return raw.decode("utf-8", errors="replace")


def _read_file_content(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _adapt_command_for_platform(command: str) -> str:
    if sys.platform != "win32":
        return command
    adapted = command
    replacements = [
        (r'\bmkdir\s+-p\s+', 'mkdir '),
        (r'\bls\s+', 'dir '),
        (r'\bls$', 'dir'),
        (r'\bcat\s+', 'type '),
        (r'\bcat$', 'type'),
        (r'\brm\s+', 'del '),
        (r'\brm$', 'del'),
        (r'\bcp\s+', 'copy '),
        (r'\bcp$', 'copy'),
        (r'\bmv\s+', 'move '),
        (r'\bmv$', 'move'),
        (r'\btouch\s+', 'type nul > '),
        (r'\bchmod\s+', 'icacls '),
        (r'\bwhich\s+', 'where '),
        (r'\bwhich$', 'where'),
        (r'\bgrep\s+', 'findstr '),
        (r'\bfind\s+-name\s+', 'dir /s /b '),
    ]
    for pattern, replacement in replacements:
        adapted = re.sub(pattern, replacement, adapted)
    return adapted


def _format_command_error(cmd: str, error: str, returncode: int | None = None) -> str:
    if sys.platform == "win32":
        hints = _get_windows_error_hints(cmd)
        if hints:
            return f"命令执行失败 (Windows, code={returncode or '?'}): {error}\n{hints}"
    return f"命令执行失败 (code={returncode or '?'}): {error}"


def _get_windows_error_hints(cmd: str) -> str:
    hints = []
    if cmd.startswith("mkdir ") and "-p" in cmd:
        hints.append("Windows ?mkdir 不支?-p 参数，改? " + cmd.replace("-p ", ""))
    if cmd.startswith("ls ") or cmd == "ls":
        hints.append("Windows 使用 dir 代替 ls")
    if cmd.startswith("cat ") or cmd == "cat":
        hints.append("Windows 使用 type 代替 cat")
    if cmd.startswith("touch "):
        hints.append("Windows 使用 'type nul > filename' 代替 touch")
    if cmd.startswith("rm ") or cmd == "rm":
        hints.append("Windows 使用 del 代替 rm")
    if cmd.startswith("grep "):
        hints.append("Windows 使用 findstr 代替 grep")
    if not hints:
        hints.append("请尝试使用 PowerShell 兼容的语法")
    return "  建议: " + "\n  建议: ".join(hints)


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
# 命令安全：黑名单 + 逃逸检?+ 环境变量过滤 (严格模式)
# ============================================================================

# 严格模式黑名单：匹配即拒绝，YOLO 模式也不可绕过
COMMAND_BLACKLIST: list[tuple[re.Pattern, str]] = [
    (re.compile(r'\brm\s+(-rf?|--recursive)\s+[/~]'), "禁止递归删除根目?/ 或家目录 ~"),
    (re.compile(r'\bsudo\b'), "禁止使用 sudo（权限提升）"),
    (re.compile(r'\bdd\s+(if=|of=)'), "禁止直接块设备读?(dd)"),
    (re.compile(r'\bchmod\s+(-R\s+)?777\b'), "禁止设置或递归设置 777 权限"),
    (re.compile(r'\bchown\b'), "禁止变更文件所有者"),
    (re.compile(r':\(\)\s*\{'), "禁止 fork 炸弹"),
    (re.compile(r'\bmkfs\b'), "禁止格式化文件系统"),
    (re.compile(r'\bfdisk\b'), "禁止分区操作"),
    (re.compile(r'\bformat\b'), "禁止格式化操作"),
    (re.compile(r'\bdiskpart\b'), "禁止磁盘分区操作"),
    (re.compile(r'\b(wget|curl)\s+.*[\|;]\s*(bash|sh|zsh|pwsh|powershell)\b'), "禁止远程脚本管道?shell 执行"),
    (re.compile(r'\b(wget|curl)\s+.*-O\s+[/\\]'), "禁止下载文件到根目录"),
    (re.compile(r'>\s*[/\\]dev[/\\](sda|sdb|sdc|nvme|mmc)'), "禁止直接写入块设备文件"),
    (re.compile(r'\b(mount|umount)\b'), "禁止挂载/卸载文件系统"),
    (re.compile(r'\b(reboot|shutdown|halt|poweroff|init)\b'), "禁止系统关停/重启命令"),
    (re.compile(r'\bpasswd\b'), "禁止修改用户密码"),
    (re.compile(r'\bkill(?:all)?\s+-?9\b'), "禁止强制终止进程 (kill -9)"),
    (re.compile(r'\bpfexec\b'), "禁止权限提升 (pfexec)"),
    (re.compile(r'\bdoas\b'), "禁止权限提升 (doas)"),
]

# 严格模式白名单：非白名单命令?Plan 模式下被阻止
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
                f"安全拒绝: 工作目录逃?- {reason}\n"
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
    如果命令包含 shell 元字符（|><$`;&(){}!），返回 None。
    否则返回分割后的参数列表。"""
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
    """列出目录中的文件和子目录?
    Args:
        directory: 要列出的目录路径，默认为当前目录
        pattern: glob 匹配模式，默认为 *
        recursive: 是否递归列出子目录，默认 False
    """
    p = _safe_path(directory)
    if not p.exists():
        return f"错误: 目录不存? {directory}"
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
        return f"目录为空或没有匹? {directory}/{pattern}"
    summary = f"--- {directory} ({'递归' if recursive else '仅当前层'}) ?{len(results)} ?---\n"
    return summary + "\n".join(results)


@tool
def read_file(filepath: str, start_line: int = 1, end_line: int = -1) -> str:
    """读取文件内容?
    Args:
        filepath: 文件路径
        start_line: 起始行号 (1-based)，默?1
        end_line: 结束行号 (1-based)?1 表示读到末尾
    """
    p = _safe_path(filepath)
    if not p.exists():
        return f"错误: 文件不存? {filepath}"
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
    header = f"--- {filepath} (?{start_line}-{end_line} / ?{total} ? ---"
    return header + "\n" + "\n".join(selected)


class WriteFileInput(BaseModel):
    filepath: str = Field(description="文件路径")
    content: str = Field(description="要写入的内容")
    reason: str = Field(default="", description="解释为什么要做此操作（可选）")


class WriteFileTool(BaseTool):
    name: str = "write_file"
    description: str = "写入内容到文件（覆盖模式）"
    args_schema: Type[BaseModel] = WriteFileInput
    return_direct: bool = False

    def _run(self, filepath: str, content: str, reason: str = "") -> str:
        p = _safe_path(filepath)
        try:
            _write_file_content(p, content)
            return f"写入成功: {filepath} ({len(content)} 字符)"
        except Exception as e:
            return f"错误: 写入文件失败: {e}"

    async def _arun(self, filepath: str, content: str, reason: str = "") -> str:
        return self._run(filepath, content, reason)


class DeleteFileInput(BaseModel):
    filepath: str = Field(description="要删除的文件或目录路径")
    recursive: bool = Field(default=False, description="如果目标是目录，是否递归删除")
    reason: str = Field(default="", description="解释为什么要做此操作（可选）")


class DeleteFileTool(BaseTool):
    name: str = "delete_file"
    description: str = "删除文件或目录"
    args_schema: Type[BaseModel] = DeleteFileInput
    return_direct: bool = False

    def _run(self, filepath: str, recursive: bool = False, reason: str = "") -> str:
        p = _safe_path(filepath)
        if not p.exists():
            return f"错误: 路径不存? {filepath}"
        try:
            if p.is_dir():
                if recursive:
                    shutil.rmtree(p)
                    return f"已递归删除目录: {filepath}"
                else:
                    return f"错误: '{filepath}' 是目录，请设?recursive=True 来递归删除"
            else:
                p.unlink()
                return f"已删除文? {filepath}"
        except Exception as e:
            return f"错误: 删除失败: {e}"

    async def _arun(self, filepath: str, recursive: bool = False, reason: str = "") -> str:
        return self._run(filepath, recursive, reason)


@tool
def move_file(source: str, destination: str) -> str:
    """移动或重命名文件/目录?
    Args:
        source: 源路?        destination: 目标路径
    """
    src = _safe_path(source)
    dst = _safe_path(destination)

    if not src.exists():
        return f"错误: 源路径不存在: {source}"

    try:
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(src), str(dst))
        return f"已移? {source} -> {destination}"
    except Exception as e:
        return f"错误: 移动失败: {e}"


@tool
def copy_file(source: str, destination: str) -> str:
    """复制文件或目录?
    Args:
        source: 源路?        destination: 目标路径
    """
    src = _safe_path(source)
    dst = _safe_path(destination)

    if not src.exists():
        return f"错误: 源路径不存在: {source}"

    try:
        dst.parent.mkdir(parents=True, exist_ok=True)
        if src.is_dir():
            shutil.copytree(src, dst, dirs_exist_ok=True)
            return f"已复制目? {source} -> {destination}"
        else:
            shutil.copy2(src, dst)
            return f"已复制文? {source} -> {destination}"
    except Exception as e:
        return f"错误: 复制失败: {e}"


@tool
def save_plan_doc(filename: str, content: str) -> str:
    """保存计划/分析文档到 .goat/doc/ 目录（Plan 模式专用）。
    该工具只能写入 .goat/doc/ 目录，无法写入项目其他位置。
    Plan 模式下可用，用于持久化规划产出，方便跨模式/跨对话复用。
    Args:
        filename: 文件名，建议使用有意义的名称，如 "project-analysis" 或 "plan-xxx.md"
        content: 文档内容（markdown 格式）
    """
    safe_name = Path(filename).name
    if not safe_name.endswith('.md'):
        safe_name += '.md'

    doc_dir = get_plan_workspace() / ".goat" / "doc"
    doc_dir.mkdir(parents=True, exist_ok=True)

    target = (doc_dir / safe_name).resolve()
    doc_dir_resolved = doc_dir.resolve()
    if not str(target).startswith(str(doc_dir_resolved)):
        return f"错误: 文件名 '{filename}' 包含非法路径遍历，拒绝写入"

    try:
        target.write_text(content, encoding="utf-8")
        lines = content.count("\n") + 1
        return (
            f"文档已保存: .goat/doc/{safe_name}\n"
            f"  大小: {len(content)} 字符, {lines} 行\n"
            f"  提示: 切换到执行模式后，AI 会自动发现此文件"
        )
    except Exception as e:
        return f"错误: 保存文档失败: {e}"


@tool
def get_file_info(filepath: str) -> str:
    """获取文件或目录的详细信息?
    Args:
        filepath: 文件或目录路?    """
    p = _safe_path(filepath)
    if not p.exists():
        return f"错误: 路径不存? {filepath}"

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
    """使用 glob 模式搜索文件?
    类似 Claude Code ?Glob 工具?Codex CLI ?glob 功能?    根据 pattern 搜索文件名，支持通配符?
    Args:
        pattern: glob 搜索模式，如 "**/*.py", "src/**/*.ts"
        directory: 搜索的根目录，默认为当前目录
        max_results: 最大返回结果数，默?200
    """
    p = _safe_path(directory)
    if not p.exists():
        return f"错误: 目录不存? {directory}"

    try:
        results = []
        for item in sorted(p.glob(pattern)):
            if item.is_file() or item.is_dir():
                results.append(str(item.relative_to(p)) if item.is_relative_to(p) else str(item))
                if len(results) >= max_results:
                    break

        if not results:
            return f"未找到匹? {directory}/{pattern}"

        summary = f"--- 匹配结果 ({len(results)} ? 显示?{min(len(results), max_results)} ? ---\n"
        return summary + "\n".join(results)
    except Exception as e:
        return f"错误: 搜索失败: {e}"


# ============================================================================
# 2. FileEditTool ?SearchReplace 精确编辑
# ============================================================================

class FileEditInput(BaseModel):
    file_path: str = Field(
        description="要编辑的文件绝对路径"
    )
    old_string: str = Field(
        description="要查找并替换的原文。必须是文件中连续存在的文本。"
    )
    new_string: str = Field(
        description="替换后的新文本。留空则删除 old_string"
    )
    reason: str = Field(
        default="",
        description="解释为什么要做此操作（可选）"
    )
    partial: bool = Field(
        default=False,
        description="如果?True，old_string 只需是匹配内容的子串，不要求完全匹配"
    )
    fuzz: bool = Field(
        default=False,
        description="如果?True，容忍前导空白差异和智能引号变体"
    )


class FileEditTool(BaseTool):
    """Search-and-Replace 文件编辑器?
    类似?Claude Code ?Edit 工具、DeepSeek TUI ?edit_file 工具?    通过内容而非行号精确定位代码，确?old_string 唯一匹配?
    特?
    - exact 模式: old_string 必须完全且唯一匹配
    - partial 模式: old_string 可作为子串匹?    - fuzz 模式: 容忍缩进差异和智能引?    - 自动生成 diff 输出
    - 多层安全校验
    """
    name: str = "file_edit"
    description: str = """通过内容搜索替换编辑文件 - 类似 Claude Code ?Edit?
精确匹配 old_string 并替换为 new_string，适用于修改已有文件的特定部分?old_string 必须在文件中唯一出现。创建新文件请用 write_file?
示例:
  - 修改函数: old_string="def old_name():" new_string="def new_name():"
  - 修复拼写: old_string="teh" new_string="the"
  - 删除一? old_string="print('debug')\\n" new_string=""
"""
    args_schema: Type[BaseModel] = FileEditInput
    return_direct: bool = False

    @staticmethod
    def _check_safety(file_path: str, old_string: str, new_string: str) -> Optional[str]:
        if old_string == new_string and new_string:
            return "错误: old_string ?new_string 完全相同，无需修改"

        protected_patterns = [r'\.git/', r'\.ssh/', r'node_modules/']
        for pattern in protected_patterns:
            if re.search(pattern, file_path):
                return f"错误: 不允许编辑受保护的路? {file_path}"
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
             partial: bool = False, fuzz: bool = False, reason: str = "") -> str:
        file_path = os.path.abspath(os.path.expanduser(file_path))

        safety_error = self._check_safety(file_path, old_string, new_string)
        if safety_error:
            return safety_error

        if not os.path.isfile(file_path):
            return f"错误: 文件不存? {file_path}"

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
                return f"错误: ?{file_path} 中未找到匹配文本"
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
                            f"已编?{os.path.basename(file_path)} (模糊匹配)\n"
                            f"匹配: {repr(matched_text[:80])}\n{diff}"
                        )
                return f"错误: ?{file_path} 中未找到匹配文本"

            if count > 1:
                return (
                    f"错误: old_string 在 {file_path} 中出现 {count} 次",
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
            f"已编?{os.path.basename(file_path)}: "
            f"{abs(lines_changed)} {line_word} {direction}\n{diff}"
        )

    async def _arun(self, file_path: str, old_string: str, new_string: str,
                    partial: bool = False, fuzz: bool = False, reason: str = "") -> str:
        return self._run(file_path, old_string, new_string, partial, fuzz, reason=reason)


# ============================================================================
# 3. FileGrepTool ?文件内容搜索 (ripgrep / Python fallback)
# ============================================================================

class FileGrepInput(BaseModel):
    pattern: str = Field(
        description="要搜索的正则表达式模式"
    )
    path: str = Field(
        default=".",
        description="要搜索的目录或文件路径，默认为当前目?"
    )
    glob: Optional[str] = Field(
        default=None,
        description="文件过滤 glob 模式，如 '*.py', 'src/**/*.ts'"
    )
    output_mode: str = Field(
        default="content",
        description="输出模式: 'content' (显示匹配?上下?, "
                    "'files_with_matches' (仅显示文件路?, "
                    "'count' (显示每个文件的匹配数)"
    )
    context_lines: int = Field(
        default=2,
        description="匹配行前后的上下文行数，?content 模式有效"
    )
    max_results: int = Field(
        default=100,
        description="最大返回结果数"
    )
    case_sensitive: bool = Field(
        default=True,
        description="是否区分大小?"
    )
    include_line_numbers: bool = Field(
        default=True,
        description="是否显示行号，仅 content 模式有效"
    )
    multiline: bool = Field(
        default=False,
        description="是否启用多行模式? 匹配换行符）"
    )


class FileGrepTool(BaseTool):
    """文件内容搜索工具?
    类似 Claude Code ?Grep、Codex CLI ?grep_files、DeepSeek TUI ?grep_search?    优先使用 ripgrep（更快），回退?Python re?    支持多种输出模式、上下文行、glob 过滤?    """
    name: str = "file_grep"
    description: str = """在文件内容中搜索文本模式 - 类似 ripgrep/grep?
用于查找代码定义、引用、TODO、错误信息等文本?支持正则表达式、glob 过滤、多种输出格式?
示例:
  - 查找函数定义: pattern="def \\w+\\("
  - 查找 TODO: pattern="TODO|FIXME"
  - 统计匹配? pattern="import" output_mode="count"
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
            return f"搜索超时 (60?: {pattern}"
        except Exception:
            return None

        if result.returncode == 2:
            error_msg = result.stderr.strip()
            if "error" in error_msg.lower() or "invalid" in error_msg.lower():
                return f"rg 错误: {error_msg}"

        output = result.stdout
        if not output.strip():
            return f"未找到匹? {pattern}"

        if output_mode == "content":
            lines = output.splitlines()
            if len(lines) > max_results:
                output = "\n".join(lines[:max_results])
                output += f"\n... (显示?{max_results} 行，?{len(lines)} ?"

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
                        prefix = f"{str(ctx_ln + 1).rjust(4)}? if include_line_numbers else " ""
                        marker = ">" if ctx_ln + 1 == ln else " "
                        matches.append(f"{file_path}:{prefix}{marker}{lines[ctx_ln].rstrip()}")
                    if ln != file_matches[-1]:
                        matches.append("--")

            if match_count >= max_results:
                break

        if not matches:
            return f"未找到匹? {pattern}"

        result = "\n".join(matches[:max_results + context_lines * 2])
        if match_count > max_results:
            result += f"\n... (显示?{max_results} 个匹配，?{match_count} ?"

        return result

    async def _arun(self, **kwargs) -> str:
        return self._run(**kwargs)


# ============================================================================
# 4. 代码搜索 (增强?
# ============================================================================

@tool
def search_code(directory: str, query: str, file_pattern: str = "*",
                context_lines: int = 2, max_results: int = 50) -> str:
    """在代码文件中搜索指定内容?
    增强? 支持 ripgrep（自动检测），Python 原生回退，更多上下文控制?
    Args:
        directory: 搜索目录
        query: 搜索关键词或正则表达?        file_pattern: 文件名模式，默认?*
        context_lines: 上下文行数，默认 2
        max_results: 最大结果数，默?50
    """
    p = _safe_path(directory)
    if not p.exists():
        return f"错误: 目录不存? {directory}"

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
                output += f"\n... (显示?{max_results} 行，?{len(lines)} ?"
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
            return f"未找到匹? {query}"
        lines = output.split("\n")
        if len(lines) > max_results:
            output = "\n".join(lines[:max_results])
            output += f"\n... (显示?{max_results} 个匹配，?{len(lines)} ?"
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
                prefix = f"{str(ctx_ln + 1).rjust(4)}?"
                marker = ">" if ctx_ln + 1 == ln else " "
                matches.append(f"{file_path}:{prefix}{marker}{lines[ctx_ln].rstrip()}")
            if ln != file_matches[-1]:
                matches.append("--")

        if match_count >= max_results:
            break

    if not matches:
        return f"未找到匹? {query}"

    result = "\n".join(matches[:max_results + context_lines * 2])
    if match_count > max_results:
        result += f"\n... (显示?{max_results} 个匹配，?{match_count} ?"

    return result


# ============================================================================
# 5. Shell 命令执行
# ============================================================================

class ExecuteCommandInput(BaseModel):
    command: str = Field(description="要执行的命令")
    working_dir: str = Field(default=".", description="工作目录")
    timeout: int = Field(default=60, description="超时时间（秒）")
    reason: str = Field(default="", description="解释为什么要做此操作（可选）")


class ExecuteCommandTool(BaseTool):
    """执行 shell 命令并返回输出?
    安全特性：
    - 严格模式命令黑名单（YOLO 模式下也不可绕过?
    - 工作目录逃逸检?
    - 敏感环境变量自动过滤
    - shell 注入防护（优?shell=False 无元字符执行?
    """
    name: str = "execute_command"
    description: str = """执行 shell 命令并返回输出?
安全特性：
- 严格模式命令黑名单（YOLO 模式下也不可绕过?
- 工作目录逃逸检?
- 敏感环境变量自动过滤
- shell 注入防护（优?shell=False 无元字符执行?
Args:
    command: 要执行的命令
    working_dir: 工作目录
    timeout: 超时时间（秒），默认 60
"""
    args_schema: Type[BaseModel] = ExecuteCommandInput
    return_direct: bool = False

    def _run(self, command: str, working_dir: str = ".", timeout: int = 60, reason: str = "") -> str:
        wd = _safe_path(working_dir)
        cmd_stripped = command.strip()
        if not cmd_stripped:
            return "错误: 命令为空"
        cmd_stripped = _adapt_command_for_platform(cmd_stripped)

        blacklist_error = _check_command_blacklist(cmd_stripped)
        if blacklist_error:
            return blacklist_error

        escape_error = _detect_escape_attempt(cmd_stripped)
        if escape_error:
            return escape_error

        safe_env = _sanitize_environment()

        args = _try_split_command(cmd_stripped)
        if args is not None:
            use_shell = False
        else:
            use_shell = True

        try:
            result = subprocess.run(
                args if not use_shell else cmd_stripped,
                shell=use_shell,
                capture_output=True,
                timeout=timeout, cwd=str(wd),
                env=safe_env,
            )
            output = _decode_output(result.stdout).strip()
            stderr_text = _decode_output(result.stderr).strip()
            if stderr_text:
                output += "\n[stderr]\n" + stderr_text
            if not output:
                output = f"命令执行成功 (exit code: {result.returncode})"

            if len(output) > 50000:
                output = output[:50000] + f"\n\n[输出截断，共 {len(output)} 字符]"
            return output
        except subprocess.TimeoutExpired:
            return f"错误: 命令执行超时 ({timeout}?"
        except Exception as e:
            return _format_command_error(cmd_stripped, str(e))

    async def _arun(self, command: str, working_dir: str = ".", timeout: int = 60, reason: str = "") -> str:
        return self._run(command, working_dir, timeout, reason=reason)


@tool
async def async_execute_command(command: str, working_dir: str = ".", timeout: int = 60) -> str:
    """执行 shell 命令并实时流式返回输出（异步版本）?
    ?execute_command 同等的安全检查，但使?asyncio 异步执行?    命令输出会通过 EventBus 流式推送，不会阻塞事件循环?
    Args:
        command: 要执行的命令
        working_dir: 工作目录
        timeout: 超时时间（秒），默认 60
    """
    wd = _safe_path(working_dir)
    cmd_stripped = command.strip()
    if not cmd_stripped:
        return "错误: 命令为空"
    cmd_stripped = _adapt_command_for_platform(cmd_stripped)

    blacklist_error = _check_command_blacklist(cmd_stripped)
    if blacklist_error:
        return blacklist_error

    escape_error = _detect_escape_attempt(cmd_stripped)
    if escape_error:
        return escape_error

    safe_env = _sanitize_environment()

    args = _try_split_command(cmd_stripped)
    use_shell = args is None

    try:
        from goat.security.sandbox import create_sandbox
        sandbox = create_sandbox()
        result = await sandbox.run(
            cmd_stripped if use_shell else " ".join(shlex.quote(a) for a in args),
            working_dir=str(wd),
            timeout=timeout,
            shell=use_shell,
        )
        output = result.stdout
        if result.stderr:
            output += "\n[stderr]\n" + result.stderr
        if not output:
            output = f"命令执行成功 (exit code: {result.returncode})"
        if len(output) > 50000:
            output = output[:50000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output
    except Exception as e:
        proc = await (
            asyncio.create_subprocess_shell(
                command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=str(wd),
                env=safe_env,
            ) if use_shell else asyncio.create_subprocess_exec(
                *args,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=str(wd),
                env=safe_env,
            )
        )

        stdout_lines: list[str] = []
        stderr_lines: list[str] = []

        async def _read_stream(
            stream: asyncio.StreamReader,
            lines: list[str],
        ) -> None:
            while True:
                line_bytes = await stream.readline()
                if not line_bytes:
                    break
                line = _decode_output(line_bytes).rstrip("\r\n")
                lines.append(line)

        stdout_task = asyncio.create_task(_read_stream(proc.stdout, stdout_lines))
        stderr_task = asyncio.create_task(_read_stream(proc.stderr, stderr_lines))

        try:
            await asyncio.wait_for(asyncio.gather(stdout_task, stderr_task, proc.wait()), timeout=timeout)
        except asyncio.TimeoutError:
            proc.kill()
            return f"错误: 命令执行超时 ({timeout}?"
        except asyncio.CancelledError:
            proc.terminate()
            return "执行被取?"

        output = "\n".join(stdout_lines)
        if stderr_lines:
            output += "\n[stderr]\n" + "\n".join(stderr_lines)
        if not output:
            output = f"命令执行成功 (exit code: {proc.returncode})"
        if len(output) > 50000:
            output = output[:50000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output


# ============================================================================
# 6. Web 工具
# ============================================================================

@tool
@retry_sync(_RETRYABLE_NET)
def web_search(query: str, max_results: int = 5) -> str:
    """搜索互联网获取最新信息?
    类似 Claude Code ?WebSearch、Codex CLI ?web_search?    使用 DuckDuckGo 的免费搜?API?
    Args:
        query: 搜索关键?        max_results: 返回结果数，默认 5，最?10
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
                results.append(f"?{text}")
                if first_url:
                    results.append(f"  {first_url}")
            elif "Topics" in topic:
                for sub in topic["Topics"][:3]:
                    if "Text" in sub:
                        results.append(f"?{sub['Text']}")
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
                results.append(f"?{html.unescape(re.sub(r'<[^>]+>', '', s)).strip()}")

        if not results:
            return f"未找?'{query}' 的相关结?"

        return f"--- 搜索结果: {query} ---\n" + "\n".join(results)

    except (requests.exceptions.ConnectionError,
            requests.exceptions.Timeout,
            OSError):
        raise
    except ImportError:
        return "错误: 需要安?requests ?(pip install requests)"
    except Exception as e:
        return f"错误: 搜索失败: {e}"


@tool
@retry_sync(_RETRYABLE_NET)
def web_fetch(url: str, max_length: int = 10000) -> str:
    """获取网页内容并转换为 Markdown 格式?
    类似 Claude Code ?WebFetch、Codex CLI ?web_fetch?    用于读取在线文档、API 响应等?
    Args:
        url: 网页 URL
        max_length: 最大返回字符数，默?10000
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

        return text if text else f"错误: 无法提取 '{url}' 的内?"

    except (requests.exceptions.ConnectionError,
            requests.exceptions.Timeout,
            OSError):
        raise
    except requests.exceptions.HTTPError as e:
        return f"错误: HTTP {e.response.status_code}: {url}"
    except ImportError:
        return "错误: 需要安?requests ?(pip install requests)"
    except Exception as e:
        return f"错误: 获取失败: {e}"


# ============================================================================
# 7. Git 工具
# ============================================================================

@tool
def git_status(path: str = "") -> str:
    """查看 Git 工作区状态（相当?git status）?
    Args:
        path: 可选的子目录或文件路径，只查看指定范围的状?    """
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
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_diff(path: str = "", cached: bool = False, unified: int = 3) -> str:
    """查看 Git 工作区差异（相当?git diff）?
    Args:
        path: 可选的子目录或文件路径
        cached: 是否查看已暂存的变更 (--cached)
        unified: 上下文行数，默认 3，范?0-50
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
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_log(max_count: int = 20, path: str = "", author: str = "",
            since: str = "") -> str:
    """查看 Git 提交历史（相当于 git log）?
    Args:
        max_count: 最大返回提交数，默?20，最?200
        path: 可选的子目录或文件路径，只查看指定文件的变更历?        author: 按作者筛选（?"author:张三"?        since: 起始时间（如 "2 weeks ago" ?"2025-01-01"?    """
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
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_commit(message: str, add_all: bool = True) -> str:
    """创建 Git 提交（先自动暂存变动，再提交）?
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
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_branch(name: str = "", action: str = "list", list_all: bool = False, force: bool = False) -> str:
    """列出、创建或删除 Git 分支（相当于 git branch）?
    Args:
        name: 分支名称。action ?"create" 时创建该分支；为 "delete" 时删除该分支
        action: 操作类型，可?"list"（列出）?create"（创建）?delete"（删除），默?"list"
        list_all: 是否列出所有分支（含远程分支），仅 action="list" 时有效，默认 False
        force: 是否强制删除未合并的分支（使?-D），?action="delete" 时有效，默认 False
    """
    try:
        if action == "create":
            if not name:
                return "错误: 创建分支时必须提?name"
            cmd = ["git", "branch", name]
        elif action == "delete":
            if not name:
                return "错误: 删除分支时必须提?name"
            flag = "-D" if force else "-d"
            cmd = ["git", "branch", flag, name]
        else:
            cmd = ["git", "branch"]
            if list_all:
                cmd.append("-a")

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return f"git branch 失败:\n{result.stderr.strip()}"
        output = result.stdout.strip()
        if not output:
            return "没有分支信息"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_stash(action: str = "list", message: str = "", index: int = 0) -> str:
    """管理 Git 暂存区（相当?git stash）?
    Args:
        action: 操作类型，可?"list"（列出）?push"（暂存）?pop"（恢复并删除）?                "apply"（恢复但不删除）?drop"（删除指定暂存）?show"（查看暂存内容），默?"list"
        message: 暂存时的描述信息，仅 action="push" 时有?        index: 暂存编号（从 0 开始），用?pop/apply/drop/show，默?0
    """
    try:
        if action == "push":
            cmd = ["git", "stash", "push"]
            if message:
                cmd.extend(["-m", message])
        elif action == "pop":
            cmd = ["git", "stash", "pop", f"stash@{{{index}}}"]
        elif action == "apply":
            cmd = ["git", "stash", "apply", f"stash@{{{index}}}"]
        elif action == "drop":
            cmd = ["git", "stash", "drop", f"stash@{{{index}}}"]
        elif action == "show":
            cmd = ["git", "stash", "show", "-p", f"stash@{{{index}}}"]
        else:
            cmd = ["git", "stash", "list"]

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        output = result.stdout.strip()
        if result.returncode != 0:
            error = result.stderr.strip()
            if "No stash found" in error:
                return "没有暂存记录"
            return f"git stash 失败:\n{error}"
        if not output:
            if action == "push":
                return "工作区已干净，无需暂存"
            if action == "drop":
                return f"已删除暂?stash@{{{index}}}"
            return "操作成功完成"
        if len(output) > 20000:
            output = output[:20000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_restore(path: str, staged: bool = False, source: str = "") -> str:
    """恢复 Git 工作区文件（相当?git restore）?
    Args:
        path: 要恢复的文件路径（必须）
        staged: 是否从暂存区恢复到工作区?-staged），默认 False
        source: 从指定提交或分支恢复?-source），?"HEAD~1" ?"main"
    """
    try:
        cmd = ["git", "restore"]
        if staged:
            cmd.append("--staged")
        if source:
            cmd.extend(["--source", source])
        cmd.append("--")
        cmd.append(path)

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return f"git restore 失败:\n{result.stderr.strip()}"
        parts = []
        if source:
            parts.append(f"?{source}")
        if staged:
            parts.append("从暂存区")
        parts.append(path)
        return f"已恢? {' '.join(parts)}"
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_blame(file_path: str, start_line: int = 1, end_line: int = 0, email: bool = False) -> str:
    """查看文件每行归属信息（相当于 git blame）?
    Args:
        file_path: 文件路径（必须）
        start_line: 起始行号 (1-based)，默?1
        end_line: 结束行号 (1-based)? 表示到文件末?        email: 是否显示邮箱代替用户名，默认 False
    """
    try:
        cmd = ["git", "blame"]
        if email:
            cmd.append("-e")
        if end_line > 0 and end_line >= start_line:
            cmd.append(f"-L {start_line},{end_line}")
        cmd.append("--")
        cmd.append(file_path)

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=30,
        )
        if result.returncode != 0:
            return f"git blame 失败:\n{result.stderr.strip()}"
        output = result.stdout.strip()
        if not output:
            return f"文件 '{file_path}' 为空或没?blame 信息"
        if len(output) > 20000:
            output = output[:20000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


@tool
def git_cherry_pick(commits: str, no_commit: bool = False) -> str:
    """将指定提交应用到当前分支（相当于 git cherry-pick）?
    Args:
        commits: 要应用的提交哈希，多个提交用空格分隔，如 "abc123 def456"
        no_commit: 仅应用变更但不自动创建提交（--no-commit），默认 False
    """
    try:
        cmd = ["git", "cherry-pick"]
        if no_commit:
            cmd.append("--no-commit")
        cmd.extend(commits.split())

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=60,
        )
        output = result.stdout.strip()
        if result.returncode != 0:
            error = result.stderr.strip()
            if "nothing to commit" in error:
                return "cherry-pick 无变更可应用"
            return f"git cherry-pick 失败:\n{error}"
        if not output:
            return f"成功应用提交: {commits}"
        if len(output) > 20000:
            output = output[:20000] + f"\n\n[输出截断，共 {len(output)} 字符]"
        return output
    except FileNotFoundError:
        return "错误: git 未安装或不在 PATH ?"
    except subprocess.TimeoutExpired:
        return "错误: git 命令超时"
    except Exception as e:
        return f"错误: git 执行失败: {e}"


# ============================================================================
# 工具注册?# ============================================================================

WRITE_FILE_TOOL = WriteFileTool()
DELETE_FILE_TOOL = DeleteFileTool()
EXECUTE_COMMAND_TOOL = ExecuteCommandTool()
FILE_EDIT_TOOL = FileEditTool()
FILE_GREP_TOOL = FileGrepTool()

BUILTIN_TOOLS = {
    "list_files": list_files,
    "read_file": read_file,
    "write_file": WRITE_FILE_TOOL,
    "delete_file": DELETE_FILE_TOOL,
    "move_file": move_file,
    "copy_file": copy_file,
    "get_file_info": get_file_info,
    "glob_search": glob_search,
    "file_edit": FILE_EDIT_TOOL,
    "file_grep": FILE_GREP_TOOL,
    "search_code": search_code,
    "execute_command": EXECUTE_COMMAND_TOOL,
    "async_execute_command": async_execute_command,
    "web_search": web_search,
    "web_fetch": web_fetch,
    "git_status": git_status,
    "git_diff": git_diff,
    "git_log": git_log,
    "git_commit": git_commit,
    "git_branch": git_branch,
    "git_stash": git_stash,
    "git_restore": git_restore,
    "git_blame": git_blame,
    "git_cherry_pick": git_cherry_pick,
}

try:
    from goat.memory.remember_tool import REMEMBER_TOOL
    BUILTIN_TOOLS["remember"] = REMEMBER_TOOL
except ImportError:
    pass

try:
    from goat.tools.browser_tools import web_run, web_screenshot
    BUILTIN_TOOLS["web_run"] = web_run
    BUILTIN_TOOLS["web_screenshot"] = web_screenshot
except ImportError:
    pass

try:
    from goat.tools.notify_tool import NOTIFY_TOOL
    BUILTIN_TOOLS["notify"] = NOTIFY_TOOL
except ImportError:
    pass

try:
    from goat.tools.ask_user_tool import ASK_USER_TOOL
    BUILTIN_TOOLS["ask_user"] = ASK_USER_TOOL
except ImportError:
    pass

try:
    from goat.tools.patch_tool import APPLY_PATCH_TOOL
    BUILTIN_TOOLS["apply_patch"] = APPLY_PATCH_TOOL
except ImportError:
    pass

try:
    from goat.tools.find_skills_tool import FIND_SKILLS_TOOL
    BUILTIN_TOOLS["find_skills"] = FIND_SKILLS_TOOL
except ImportError:
    pass

# save_plan_doc 作为内置工具直接注册
BUILTIN_TOOLS["save_plan_doc"] = save_plan_doc


def get_all_tools() -> list:
    return list(BUILTIN_TOOLS.values())


def get_tools_by_names(names: list[str]) -> list:
    tools = [BUILTIN_TOOLS[n] for n in names if n in BUILTIN_TOOLS]
    tools.sort(key=lambda t: t.name)
    return tools


# ============================================================================
# 工具调用描述生成器（用于审批弹窗展示 LLM 意图）
# ============================================================================

def describe_tool_action(name: str, args: dict) -> str:
    """根据工具名和参数生成人类可读的工具调用意图描述。

    - 优先从 args 中读取 LLM 主动提供的 reason/purpose/description
    - 降级为基于参数的自动描述
    """
    llm_reason = args.get("reason") or args.get("purpose") or args.get("description")
    if llm_reason and isinstance(llm_reason, str) and llm_reason.strip():
        return llm_reason.strip()[:200]

    if name in ("write_file",):
        fp = args.get("filepath", "?")
        content = args.get("content", "")
        return f"将 {len(content)} 字符的内容写入文件 {fp}"
    if name in ("read_file", "get_file_info"):
        return f"读取文件 {args.get('filepath', args.get('file_path', '?'))}"
    if name in ("delete_file",):
        extra = "（递归删除）" if args.get("recursive") else ""
        return f"删除 {args.get('filepath', '?')}{extra}"
    if name in ("move_file",):
        return f"将 {args.get('source', '?')} 移动到 {args.get('destination', '?')}"
    if name in ("copy_file",):
        return f"将 {args.get('source', '?')} 复制到 {args.get('destination', '?')}"
    if name in ("file_edit",):
        fp = args.get("file_path", "?")
        old = (args.get("old_string", "") or "")[:30]
        new = (args.get("new_string", "") or "")[:30]
        return f"编辑文件 {fp}：\"{old}\" → \"{new}\""
    if name in ("glob_search",):
        return f"在 {args.get('directory', '.')} 中搜索匹配 {args.get('pattern', '?')} 的文件"
    if name in ("list_files",):
        return f"列出 {args.get('directory', '.')} 中的文件"
    if name in ("file_grep", "search_code"):
        return f"在 {args.get('path', '.')} 中搜索 \"{args.get('query') or args.get('pattern', '')}\""
    if name in ("execute_command", "async_execute_command"):
        cmd = (args.get("command", "") or "")[:80]
        return f"执行命令：{cmd}"
    if name in ("web_search",):
        return f"搜索网络：{args.get('query', '?')}"
    if name in ("web_fetch",):
        return f"抓取网页：{args.get('url', '?')}"
    if name in ("git_status",):
        return "查看 Git 状态"
    if name in ("git_diff",):
        return "查看 Git 差异"
    if name in ("git_log",):
        return "查看 Git 提交历史"
    if name in ("git_blame",):
        return "查看 Git blame 信息"
    if name in ("git_commit",):
        msg = (args.get("message", "") or "")[:50]
        return f"提交 Git 变更：\"{msg}\""
    if name in ("git_branch",):
        action = args.get("action", "list")
        branch = args.get("name", "")
        if action == "create":
            return f"创建 Git 分支 {branch}"
        if action == "delete":
            return f"删除 Git 分支 {branch}"
        return "列出 Git 分支"
    if name in ("git_stash",):
        action = args.get("action", "list")
        action_labels = {"list": "列出", "push": "暂存", "pop": "恢复并删除", "apply": "恢复", "drop": "删除", "show": "查看"}
        return f"Git stash：{action_labels.get(action, action)} 当前变更"
    if name in ("git_restore",):
        return f"从 {args.get('source', 'HEAD')} 恢复 {args.get('path', '?')}"
    if name in ("git_cherry_pick",):
        return f"应用提交 {args.get('commits', '?')}"
    if name in ("ask_user",):
        q = (args.get("question", "") or "")[:60]
        return f"询问用户：{q}"
    if name in ("notify",):
        msg = (args.get("message", "") or "")[:60]
        return f"发送通知：{msg}"
    if name in ("save_plan_doc",):
        return f"保存计划文档 .goat/doc/{args.get('filename', '?')}"
    if name in ("apply_patch",):
        target = args.get("target_file", "") or ""
        if not target:
            target = "从补丁头解析"
        return f"应用补丁到 {target}"
    if name in ("find_skills",):
        return f"搜索技能：{args.get('query', '?')}"

    # fallback
    keys = list(args.keys())[:3]
    return f"调用 {name}（参数：{', '.join(k for k in keys if k not in ('reason',))}）"















