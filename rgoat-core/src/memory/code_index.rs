//! 代码语义索引 — 将代码库分块并建立向量索引
//!
//! 使用 tree-sitter 解析代码结构（函数/结构体/模块），
//! 对每个代码块生成嵌入向量，存入 Zvec 供语义搜索。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::embedding::Embedder;
use super::vector_store::VectorMemory;

/// 代码块类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Module,
    Constant,
    TypeAlias,
    Other,
}

impl std::fmt::Display for CodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodeKind::Function => write!(f, "function"),
            CodeKind::Struct => write!(f, "struct"),
            CodeKind::Enum => write!(f, "enum"),
            CodeKind::Trait => write!(f, "trait"),
            CodeKind::Impl => write!(f, "impl"),
            CodeKind::Module => write!(f, "module"),
            CodeKind::Constant => write!(f, "constant"),
            CodeKind::TypeAlias => write!(f, "type_alias"),
            CodeKind::Other => write!(f, "other"),
        }
    }
}

/// 代码块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeChunk {
    pub kind: CodeKind,
    pub name: String,
    pub file: PathBuf,
    pub line_start: usize,
    pub line_end: usize,
    pub content: String,
    pub language: String,
}

/// 代码索引器
pub struct CodeIndexer {
    memory: VectorMemory,
    embedder: Arc<dyn Embedder>,
    /// 要忽略的目录/文件
    ignore_patterns: Vec<String>,
}

impl CodeIndexer {
    pub fn new(memory: VectorMemory, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            memory,
            embedder,
            ignore_patterns: vec![
                ".git".to_string(),
                "node_modules".to_string(),
                "target".to_string(),
                "dist".to_string(),
                "__pycache__".to_string(),
                ".venv".to_string(),
                "build".to_string(),
            ],
        }
    }

    /// Add ignore pattern
    pub fn add_ignore(&mut self, pattern: &str) {
        self.ignore_patterns.push(pattern.to_string());
    }

    /// 索引整个项目
    pub async fn index_project(
        &self,
        project_path: &Path,
    ) -> Result<IndexStats, CodeIndexError> {
        let walker = ignore::WalkBuilder::new(project_path)
            .hidden(false)
            .git_ignore(true)
            .build();

        let mut stats = IndexStats::default();

        for entry in walker {
            let entry = entry.map_err(|e| CodeIndexError::Io(e.to_string()))?;
            if !entry.file_type().map_or(false, |ft| ft.is_file()) {
                continue;
            }

            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            // Determine language from extension
            let language = match ext {
                "rs" => "rust",
                "py" => "python",
                "js" | "jsx" => "javascript",
                "ts" | "tsx" => "typescript",
                "go" => "go",
                "java" => "java",
                "cpp" | "cxx" | "cc" => "cpp",
                "c" => "c",
                _ => continue, // Skip unsupported languages
            };

            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Chunk the file into code blocks
            let chunks = self.chunk_code(path, &content, language);
            stats.files += 1;

            for chunk in chunks {
                // Generate embedding for the chunk
                let embedding = match self.embedder.embed(&chunk.content).await {
                    Ok(e) => e.vector,
                    Err(_) => continue,
                };

                // Store in vector memory
                match self.memory.index_code(
                    embedding,
                    &chunk.file.to_string_lossy(),
                    &chunk.name,
                    &chunk.kind.to_string(),
                    &chunk.content.chars().take(200).collect::<String>(),
                ).await {
                    Ok(_) => stats.chunks += 1,
                    Err(e) => {
                        tracing::warn!("Failed to index chunk: {}", e);
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Chunk a single file into code blocks using tree-sitter
    fn chunk_code(&self, path: &Path, content: &str, language: &str) -> Vec<CodeChunk> {
        let lines: Vec<&str> = content.lines().collect();
        let mut chunks = Vec::new();

        // Simple heuristic-based chunking (tree-sitter integration would be more precise)
        let mut current_kind = CodeKind::Other;
        let mut current_name = String::new();
        let mut chunk_start = 0usize;
        let mut brace_depth: i32 = 0;
        let mut in_block = false;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Detect function/struct/enum/impl starts
            if trimmed.starts_with("fn ") && !in_block {
                current_kind = CodeKind::Function;
                current_name = trimmed
                    .trim_start_matches("fn ")
                    .split(|c: char| c == '(' || c == '<' || c == '{')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
                in_block = true;
            } else if trimmed.starts_with("pub fn ") && !in_block {
                current_kind = CodeKind::Function;
                current_name = trimmed
                    .trim_start_matches("pub fn ")
                    .split(|c: char| c == '(' || c == '<' || c == '{')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
                in_block = true;
            } else if (trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ")) && !in_block {
                current_kind = CodeKind::Struct;
                current_name = trimmed
                    .trim_start_matches("pub struct ")
                    .trim_start_matches("struct ")
                    .split(|c: char| c == '<' || c == '(' || c == '{')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
                in_block = true;
            } else if trimmed.starts_with("enum ") || trimmed.starts_with("pub enum ") {
                current_kind = CodeKind::Enum;
                current_name = trimmed
                    .trim_start_matches("pub enum ")
                    .trim_start_matches("enum ")
                    .split(|c: char| c == '<' || c == '{')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
                in_block = true;
            } else if trimmed.starts_with("impl ") && !in_block {
                current_kind = CodeKind::Impl;
                current_name = trimmed
                    .trim_start_matches("impl ")
                    .split(|c: char| c == '<' || c == '{')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
                in_block = true;
            } else if trimmed.starts_with("trait ") || trimmed.starts_with("pub trait ") {
                current_kind = CodeKind::Trait;
                current_name = trimmed
                    .trim_start_matches("pub trait ")
                    .trim_start_matches("trait ")
                    .split(|c: char| c == '<' || c == '{')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
                in_block = true;
            } else if trimmed.starts_with("mod ") && !in_block {
                current_kind = CodeKind::Module;
                current_name = trimmed
                    .trim_start_matches("mod ")
                    .split(';')
                    .next()
                    .unwrap_or("unknown")
                    .trim()
                    .to_string();
                chunk_start = i;
            } else if trimmed.starts_with("class ") || trimmed.starts_with("def ") {
                // Python support
                if trimmed.starts_with("class ") {
                    current_kind = CodeKind::Struct;
                    current_name = trimmed
                        .trim_start_matches("class ")
                        .split(|c: char| c == '(' || c == ':')
                        .next()
                        .unwrap_or("unknown")
                        .trim()
                        .to_string();
                } else {
                    current_kind = CodeKind::Function;
                    current_name = trimmed
                        .trim_start_matches("def ")
                        .split('(')
                        .next()
                        .unwrap_or("unknown")
                        .trim()
                        .to_string();
                }
                chunk_start = i;
                in_block = true;
            }

            // Track brace depth
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;

            // End of block
            if in_block && brace_depth <= 0 {
                // Python: function end is at `dedent` (line starting at column 0)
                if language == "python" && i + 1 < lines.len() {
                    let next = lines[i + 1];
                    if !next.is_empty() && !next.starts_with(' ') && !next.starts_with('\t') {
                        finish_chunk(&mut chunks, &lines, &path, &current_kind, &current_name, chunk_start, i, language);
                        in_block = false;
                        brace_depth = 0;
                    }
                } else {
                    finish_chunk(&mut chunks, &lines, &path, &current_kind, &current_name, chunk_start, i, language);
                    in_block = false;
                    brace_depth = 0;
                }
            }
        }

        // Handle last unfinished block
        if in_block {
            let end = lines.len().saturating_sub(1);
            finish_chunk(&mut chunks, &lines, &path, &current_kind, &current_name, chunk_start, end, language);
        }

        chunks
    }
}

fn finish_chunk(
    chunks: &mut Vec<CodeChunk>,
    lines: &[&str],
    path: &Path,
    kind: &CodeKind,
    name: &str,
    start: usize,
    end: usize,
    language: &str,
) {
    let content = lines[start..=end].join("\n");
    // Only index chunks with reasonable size
    if content.len() > 10 && content.len() < 10000 {
        chunks.push(CodeChunk {
            kind: kind.clone(),
            name: name.to_string(),
            file: path.to_path_buf(),
            line_start: start + 1,
            line_end: end + 1,
            content,
            language: language.to_string(),
        });
    }
}

// ============================================================================
// 统计与错误
// ============================================================================

/// 索引统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub files: usize,
    pub chunks: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum CodeIndexError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Embed error: {0}")]
    Embed(String),
}
