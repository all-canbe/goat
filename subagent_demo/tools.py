from __future__ import annotations

import subprocess
from pathlib import Path

from langchain_core.tools import tool


@tool
def list_files(directory: str = ".", pattern: str = "*") -> str:
    """列出目录中的文件和子目录。

    Args:
        directory: 要列出的目录路径，默认为当前目录
        pattern: glob 匹配模式，默认为 *
    """
    p = Path(directory).expanduser().resolve()
    if not p.exists():
        return f"错误: 目录不存在: {directory}"
    if not p.is_dir():
        return f"错误: 不是目录: {directory}"

    results = []
    for item in sorted(p.glob(pattern)):
        prefix = "[D]" if item.is_dir() else "[F]"
        size = ""
        if item.is_file():
            try:
                size = f" ({item.stat().st_size} bytes)"
            except OSError:
                pass
        results.append(f"{prefix} {item.name}{size}")

    if not results:
        return f"目录为空: {directory}"
    return "\n".join(results)


@tool
def read_file(filepath: str, start_line: int = 1, end_line: int = -1) -> str:
    """读取文件内容。

    Args:
        filepath: 文件路径
        start_line: 起始行号 (1-based)，默认 1
        end_line: 结束行号 (1-based)，-1 表示读到末尾
    """
    p = Path(filepath).expanduser().resolve()
    if not p.exists():
        return f"错误: 文件不存在: {filepath}"
    if not p.is_file():
        return f"错误: 不是文件: {filepath}"

    try:
        content = p.read_text(encoding="utf-8")
    except Exception as e:
        return f"错误: 读取文件失败: {e}"

    lines = content.split("\n")
    total = len(lines)

    if end_line == -1:
        end_line = total
    start_line = max(1, start_line)
    end_line = min(total, end_line)

    selected = lines[start_line - 1 : end_line]
    header = f"--- {filepath} (行 {start_line}-{end_line} / 共 {total} 行) ---"
    return header + "\n" + "\n".join(selected)


@tool
def write_file(filepath: str, content: str) -> str:
    """写入内容到文件（覆盖模式）。

    Args:
        filepath: 文件路径
        content: 要写入的内容
    """
    p = Path(filepath).expanduser().resolve()
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding="utf-8")
        return f"写入成功: {filepath} ({len(content)} 字符)"
    except Exception as e:
        return f"错误: 写入文件失败: {e}"


@tool
def search_code(directory: str, query: str, file_pattern: str = "*") -> str:
    """在代码文件中搜索指定内容（使用 grep）。

    Args:
        directory: 搜索目录
        query: 搜索关键词或正则表达式
        file_pattern: 文件名模式，默认 *
    """
    p = Path(directory).expanduser().resolve()
    if not p.exists():
        return f"错误: 目录不存在: {directory}"

    try:
        result = subprocess.run(
            ["grep", "-rn", "--include=" + file_pattern, query, str(p)],
            capture_output=True, text=True, timeout=30,
        )
        output = result.stdout.strip()
        if not output:
            return f"未找到匹配: {query}"
        lines = output.split("\n")[:50]
        summary = f"找到 {len(output.split(chr(10)))} 个匹配 (显示前 50 个):\n" + "\n".join(lines)
        return summary
    except subprocess.TimeoutExpired:
        return "错误: 搜索超时"
    except Exception as e:
        return f"错误: 搜索失败: {e}"


@tool
def execute_command(command: str, working_dir: str = ".") -> str:
    """执行 shell 命令并返回输出。

    Args:
        command: 要执行的命令
        working_dir: 工作目录
    """
    wd = Path(working_dir).expanduser().resolve()
    try:
        result = subprocess.run(
            command, shell=True, capture_output=True, text=True,
            timeout=60, cwd=str(wd),
        )
        output = result.stdout.strip()
        if result.stderr.strip():
            output += "\n[stderr]\n" + result.stderr.strip()
        if not output:
            output = f"命令执行成功 (exit code: {result.returncode})"
        return output
    except subprocess.TimeoutExpired:
        return "错误: 命令执行超时"
    except Exception as e:
        return f"错误: 命令执行失败: {e}"


BUILTIN_TOOLS = {
    "list_files": list_files,
    "read_file": read_file,
    "write_file": write_file,
    "search_code": search_code,
    "execute_command": execute_command,
}


def get_all_tools() -> list:
    return list(BUILTIN_TOOLS.values())


def get_tools_by_names(names: list[str]) -> list:
    return [BUILTIN_TOOLS[n] for n in names if n in BUILTIN_TOOLS]