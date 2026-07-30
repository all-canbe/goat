//! Edit 工具 — 精确字符串替换编辑（支持批量、模糊匹配、BOM/行尾保留）
//!
//! 从 builtin.rs 拆分而来。持有 `FileMutationQueue` 序列化同一文件的并发编辑，
//! 并在写文件前检查 `CancellationToken`。
//!
//! 增强特性：
//! - 批量编辑（edits[] 数组），保留 old_string/new_string 作为 legacy 兼容
//! - 模糊匹配（trailing whitespace、smart quote、unicode dash、特殊空格）
//! - BOM 检测与恢复
//! - 行尾检测（CRLF/LF）与保留
//! - 重叠检测（批量模式下 edits 不能重叠）
//! - 精确错误信息（区分 empty/not_found/duplicate/no_change）

use async_trait::async_trait;
use serde_json::json;
use similar::{ChangeTag, TextDiff};
use std::path::PathBuf;
use std::sync::Arc;

use super::file_mutation_queue::FileMutationQueue;
use super::registry::{Tool, ToolResult};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

pub struct EditFileTool {
    workspace: PathBuf,
    mutation_queue: Arc<FileMutationQueue>,
    cancellation: Option<CancellationToken>,
}

impl EditFileTool {
    pub fn new(
        workspace: PathBuf,
        cancellation: Option<CancellationToken>,
        mutation_queue: Arc<FileMutationQueue>,
    ) -> Self {
        Self {
            workspace,
            cancellation,
            mutation_queue,
        }
    }
}

// ============================================================================
// 辅助函数：diff 与路径
// ============================================================================

/// 生成 unified diff 格式字符串。
///
/// 输入旧/新文本与展示用文件路径，返回 `--- a/<path>\n+++ b/<path>\n` 开头的 diff。
/// 若 old == new 返回空字符串。
fn generate_unified_diff(old: &str, new: &str, file_path: &str) -> String {
    if old == new {
        return String::new();
    }
    let diff = TextDiff::from_lines(old, new);
    let mut output = String::new();
    output.push_str(&format!("--- a/{}\n", file_path));
    output.push_str(&format!("+++ b/{}\n", file_path));
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        output.push(sign);
        output.push_str(change.value());
        // 最后一行可能无换行符，补齐以保证 diff 行对齐
        if !change.value().ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

/// 将绝对路径转为相对 workspace 的路径字符串（用于 diff 头部展示）。
fn relative_path_str(path: &std::path::Path, workspace: &std::path::Path) -> String {
    path.strip_prefix(workspace)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

// ============================================================================
// 辅助函数：行尾检测与归一化
// ============================================================================

/// 检测内容的主导行尾。返回 "\r\n" 或 "\n"。
///
/// 取内容中首个 \n 的位置：若其前紧邻 \r 则为 CRLF，否则 LF。
/// 参考 pi edit-diff.ts:7-13。
fn detect_line_ending(content: &str) -> &'static str {
    let crlf_idx = content.find("\r\n");
    let lf_idx = content.find('\n');
    match (crlf_idx, lf_idx) {
        (_, None) => "\n",
        (None, Some(_)) => "\n",
        (Some(c), Some(l)) if c < l => "\r\n",
        _ => "\n",
    }
}

/// 将 CRLF/CR 统一为 LF。参考 pi edit-diff.ts:15-17。
fn normalize_to_lf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 按 ending 恢复行尾。ending == "\r\n" 时把所有 \n 换成 \r\n。
/// 参考 pi edit-diff.ts:19-21。
fn restore_line_endings(text: &str, ending: &str) -> String {
    if ending == "\r\n" {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

// ============================================================================
// 辅助函数：BOM 处理
// ============================================================================

/// 检测并剥离 UTF-8 BOM (U+FEFF)。返回 (has_bom, 剥离后的内容)。
/// 参考 pi edit-diff.ts:244-246。
fn strip_bom(content: &str) -> (bool, &str) {
    if let Some(stripped) = content.strip_prefix('\u{FEFF}') {
        (true, stripped)
    } else {
        (false, content)
    }
}

// ============================================================================
// 辅助函数：模糊匹配
// ============================================================================

/// 归一化文本用于模糊匹配：
/// - smart quotes → ASCII 等价物
/// - unicode dashes → ASCII 连字符
/// - 特殊空格 → 普通空格
/// - 每行 trim trailing whitespace
///
/// 替代 NFKC 归一化（Rust 无内置，硬编码常见 LLM 错误字符）。
/// 参考 pi edit-diff.ts:30-51。
fn normalize_for_fuzzy_match(text: &str) -> String {
    let mut replaced = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            // Smart single quotes (U+2018/U+2019/U+201A/U+201B) → '
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => replaced.push('\''),
            // Smart double quotes (U+201C-U+201F) → "
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => replaced.push('"'),
            // Unicode dashes (U+2010-U+2015, U+2212) → -
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
            | '\u{2212}' => replaced.push('-'),
            // 特殊空格 (U+00A0, U+2002-U+200A, U+202F, U+205F, U+3000) → 普通空格
            '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
            | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
            | '\u{3000}' => replaced.push(' '),
            _ => replaced.push(c),
        }
    }
    // 每行 trim trailing whitespace（保留 \n）
    let mut normalized = String::with_capacity(replaced.len());
    for line in replaced.split_inclusive('\n') {
        if let Some(body) = line.strip_suffix('\n') {
            normalized.push_str(body.trim_end());
            normalized.push('\n');
        } else {
            normalized.push_str(line.trim_end());
        }
    }
    normalized
}

/// 模糊匹配结果
struct FuzzyMatchResult {
    found: bool,
    index: usize,
    match_length: usize,
    /// 是否走了 fuzzy 路径（规格要求的返回值；execute 用更高效的 contains 预判 used_fuzzy）
    #[allow(dead_code)]
    used_fuzzy: bool,
}

/// 在 content 中查找 old_text：先精确，失败则归一化后模糊匹配。
///
/// 返回匹配的字节位置与长度（在 content 或其归一化版本中）。
/// 参考 pi edit-diff.ts:203-241。
fn fuzzy_find_text(content: &str, old_text: &str) -> FuzzyMatchResult {
    // 精确匹配优先
    if let Some(idx) = content.find(old_text) {
        return FuzzyMatchResult {
            found: true,
            index: idx,
            match_length: old_text.len(),
            used_fuzzy: false,
        };
    }
    // 模糊匹配：在归一化空间查找
    let fuzzy_content = normalize_for_fuzzy_match(content);
    let fuzzy_old = normalize_for_fuzzy_match(old_text);
    if let Some(idx) = fuzzy_content.find(&fuzzy_old) {
        return FuzzyMatchResult {
            found: true,
            index: idx,
            match_length: fuzzy_old.len(),
            used_fuzzy: true,
        };
    }
    FuzzyMatchResult {
        found: false,
        index: 0,
        match_length: 0,
        used_fuzzy: false,
    }
}

/// 在 content 中计数 old_text 出现次数。
/// used_fuzzy 时对 old_text 做归一化（content 假定已归一化）。
/// 参考 pi edit-diff.ts:248-252。
fn count_occurrences(content: &str, old_text: &str, used_fuzzy: bool) -> usize {
    if used_fuzzy {
        let fuzzy_old = normalize_for_fuzzy_match(old_text);
        content.matches(fuzzy_old.as_str()).count()
    } else {
        content.matches(old_text).count()
    }
}

// ============================================================================
// 批量编辑：数据结构与替换应用
// ============================================================================

/// 单条编辑（old → new）
struct EditEntry {
    old_string: String,
    new_string: String,
}

/// 已匹配的编辑（带位置信息）
#[derive(Clone)]
struct MatchedEdit {
    edit_index: usize,
    match_index: usize,
    match_length: usize,
    new_text: String,
}

/// 文本替换描述（用于 apply 函数）
struct TextReplacement {
    match_index: usize,
    match_length: usize,
    new_text: String,
}

/// 按行分割，保留行尾分隔符。最后一行若无换行符，也作为一行返回。
fn split_lines_with_endings(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = content.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            lines.push(&content[start..=i]);
            start = i + 1;
        }
    }
    if start < content.len() {
        lines.push(&content[start..]);
    }
    lines
}

/// 计算每行的字节范围 (start, end)
fn get_line_spans(content: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for line in split_lines_with_endings(content) {
        let end = offset + line.len();
        spans.push((offset, end));
        offset = end;
    }
    spans
}

/// 在 content 上按逆序应用替换，返回新字符串。
/// 参考 pi edit-diff.ts:107-116。
fn apply_replacements(content: &str, replacements: &[TextReplacement]) -> String {
    let mut sorted: Vec<&TextReplacement> = replacements.iter().collect();
    // 逆序排序：从后往前替换，保证前面位置不被破坏
    sorted.sort_by(|a, b| b.match_index.cmp(&a.match_index));
    let mut result = content.to_string();
    for r in sorted {
        let start = r.match_index;
        let end = r.match_index + r.match_length;
        result.replace_range(start..end, &r.new_text);
    }
    result
}

/// 计算替换覆盖的行范围 [start_line, end_line)。
/// 参考 pi edit-diff.ts:80-105。
fn get_replacement_line_range(
    lines: &[(usize, usize)],
    replacement: &TextReplacement,
) -> Option<(usize, usize)> {
    let rep_start = replacement.match_index;
    let rep_end = replacement.match_index + replacement.match_length;

    let mut start_line = None;
    for (i, &(line_start, line_end)) in lines.iter().enumerate() {
        if rep_start >= line_start && rep_start < line_end {
            start_line = Some(i);
            break;
        }
    }
    let start_line = start_line?;

    let mut end_line = start_line;
    while end_line < lines.len() && lines[end_line].1 < rep_end {
        end_line += 1;
    }
    if end_line >= lines.len() {
        return None;
    }
    Some((start_line, end_line + 1))
}

/// 在 base_content 空间匹配的替换，映射回 original_content，保留未改动行的原始字节。
///
/// 用于 fuzzy 匹配场景：base_content 是 fuzzy 归一化后的内容，original_content 是
/// LF 归一化的原始内容。两者行数相同，逐行对应。改动的行块从 base_content 取并应用
/// 替换；未改动的行从 original_content 取，保留原始字节（trailing whitespace 等）。
/// 参考 pi edit-diff.ts:128-169。
fn apply_replacements_preserving_unchanged_lines<'a>(
    original_content: &str,
    base_content: &str,
    replacements: &'a [TextReplacement],
) -> Result<String, String> {
    let original_lines = split_lines_with_endings(original_content);
    let base_lines = get_line_spans(base_content);
    if original_lines.len() != base_lines.len() {
        return Err(format!(
            "Cannot preserve unchanged lines: line count mismatch (original={}, base={})",
            original_lines.len(),
            base_lines.len()
        ));
    }

    // 按匹配位置排序
    let mut sorted: Vec<&TextReplacement> = replacements.iter().collect();
    sorted.sort_by(|a, b| a.match_index.cmp(&b.match_index));

    // 分组：相邻/重叠的替换合并为一个 group
    struct Group<'a> {
        start_line: usize,
        end_line: usize,
        replacements: Vec<&'a TextReplacement>,
    }
    let mut groups: Vec<Group<'a>> = Vec::new();
    for r in &sorted {
        let range = get_replacement_line_range(&base_lines, r)
            .ok_or_else(|| "Replacement range outside base content".to_string())?;
        if let Some(last) = groups.last_mut() {
            if range.0 < last.end_line {
                last.end_line = last.end_line.max(range.1);
                last.replacements.push(*r);
                continue;
            }
        }
        groups.push(Group {
            start_line: range.0,
            end_line: range.1,
            replacements: vec![*r],
        });
    }

    let mut result = String::new();
    let mut original_line_index = 0;
    for group in &groups {
        // 复制未改动的行（保留原始字节）
        if group.start_line > original_line_index {
            for line in &original_lines[original_line_index..group.start_line] {
                result.push_str(line);
            }
        }
        // 对改动的行块，从 base_content 切片并应用替换
        let group_start_offset = base_lines[group.start_line].0;
        let group_end_offset = base_lines[group.end_line - 1].1;
        let group_slice = &base_content[group_start_offset..group_end_offset];
        // 调整 offset：replacements 的 match_index 相对 base_content，需减去 group_start_offset
        let adjusted: Vec<TextReplacement> = group
            .replacements
            .iter()
            .map(|r| TextReplacement {
                match_index: r.match_index - group_start_offset,
                match_length: r.match_length,
                new_text: r.new_text.clone(),
            })
            .collect();
        result.push_str(&apply_replacements(group_slice, &adjusted));
        original_line_index = group.end_line;
    }
    // 复制剩余行
    if original_line_index < original_lines.len() {
        for line in &original_lines[original_line_index..] {
            result.push_str(line);
        }
    }

    Ok(result)
}

// ============================================================================
// 参数解析
// ============================================================================

/// 解析编辑参数：支持批量 edits[] 数组与 legacy old_string/new_string。
///
/// 返回 (edits, replace_all, is_legacy)：
/// - 若提供 edits 数组，使用数组（replace_all=false, is_legacy=false）
/// - 否则用 old_string/new_string 构造单元素 edits（is_legacy=true）
fn parse_edit_args(args: &serde_json::Value) -> (Vec<EditEntry>, bool, bool) {
    if let Some(edits_arr) = args.get("edits").and_then(|v| v.as_array()) {
        let edits = edits_arr
            .iter()
            .map(|e| EditEntry {
                old_string: e
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                new_string: e
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
            .collect();
        (edits, false, false)
    } else {
        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        (
            vec![EditEntry {
                old_string,
                new_string,
            }],
            replace_all,
            true,
        )
    }
}

// ============================================================================
// Tool 实现
// ============================================================================

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Performs exact string replacements in an existing file. Supports batch edits via edits[] array. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to modify"
                },
                "edits": {
                    "type": "array",
                    "description": "Array of {old_string, new_string} replacements. Each old_string must be unique and non-overlapping in the original file. All edits match against the original content (not incrementally). If two changes touch the same block, merge them into one edit.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": {
                                "type": "string",
                                "description": "Exact text to replace (must be unique in the file)"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement text"
                            }
                        },
                        "required": ["old_string", "new_string"]
                    }
                },
                "old_string": {
                    "type": "string",
                    "description": "Legacy single-edit mode: text to replace. Ignored if edits[] is provided."
                },
                "new_string": {
                    "type": "string",
                    "description": "Legacy single-edit mode: replacement text. Ignored if edits[] is provided."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Legacy single-edit mode: replace all occurrences (default: false). Only effective when using old_string/new_string; ignored for edits[] array.",
                    "default": false
                }
            },
            "required": ["file_path"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Write
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let file_path = args["file_path"].as_str().unwrap_or("").to_string();
        let path = match crate::core::paths::resolve_workspace_path(&file_path, &self.workspace) {
            Ok(p) => p,
            Err(_) => {
                return ToolResult::error(
                    "Path must be inside the workspace",
                    "path_outside_workspace",
                )
            }
        };
        let workspace = self.workspace.clone();
        let cancellation = self.cancellation.clone();

        let (edits, replace_all, is_legacy) = parse_edit_args(&args);

        if edits.is_empty() {
            return ToolResult::error(
                "No edits provided. Provide edits[] array or old_string/new_string.",
                "no_edits",
            );
        }

        // 入队前检查取消
        if let Some(token) = &cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }

        let path_for_key = path.clone();
        self.mutation_queue
            .run(&path_for_key, async move {
                // 持有锁后、读旧内容前检查取消
                if let Some(token) = &cancellation {
                    if token.is_cancelled() {
                        return ToolResult::error("Operation aborted", "cancelled");
                    }
                }

                let raw_content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        return ToolResult::error(
                            format!("Failed to read file: {}", e),
                            format!("{}", e),
                        )
                    }
                };

                // BOM 处理
                let (has_bom, content_no_bom) = strip_bom(&raw_content);
                // 行尾检测
                let line_ending = detect_line_ending(content_no_bom);
                // 归一化为 LF
                let normalized_content = normalize_to_lf(content_no_bom);

                // 对每个 edit 的 old/new 也归一化为 LF
                let normalized_edits: Vec<(String, String)> = edits
                    .iter()
                    .map(|e| (normalize_to_lf(&e.old_string), normalize_to_lf(&e.new_string)))
                    .collect();

                let total = normalized_edits.len();
                let is_single = total == 1;

                // 检查空 old_string
                for (i, (old, _)) in normalized_edits.iter().enumerate() {
                    if old.is_empty() {
                        let msg = if is_single {
                            format!("old_string must not be empty in {}", file_path)
                        } else {
                            format!("edits[{}].old_string must not be empty in {}", i, file_path)
                        };
                        return ToolResult::error(msg, "empty_old_string");
                    }
                }

                // legacy 单 edit + replace_all：精确替换所有，不做模糊/唯一性检查
                if is_legacy && replace_all {
                    let (old, new) = &normalized_edits[0];
                    let occurrences = normalized_content.matches(old.as_str()).count();
                    if occurrences == 0 {
                        let msg = format!(
                            "Could not find old_string in {}. The old_string must match exactly including all whitespace and newlines.",
                            file_path
                        );
                        return ToolResult::error(msg, "string_not_found");
                    }
                    if old == new {
                        let msg = format!(
                            "No changes made to {}. The replacement produced identical content.",
                            file_path
                        );
                        return ToolResult::error(msg, "no_change");
                    }
                    let new_content = normalized_content.replace(old.as_str(), new.as_str());

                    // 写前检查取消
                    if let Some(token) = &cancellation {
                        if token.is_cancelled() {
                            return ToolResult::error("Operation aborted", "cancelled");
                        }
                    }

                    let final_content = if has_bom {
                        format!("\u{FEFF}{}", restore_line_endings(&new_content, line_ending))
                    } else {
                        restore_line_endings(&new_content, line_ending)
                    };

                    return match std::fs::write(&path, &final_content) {
                        Ok(_) => {
                            let rel_path = relative_path_str(&path, &workspace);
                            let diff =
                                generate_unified_diff(&normalized_content, &new_content, &rel_path);
                            let affected = vec![rel_path];
                            ToolResult::success(format!(
                                "Successfully edited {}. Replaced {} occurrence(s).",
                                file_path, occurrences
                            ))
                            .with_diff(diff, affected)
                            .with_metadata(json!({
                                "change_type": "edit",
                                "old_size": raw_content.len(),
                                "new_size": final_content.len(),
                                "occurrences_replaced": occurrences,
                                "replace_all": true,
                            }))
                        }
                        Err(e) => ToolResult::error(
                            format!("Failed to write file: {}", e),
                            format!("{}", e),
                        ),
                    };
                }

                // 非 replace_all：fuzzy 匹配 + 唯一性 + 重叠检测
                // 检查是否需要 fuzzy（任一 edit 在 normalized_content 中精确匹配失败）
                let used_fuzzy = normalized_edits
                    .iter()
                    .any(|(old, _)| !normalized_content.contains(old.as_str()));

                let replacement_base = if used_fuzzy {
                    normalize_for_fuzzy_match(&normalized_content)
                } else {
                    normalized_content.clone()
                };

                // 对每个 edit 查找匹配并校验唯一性
                let mut matched_edits: Vec<MatchedEdit> = Vec::new();
                for (i, (old, new)) in normalized_edits.iter().enumerate() {
                    let match_result = fuzzy_find_text(&replacement_base, old);
                    if !match_result.found {
                        let msg = if is_single {
                            format!(
                                "Could not find old_string in {}. The old_string must match exactly including all whitespace and newlines.",
                                file_path
                            )
                        } else {
                            format!(
                                "Could not find edits[{}] in {}. The old_string must match exactly including all whitespace and newlines.",
                                i, file_path
                            )
                        };
                        return ToolResult::error(msg, "string_not_found");
                    }

                    let occurrences = count_occurrences(&replacement_base, old, used_fuzzy);
                    if occurrences > 1 {
                        let msg = if is_single {
                            format!(
                                "Found {} occurrences of old_string in {}. old_string must be unique. Provide more context.",
                                occurrences, file_path
                            )
                        } else {
                            format!(
                                "Found {} occurrences of edits[{}] in {}. Each old_string must be unique. Provide more context.",
                                occurrences, i, file_path
                            )
                        };
                        return ToolResult::error(msg, "multiple_occurrences");
                    }

                    matched_edits.push(MatchedEdit {
                        edit_index: i,
                        match_index: match_result.index,
                        match_length: match_result.match_length,
                        new_text: new.clone(),
                    });
                }

                // 重叠检测：按匹配位置排序后检查 prev.end > curr.start
                // 参考 pi edit-diff.ts:343-351
                let mut sorted_matched = matched_edits.clone();
                sorted_matched.sort_by(|a, b| a.match_index.cmp(&b.match_index));
                for i in 1..sorted_matched.len() {
                    let prev = &sorted_matched[i - 1];
                    let curr = &sorted_matched[i];
                    if prev.match_index + prev.match_length > curr.match_index {
                        let msg = format!(
                            "edits[{}] and edits[{}] overlap in {}. Merge them into one edit or target disjoint regions.",
                            prev.edit_index, curr.edit_index, file_path
                        );
                        return ToolResult::error(msg, "overlapping_edits");
                    }
                }

                // 应用替换
                let replacements: Vec<TextReplacement> = matched_edits
                    .iter()
                    .map(|m| TextReplacement {
                        match_index: m.match_index,
                        match_length: m.match_length,
                        new_text: m.new_text.clone(),
                    })
                    .collect();

                let new_content = if used_fuzzy {
                    match apply_replacements_preserving_unchanged_lines(
                        &normalized_content,
                        &replacement_base,
                        &replacements,
                    ) {
                        Ok(s) => s,
                        Err(e) => {
                            return ToolResult::error(
                                format!("Failed to apply edits: {}", e),
                                "apply_failed",
                            )
                        }
                    }
                } else {
                    apply_replacements(&replacement_base, &replacements)
                };

                // no_change 检查
                if normalized_content == new_content {
                    let msg = if is_single {
                        format!(
                            "No changes made to {}. The replacement produced identical content.",
                            file_path
                        )
                    } else {
                        format!(
                            "No changes made to {}. The replacements produced identical content.",
                            file_path
                        )
                    };
                    return ToolResult::error(msg, "no_change");
                }

                // 写前检查取消
                if let Some(token) = &cancellation {
                    if token.is_cancelled() {
                        return ToolResult::error("Operation aborted", "cancelled");
                    }
                }

                let final_content = if has_bom {
                    format!("\u{FEFF}{}", restore_line_endings(&new_content, line_ending))
                } else {
                    restore_line_endings(&new_content, line_ending)
                };

                match std::fs::write(&path, &final_content) {
                    Ok(_) => {
                        let rel_path = relative_path_str(&path, &workspace);
                        // diff 用 LF 归一化后的内容（干净，不受 CRLF/BOM 干扰）
                        let diff =
                            generate_unified_diff(&normalized_content, &new_content, &rel_path);
                        let affected = vec![rel_path];
                        ToolResult::success(format!(
                            "Successfully edited {}. Replaced {} block(s).",
                            file_path,
                            matched_edits.len()
                        ))
                        .with_diff(diff, affected)
                        .with_metadata(json!({
                            "change_type": "edit",
                            "old_size": raw_content.len(),
                            "new_size": final_content.len(),
                            "edits_applied": matched_edits.len(),
                            "used_fuzzy_match": used_fuzzy,
                        }))
                    }
                    Err(e) => ToolResult::error(
                        format!("Failed to write file: {}", e),
                        format!("{}", e),
                    ),
                }
            })
            .await
    }
}
