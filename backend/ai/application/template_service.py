"""
模板驱动的文档生成服务
编排：模板分析 → AI 生成 → 文档注入（含完整退级机制）
"""

import os
import sys
import json
import logging
import shutil
from typing import Optional, List, Dict, Any
from dataclasses import dataclass, field

# 路径设置
_APP_DIR = os.path.dirname(os.path.abspath(__file__))
_BACKEND_DIR = os.path.abspath(os.path.join(_APP_DIR, '..', '..'))
if _BACKEND_DIR not in sys.path:
    sys.path.insert(0, _BACKEND_DIR)

from shared.utils.template_style_registry import TemplateStyleRegistry
from shared.utils.template_driven_generator import (
    TemplateDrivenGenerator,
    StructuredContent,
    ContentBlock,
    RunAnnotation,
    TableBlock,
)
from shared.utils.html_to_word import (
    convert_html_to_word,
    adjust_word_format,
    WordFormatConfig,
    DEFAULT_OUTPUT_DIR,
)
from shared.utils.template_format_extractor import extract_format_config

logger = logging.getLogger(__name__)

# ──────────────────────────── 退级配置 ────────────────────────────

@dataclass
class DegradationConfig:
    """退级配置"""
    allow_default_template: bool = True    # 是否允许使用默认模板
    allow_pandoc_fallback: bool = True     # 是否允许 Pandoc 退级
    allow_element_skip: bool = True        # 是否允许元素级跳过
    fail_on_degradation: bool = False      # 是否在退级时中断（测试环境）
    notify_threshold: int = 1              # 退级级别阈值（超过此级别通知用户）


# ──────────────────────────── 生成结果 ────────────────────────────

@dataclass
class ElementInjectionWarning:
    """元素注入警告"""
    element_type: str
    element_index: int
    reason: str


@dataclass
class GenerationResult:
    """生成结果"""
    output_path: str = ""
    degradation_level: int = 0       # 0=正常, 1=样式级, 2=元素级, 3=端到端退级
    used_pandoc_fallback: bool = False
    used_default_template: bool = False
    total_failure: bool = False
    warnings: List[str] = field(default_factory=list)
    element_warnings: List[ElementInjectionWarning] = field(default_factory=list)

    def to_dict(self) -> dict:
        return {
            "output_path": self.output_path,
            "degradation_level": self.degradation_level,
            "used_pandoc_fallback": self.used_pandoc_fallback,
            "used_default_template": self.used_default_template,
            "total_failure": self.total_failure,
            "warnings": self.warnings,
            "element_warnings": [
                {"type": w.element_type, "index": w.element_index, "reason": w.reason}
                for w in self.element_warnings
            ],
        }


# ──────────────────────────── 内置默认模板 ────────────────────────────

_TEMPLATES_DIR = os.path.join(os.path.dirname(__file__), '..', 'shared', 'utils', 'templates')
DEFAULT_TEMPLATE_PATH = os.path.join(_TEMPLATES_DIR, 'default_chinese_academic.docx')


def _ensure_default_template() -> Optional[str]:
    """检查内置默认模板是否存在"""
    if os.path.exists(DEFAULT_TEMPLATE_PATH):
        return DEFAULT_TEMPLATE_PATH
    logger.warning(f"内置默认模板不存在: {DEFAULT_TEMPLATE_PATH}")
    return None


# ──────────────────────────── 模板加载（Level 0 退级） ────────────────────────────

def load_template_with_fallback(template_path: str) -> tuple:
    """
    加载模板，失败时使用内置默认模板。

    Returns:
        (doc, is_default): 文档对象 + 是否使用了默认模板
    """
    from docx import Document

    # 先校验文件
    if not os.path.exists(template_path):
        raise FileNotFoundError(f"模板文件不存在: {template_path}")

    file_size = os.path.getsize(template_path)
    if file_size == 0:
        raise ValueError("模板文件大小为 0")

    try:
        doc = Document(template_path)
        if not doc.sections:
            raise ValueError("模板无 section 定义")
        return doc, False
    except Exception as e:
        logger.warning(f"模板加载失败: {e}")
        default_path = _ensure_default_template()
        if default_path:
            return Document(default_path), True
        else:
            logger.warning("内置默认模板不存在，创建空白文档")
            return Document(), True


# ──────────────────────────── 结构化内容转 HTML ────────────────────────────

def _render_structured_content_to_html(content: StructuredContent, registry: Optional[TemplateStyleRegistry] = None) -> str:
    """将结构化内容渲染为 HTML（供 Pandoc 退级路线使用）"""
    parts = [
        "<!DOCTYPE html>",
        "<html><head><meta charset='utf-8'>",
        "<style>",
        "  body { font-family: 'SimSun', 'Times New Roman'; font-size: 12pt; line-height: 1.5; }",
        "  h1 { font-family: 'SimHei'; font-size: 16pt; font-weight: bold; text-align: center; }",
        "  h2 { font-family: 'SimHei'; font-size: 14pt; font-weight: bold; }",
        "  h3 { font-family: 'SimHei'; font-size: 12pt; font-weight: bold; }",
        "  p { text-indent: 2em; text-align: justify; }",
        "  table { border-collapse: collapse; width: 100%; margin: 10pt 0; }",
        "  th, td { border: 1px solid #000; padding: 6pt; text-align: center; }",
        "  .caption { text-align: center; font-size: 10.5pt; margin: 6pt 0; }",
        "</style>",
        "</head><body>",
    ]

    if content.title:
        parts.append(f"<h1>{content.title}</h1>")

    tag_map = {
        "heading1": "h1", "heading2": "h2", "heading3": "h3",
        "heading4": "h4", "heading5": "h5", "heading6": "h6",
        "body": "p", "quote": "blockquote",
    }

    for block in content.blocks:
        tag = tag_map.get(block.semantic_type, "p")
        text = block.text
        # 应用 run 标注
        if block.runs:
            text = _apply_run_annotations_html(block.text, block.runs)
        parts.append(f"<{tag}>{text}</{tag}>")

    for table in content.tables:
        if table.caption:
            parts.append(f"<p class='caption'>{table.caption}</p>")
        parts.append("<table>")
        if table.headers:
            parts.append("<thead><tr>")
            for h in table.headers:
                parts.append(f"<th>{h}</th>")
            parts.append("</tr></thead>")
        parts.append("<tbody>")
        for row in table.rows:
            parts.append("<tr>")
            for cell in row:
                parts.append(f"<td>{cell}</td>")
            parts.append("</tr>")
        parts.append("</tbody></table>")

    parts.append("</body></html>")
    return "\n".join(parts)


def _apply_run_annotations_html(text: str, runs: list) -> str:
    """在 HTML 中应用 run 标注（加粗/斜体等）"""
    if not runs:
        return text

    # 按 start 排序
    sorted_runs = sorted(runs, key=lambda r: r.start)
    result = []
    last_end = 0

    for run in sorted_runs:
        # 添加前面的纯文本
        if run.start > last_end:
            result.append(text[last_end:run.start])

        segment = text[run.start:run.end]
        if run.bold:
            segment = f"<b>{segment}</b>"
        if run.italic:
            segment = f"<i>{segment}</i>"
        if run.superscript:
            segment = f"<sup>{segment}</sup>"
        if run.subscript:
            segment = f"<sub>{segment}</sub>"
        if run.color:
            hex_color = "#{:02x}{:02x}{:02x}".format(*run.color)
            segment = f'<span style="color:{hex_color}">{segment}</span>'
        result.append(segment)
        last_end = run.end

    # 尾部文本
    if last_end < len(text):
        result.append(text[last_end:])

    return "".join(result)


# ──────────────────────────── AI 提示词构建 ────────────────────────────

def _build_generation_prompt(
    registry: TemplateStyleRegistry,
    user_requirement: str,
    word_count: int,
) -> str:
    """构建包含模板样式信息的 AI 提示词"""
    available_styles = list(registry.styles.keys())

    # 构建语义→样式名映射描述
    style_mapping_lines = []
    for name, style_def in registry.styles.items():
        font_desc = f"{style_def.font.east_asia or '未知'} {style_def.font.size_pt or '?'}pt"
        style_mapping_lines.append(f"  - \"{name}\" → {style_def.semantic_type}（{font_desc}）")

    style_mapping = "\n".join(style_mapping_lines) if style_mapping_lines else "  （无可用样式）"

    return f"""你是一个专业的文档生成助手。请根据以下用户需求生成文档内容。

## 用户需求
{user_requirement}

## 目标字数
约 {word_count} 字

## 模板可用样式（必须使用这些精确的样式名）
{style_mapping}

## 样式使用规则
- 标题必须使用 heading 开头的 semantic_type（heading1/heading2/heading3 等）
- 正文段落使用 "body"
- 引用使用 "quote"

## 输出格式
请严格以 JSON 格式输出，结构如下：
{{
  "title": "文档标题",
  "blocks": [
    {{"semantic_type": "heading1", "text": "第一章 绪论"}},
    {{"semantic_type": "body", "text": "正文内容..."}},
    {{"semantic_type": "heading2", "text": "1.1 研究背景"}},
    {{"semantic_type": "body", "text": "正文内容..."}}
  ],
  "tables": [
    {{"caption": "表1 xxx", "headers": ["列1", "列2"], "rows": [["a", "b"]]}}
  ]
}}

重要：
1. 只输出 JSON，不要输出其他任何内容
2. semantic_type 只能是 heading1-6 / body / quote
3. 正文段落要充实，每个 body block 至少 100 字
4. 表格数据要合理完整
"""


def _parse_ai_response(raw_response: str) -> dict:
    """解析 AI 返回的 JSON（兼容 markdown 代码块等）"""
    text = raw_response.strip()

    # 去掉 markdown 代码块
    if text.startswith("```"):
        lines = text.splitlines()
        if lines[0].startswith("```"):
            lines = lines[1:]
        if lines and lines[-1].startswith("```"):
            lines = lines[:-1]
        text = "\n".join(lines).strip()

    # 找 JSON 边界
    first_idx = -1
    for char in ['{', '[']:
        idx = text.find(char)
        if idx != -1 and (first_idx == -1 or idx < first_idx):
            first_idx = idx

    if first_idx != -1:
        closing_char = '}' if text[first_idx] == '{' else ']'
        last_idx = text.rfind(closing_char)
        if last_idx != -1:
            text = text[first_idx:last_idx + 1]

    return json.loads(text)


def _dict_to_structured_content(data: dict) -> StructuredContent:
    """将 AI 返回的 JSON dict 转换为 StructuredContent"""
    blocks = []
    for block_data in data.get("blocks", []):
        runs = []
        for run_data in block_data.get("runs", []):
            runs.append(RunAnnotation(
                start=run_data.get("start", 0),
                end=run_data.get("end", 0),
                bold=run_data.get("bold", False),
                italic=run_data.get("italic", False),
                superscript=run_data.get("superscript", False),
                subscript=run_data.get("subscript", False),
                color=run_data.get("color"),
            ))
        blocks.append(ContentBlock(
            semantic_type=block_data.get("semantic_type", "body"),
            level=block_data.get("level", 0),
            text=block_data.get("text", ""),
            runs=runs,
        ))

    tables = []
    for table_data in data.get("tables", []):
        tables.append(TableBlock(
            caption=table_data.get("caption", ""),
            headers=table_data.get("headers", []),
            rows=table_data.get("rows", []),
        ))

    return StructuredContent(
        title=data.get("title", ""),
        blocks=blocks,
        tables=tables,
    )


# ──────────────────────────── 主服务 ────────────────────────────

class TemplateAnalysisService:
    """模板驱动的文档生成服务（含完整退级机制）"""

    def __init__(self, degradation_config: Optional[DegradationConfig] = None):
        self.config = degradation_config or DegradationConfig()

    # ── 公开接口 ──

    def analyze_template(self, template_path: str) -> dict:
        """分析模板，返回样式注册表 JSON"""
        try:
            doc, is_default = load_template_with_fallback(template_path)
            # 用实际加载的文档路径构建注册表
            if is_default:
                actual_path = _ensure_default_template() or template_path
            else:
                actual_path = template_path

            registry = TemplateStyleRegistry(actual_path)
            result = registry.build()
            return {
                "success": True,
                "is_default_template": is_default,
                "registry": result.model_dump(),
            }
        except Exception as e:
            logger.error(f"模板分析失败: {e}")
            return {"success": False, "error": str(e)}

    def generate_document(
        self,
        template_path: str,
        user_requirement: str,
        word_count: int = 15000,
        model_name: str = None,
    ) -> GenerationResult:
        """
        端到端文档生成（带完整退级机制）。

        Args:
            template_path: 用户上传的模板路径
            user_requirement: 用户需求描述
            word_count: 目标字数
            model_name: LLM 模型名（可选）

        Returns:
            GenerationResult: 包含输出路径、退级级别、警告信息
        """
        result = GenerationResult()
        output_path = os.path.join(DEFAULT_OUTPUT_DIR, "generated_paper.docx")
        os.makedirs(os.path.dirname(output_path), exist_ok=True)

        # ── Level 0: 加载模板 ──
        try:
            template_doc, is_default = load_template_with_fallback(template_path)
            if is_default:
                result.degradation_level = max(result.degradation_level, 0)
                result.used_default_template = True
                result.warnings.append("使用了内置默认模板")
                if self.config.fail_on_degradation:
                    return result
        except Exception as e:
            logger.error(f"Level 0 退级也失败: {e}")
            return self._total_failure(output_path, result, str(e))

        # ── Level 1: 构建样式注册表 ──
        registry = None
        try:
            actual_path = _ensure_default_template() if is_default else template_path
            registry = TemplateStyleRegistry(actual_path)
            registry.build()

            if not registry.has_heading_styles():
                result.degradation_level = max(result.degradation_level, 1)
                result.warnings.append("模板无标题样式，使用模糊匹配")
        except Exception as e:
            logger.warning(f"Level 1: 样式注册表构建失败，退级到 Pandoc: {e}")
            return self._fallback_to_pandoc(
                template_path, user_requirement, word_count, output_path, result,
                level=1, reason=str(e), model_name=model_name,
            )

        # ── 调用 AI 生成结构化内容 ──
        try:
            structured_content = self._generate_content_via_ai(
                registry, user_requirement, word_count, model_name
            )
        except Exception as e:
            logger.warning(f"AI 生成失败，退级到 Pandoc: {e}")
            return self._fallback_to_pandoc(
                template_path, user_requirement, word_count, output_path, result,
                level=1, reason=f"AI 生成失败: {e}", model_name=model_name,
            )

        # ── Level 2+3: 注入内容 ──
        try:
            generator = TemplateDrivenGenerator(template_doc, registry)
            element_warnings = generator.generate(structured_content, output_path)
            result.output_path = output_path
            result.element_warnings = element_warnings
            if element_warnings:
                result.degradation_level = max(result.degradation_level, 2)
        except Exception as e:
            logger.error(f"模板注入崩溃，退级到 Pandoc 路线: {e}")
            return self._fallback_to_pandoc(
                template_path, user_requirement, word_count, output_path, result,
                level=3, reason=str(e), model_name=model_name,
            )

        return result

    # ── AI 调用 ──

    def _generate_content_via_ai(
        self,
        registry: TemplateStyleRegistry,
        user_requirement: str,
        word_count: int,
        model_name: str = None,
    ) -> StructuredContent:
        """调用 AI 生成结构化内容"""
        from langchain_openai import ChatOpenAI
        from langchain_core.messages import HumanMessage, SystemMessage

        # 加载模型配置
        try:
            from ai.domain.services.agent.base_agent import ConfigLoader, create_llm
            config_loader = ConfigLoader.get_instance()
            if model_name:
                model_config = config_loader.get_model_config(model_name)
            else:
                model_config = config_loader.get_model_config("default")
            llm = create_llm(model_config)
        except Exception as e:
            logger.warning(f"加载模型配置失败，使用默认配置: {e}")
            llm = ChatOpenAI(
                model="deepseek-v4-flash",
                temperature=0.7,
                max_tokens=4096,
                openai_api_key="sk-K2WgPDogistUUeZ28LiuSvxoUnaWRLjqzfPRFUStn5tbiSsZOl0jAAVa7E0dgxQ3",
                base_url="https://opencode.ai/zen/go/v1",
                timeout=120,
            )

        prompt = _build_generation_prompt(registry, user_requirement, word_count)

        messages = [
            SystemMessage(content="你是一个专业的文档生成助手。严格以 JSON 格式输出，不要输出其他内容。"),
            HumanMessage(content=prompt),
        ]

        response = llm.invoke(messages)
        raw = response.content.strip()
        logger.info(f"AI 响应长度: {len(raw)} 字符")

        data = _parse_ai_response(raw)
        return _dict_to_structured_content(data)

    # ── 退级方法 ──

    def _fallback_to_pandoc(
        self,
        template_path: str,
        user_requirement: str,
        word_count: int,
        output_path: str,
        result: GenerationResult,
        level: int,
        reason: str,
        model_name: str = None,
    ) -> GenerationResult:
        """退级到 Pandoc 路线"""
        result.degradation_level = max(result.degradation_level, level)
        result.warnings.append(f"退级原因: {reason}")

        if not self.config.allow_pandoc_fallback:
            result.total_failure = True
            result.warnings.append("Pandoc 退级被禁用")
            return result

        try:
            # 先尝试用 AI 生成内容（即使模板注入失败，AI 内容可能仍有用）
            try:
                registry = TemplateStyleRegistry(template_path)
                registry.build()
                structured_content = self._generate_content_via_ai(
                    registry, user_requirement, word_count, model_name
                )
                html_content = _render_structured_content_to_html(structured_content, registry)
            except Exception as ai_e:
                logger.warning(f"Pandoc 退级中 AI 也失败，使用占位内容: {ai_e}")
                html_content = f"""<!DOCTYPE html>
<html><head><meta charset='utf-8'></head><body>
<h1>{user_requirement[:50]}</h1>
<p>文档内容生成过程中遇到问题。请检查模板文件后重试。</p>
<p>错误信息: {reason}</p>
</body></html>"""

            # 保存 HTML
            html_path = output_path.replace('.docx', '_temp.html')
            with open(html_path, 'w', encoding='utf-8') as f:
                f.write(html_content)

            # Pandoc 转换
            docx_path = convert_html_to_word(html_path, output_dir=os.path.dirname(output_path))

            # 尝试从模板提取页面设置
            try:
                format_config = extract_format_config(template_path)
                docx_path = adjust_word_format(docx_path, format_config)
            except Exception as fmt_e:
                logger.warning(f"页面设置提取失败，使用默认格式: {fmt_e}")

            result.output_path = docx_path
            result.used_pandoc_fallback = True

            # 清理临时 HTML
            try:
                os.remove(html_path)
            except:
                pass

        except Exception as e:
            logger.error(f"Pandoc 路线也失败: {e}")
            return self._total_failure(output_path, result, f"Pandoc 退级也失败: {e}")

        return result

    def _total_failure(
        self,
        output_path: str,
        result: GenerationResult,
        reason: str,
    ) -> GenerationResult:
        """最终兜底：创建空白文档"""
        result.total_failure = True
        result.warnings.append(f"完全失败: {reason}")

        try:
            from docx import Document
            doc = Document()
            doc.add_paragraph("文档生成失败")
            doc.add_paragraph(f"原因: {reason}")
            doc.add_paragraph("请检查模板文件是否有效，然后重试。")
            doc.save(output_path)
            result.output_path = output_path
        except Exception as e:
            logger.error(f"创建兜底文档也失败: {e}")
            result.warnings.append(f"兜底文档创建失败: {e}")

        return result


# ──────────────────────────── 便捷函数 ────────────────────────────

def analyze_template(template_path: str) -> dict:
    """便捷函数：分析模板"""
    service = TemplateAnalysisService()
    return service.analyze_template(template_path)


def generate_document(
    template_path: str,
    user_requirement: str,
    word_count: int = 15000,
) -> GenerationResult:
    """便捷函数：生成文档"""
    service = TemplateAnalysisService()
    return service.generate_document(template_path, user_requirement, word_count)
