// D1-T04: unified diff 解析工具
// 参考 rgoat-tui/src/components/diff_view.rs 的逻辑改写为 TypeScript

export interface DiffLine {
  type: 'add' | 'del' | 'context' | 'hunk' | 'meta';
  oldLineNo?: number;
  newLineNo?: number;
  content: string;
}

export interface DiffStats {
  additions: number;
  deletions: number;
}

const HUNK_HEADER_RE = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/;

/**
 * 解析 unified diff 字符串为 DiffLine 数组
 * 输入格式示例：
 *   --- a/path
 *   +++ b/path
 *   @@ -1,3 +1,4 @@
 *    context
 *   -deleted
 *   +added
 */
export function parseUnifiedDiff(diff: string): DiffLine[] {
  if (!diff) return [];

  const lines = diff.split('\n');
  const result: DiffLine[] = [];
  let oldLine = 0;
  let newLine = 0;

  for (const raw of lines) {
    if (raw === '') continue;

    // 文件头 --- a/path / +++ b/path
    if (raw.startsWith('--- ') || raw.startsWith('+++ ')) {
      result.push({ type: 'meta', content: raw });
      continue;
    }

    // hunk header @@ -L1,N1 +L2,N2 @@
    const hunkMatch = raw.match(HUNK_HEADER_RE);
    if (hunkMatch) {
      oldLine = parseInt(hunkMatch[1], 10);
      newLine = parseInt(hunkMatch[3], 10);
      result.push({ type: 'hunk', content: raw });
      continue;
    }

    // 添加行
    if (raw.startsWith('+')) {
      result.push({
        type: 'add',
        newLineNo: newLine,
        content: raw.slice(1),
      });
      newLine++;
      continue;
    }

    // 删除行
    if (raw.startsWith('-')) {
      result.push({
        type: 'del',
        oldLineNo: oldLine,
        content: raw.slice(1),
      });
      oldLine++;
      continue;
    }

    // 上下文行（空格前缀或无前缀）
    if (raw.startsWith(' ')) {
      result.push({
        type: 'context',
        oldLineNo: oldLine,
        newLineNo: newLine,
        content: raw.slice(1),
      });
      oldLine++;
      newLine++;
      continue;
    }

    // 其他行（如 "\ No newline at end of file"）作为 context 处理
    result.push({ type: 'context', content: raw });
  }

  return result;
}

/**
 * 统计 diff 中的添加/删除行数
 */
export function computeDiffStats(diff: string): DiffStats {
  if (!diff) return { additions: 0, deletions: 0 };

  const lines = diff.split('\n');
  let additions = 0;
  let deletions = 0;

  for (const line of lines) {
    // 必须是真正的 +/- 行，而非 +++ b/path 或 --- a/path 文件头
    if (line.startsWith('+') && !line.startsWith('+++')) {
      additions++;
    } else if (line.startsWith('-') && !line.startsWith('---')) {
      deletions++;
    }
  }

  return { additions, deletions };
}

/**
 * 根据文件路径推断语言（用于语法高亮的提示，D1 阶段暂不深度集成）
 */
export function detectLanguage(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase() || '';
  const map: Record<string, string> = {
    ts: 'typescript',
    tsx: 'tsx',
    js: 'javascript',
    jsx: 'jsx',
    py: 'python',
    rs: 'rust',
    go: 'go',
    java: 'java',
    json: 'json',
    md: 'markdown',
    html: 'html',
    css: 'css',
    toml: 'toml',
    yaml: 'yaml',
    yml: 'yaml',
    sh: 'bash',
  };
  return map[ext] || 'plaintext';
}
