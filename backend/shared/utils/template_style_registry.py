"""
模板样式注册表
从任意 .docx 模板中提取完整的样式定义，输出结构化的样式名→格式映射。

三层提取策略（复用 TemplateFormatExtractor 的核心逻辑）：
  Layer 1: document.styles — 样式定义（优先级最高）
  Layer 2: document.paragraphs — 段落实例（捕获手动覆盖）
  Layer 3: XML 底层 — w:eastAsia、w:firstLineChars 等 python-docx 不暴露的属性

与 TemplateFormatExtractor 的区别：
  - 不限于标题1-6，提取所有段落样式
  - 输出样式名→样式定义映射（保留样式名）
  - 生成 AI 提示词，用于指导 AI 生成结构化内容
"""

import re
import os
import json
import logging
from collections import Counter
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Tuple

from docx import Document
from docx.enum.text import WD_LINE_SPACING, WD_ALIGN_PARAGRAPH
from docx.oxml.ns import qn
from docx.shared import Pt

logger = logging.getLogger(__name__)


# ──────────────────────────── 样式名匹配 ────────────────────────────

HEADING_LEVEL_PATTERNS: Dict[int, List[str]] = {
    1: ["标题 1", "标题1", "Heading 1", "heading 1", "一级标题"],
    2: ["标题 2", "标题2", "Heading 2", "heading 2", "二级标题"],
    3: ["标题 3", "标题3", "Heading 3", "heading 3", "三级标题"],
    4: ["标题 4", "标题4", "Heading 4", "heading 4", "四级标题"],
    5: ["标题 5", "标题5", "Heading 5", "heading 5", "五级标题"],
    6: ["标题 6", "标题6", "Heading 6", "heading 6", "六级标题"],
}

BODY_STYLE_PATTERNS = ["正文", "Normal", "normal", "Body", "body", "默认段落字体"]
LIST_STYLE_PATTERNS = ["List Paragraph", "列表段落", "List Bullet", "List Number"]
QUOTE_STYLE_PATTERNS = ["Quote", "引用", "Intense Quote"]
CAPTION_STYLE_PATTERNS = ["Caption", "题注"]
TOC_STYLE_PATTERNS = ["TOC Heading", "toc 1", "toc 2", "toc 3"]


def _match_heading_level(style_name: str) -> Optional[int]:
    """从样式名推断标题级别，支持中英文"""
    for level, patterns in HEADING_LEVEL_PATTERNS.items():
        for pat in patterns:
            if pat == style_name:
                return level
    # 兜底：从 Heading N 格式提取数字
    m = re.search(r'Heading\s*(\d+)', style_name, re.IGNORECASE)
    if m:
        return int(m.group(1))
    m = re.search(r'标题\s*(\d+)', style_name)
    if m:
        return int(m.group(1))
    return None


def _classify_style(style_name: str) -> Tuple[str, int]:
    """
    分类样式语义类型
    
    Returns:
        (semantic_type, level): semantic_type 为 heading1-6, body, list, quote, caption, toc, unknown
    """
    level = _match_heading_level(style_name)
    if level is not None:
        return f"heading{level}", level
    
    for pattern in BODY_STYLE_PATTERNS:
        if pattern == style_name:
            return "body", 0
    
    for pattern in LIST_STYLE_PATTERNS:
        if pattern in style_name:
            return "list", 0
    
    for pattern in QUOTE_STYLE_PATTERNS:
        if pattern in style_name:
            return "quote", 0
    
    for pattern in CAPTION_STYLE_PATTERNS:
        if pattern in style_name:
            return "caption", 0
    
    for pattern in TOC_STYLE_PATTERNS:
        if pattern in style_name:
            return "toc", 0
    
    return "unknown", 0


# ──────────────────────────── 数据结构 ────────────────────────────

@dataclass
class FontInfo:
    """字体信息（从 XML 底层读取）"""
    east_asia: Optional[str] = None    # 中文字体
    ascii: Optional[str] = None        # 西文字体
    size_pt: Optional[float] = None    # 字号（磅）
    bold: Optional[bool] = None
    italic: Optional[bool] = None
    color_rgb: Optional[List[int]] = None


@dataclass
class ParagraphInfo:
    """段落格式"""
    alignment: Optional[int] = None           # 0=left, 1=center, 2=right, 3=justify
    line_spacing: Optional[float] = None      # 行间距（倍数）
    first_line_indent_chars: Optional[int] = None  # 首行缩进字符数
    space_before_pt: Optional[float] = None
    space_after_pt: Optional[float] = None


@dataclass
class StyleDefinition:
    """单个样式的完整定义"""
    name: str                          # 样式名（如 "标题 1", "Normal"）
    semantic_type: str                 # 语义类型：heading1-6, body, list, quote, caption, toc, unknown
    level: int = 0                     # 标题级别（仅 heading 类型有效）
    font: FontInfo = field(default_factory=FontInfo)
    paragraph: ParagraphInfo = field(default_factory=ParagraphInfo)
    is_builtin: bool = True            # 是否为 Word 内置样式


@dataclass
class PageSettingsInfo:
    """页面设置"""
    page_width: float = 21.0           # cm
    page_height: float = 29.7          # cm
    margin_top: float = 2.54           # cm
    margin_bottom: float = 2.54        # cm
    margin_left: float = 3.18          # cm
    margin_right: float = 3.18         # cm
    orientation: str = "portrait"      # portrait/landscape


@dataclass
class HeaderFooterInfo:
    """页眉页脚设置"""
    header_text: Optional[str] = None
    footer_text: Optional[str] = None
    header_alignment: int = 1          # 0=left, 1=center, 2=right
    footer_alignment: int = 1
    show_page_number: bool = True
    page_number_format: str = "第 {page} 页"


@dataclass
class TemplateStyleRegistry:
    """模板的完整样式注册表"""
    styles: Dict[str, StyleDefinition] = field(default_factory=dict)
    page_settings: PageSettingsInfo = field(default_factory=PageSettingsInfo)
    header_footer: HeaderFooterInfo = field(default_factory=HeaderFooterInfo)
    special_sections: List[str] = field(default_factory=list)
    ai_prompt: str = ""
    
    # 便捷属性：语义类型 → 样式名映射
    heading_names: Dict[int, str] = field(default_factory=dict)
    body_name: str = "Normal"
    list_name: str = "Normal"
    quote_name: str = "Normal"


# ──────────────────────────── 提取器 ────────────────────────────

class TemplateStyleExtractor:
    """模板样式提取器"""

    def __init__(self, template_path: str):
        self.template_path = template_path
        self.document: Optional[Document] = None
        self._registry: Optional[TemplateStyleRegistry] = None

    def extract(self) -> TemplateStyleRegistry:
        """主入口：提取模板样式，返回 TemplateStyleRegistry"""
        if self._registry is not None:
            return self._registry

        try:
            self.document = Document(self.template_path)
        except Exception as e:
            raise TemplateLoadError(f"无法加载模板文件: {e}")

        registry = TemplateStyleRegistry()
        
        # 1. 提取所有段落样式
        registry.styles = self._extract_all_styles()
        
        # 2. 提取页面设置
        registry.page_settings = self._extract_page_settings()
        
        # 3. 提取页眉页脚
        registry.header_footer = self._extract_header_footer()
        
        # 4. 检测特殊节
        registry.special_sections = self._detect_special_sections()
        
        # 5. 构建便捷映射
        self._build_convenience_mappings(registry)
        
        # 6. 生成 AI 提示词
        registry.ai_prompt = self._generate_ai_prompt(registry)
        
        self._registry = registry
        return registry

    def _extract_all_styles(self) -> Dict[str, StyleDefinition]:
        """提取所有段落样式"""
        styles = {}
        
        for style in self.document.styles:
            if style.type != 1:  # 只处理段落样式
                continue
            
            try:
                semantic_type, level = _classify_style(style.name)
                font_info = self._read_font_info(style)
                para_info = self._read_paragraph_info(style)
                
                styles[style.name] = StyleDefinition(
                    name=style.name,
                    semantic_type=semantic_type,
                    level=level,
                    font=font_info,
                    paragraph=para_info,
                    is_builtin=style.builtin if hasattr(style, 'builtin') else True
                )
            except Exception as e:
                logger.warning(f"提取样式 '{style.name}' 失败: {e}")
        
        return styles

    def _read_font_info(self, style) -> FontInfo:
        """从样式定义读取字体信息（含 XML 底层）"""
        info = FontInfo()
        
        # 沿继承链查找字体对
        current = style
        while current is not None:
            try:
                # 从 XML 读取 rFonts
                rPr = current.element.find(qn('w:rPr'))
                if rPr is not None:
                    rFonts = rPr.find(qn('w:rFonts'))
                    if rFonts is not None:
                        if info.east_asia is None:
                            info.east_asia = rFonts.get(qn('w:eastAsia'))
                        if info.ascii is None:
                            info.ascii = rFonts.get(qn('w:ascii'))
                            if info.ascii is None:
                                info.ascii = rFonts.get(qn('w:hAnsi'))
                
                # 从 python-docx 读取其他属性
                if info.size_pt is None and current.font.size:
                    try:
                        info.size_pt = current.font.size.pt
                    except:
                        pass
                
                if info.bold is None and current.font.bold is not None:
                    info.bold = current.font.bold
                
                if info.italic is None and current.font.italic is not None:
                    info.italic = current.font.italic
                
                if info.color_rgb is None and current.font.color and current.font.color.rgb:
                    try:
                        rgb = current.font.color.rgb
                        info.color_rgb = [rgb[0], rgb[1], rgb[2]]
                    except:
                        pass
                
                # 如果都找到了就停止
                if all([info.east_asia, info.ascii, info.size_pt is not None, 
                        info.bold is not None, info.italic is not None]):
                    break
                    
            except Exception:
                pass
            
            current = getattr(current, 'base_style', None)
        
        return info

    def _read_paragraph_info(self, style) -> ParagraphInfo:
        """从样式定义读取段落格式"""
        info = ParagraphInfo()
        
        pf = style.paragraph_format
        
        # 对齐
        if pf.alignment is not None:
            alignment_map = {
                WD_ALIGN_PARAGRAPH.LEFT: 0,
                WD_ALIGN_PARAGRAPH.CENTER: 1,
                WD_ALIGN_PARAGRAPH.RIGHT: 2,
                WD_ALIGN_PARAGRAPH.JUSTIFY: 3,
            }
            info.alignment = alignment_map.get(pf.alignment)
        
        # 行间距
        if pf.line_spacing is not None:
            rule = pf.line_spacing_rule
            if rule == WD_LINE_SPACING.MULTIPLE:
                info.line_spacing = pf.line_spacing
            elif rule in (WD_LINE_SPACING.EXACTLY, WD_LINE_SPACING.AT_LEAST):
                try:
                    info.line_spacing = round(pf.line_spacing.pt / 12.0, 2)
                except:
                    info.line_spacing = 1.5
            else:
                info.line_spacing = pf.line_spacing if isinstance(pf.line_spacing, (int, float)) else 1.5
        
        # 首行缩进（从 XML 读取字符数）
        info.first_line_indent_chars = self._read_first_line_chars(style)
        
        # 段前段后间距
        if pf.space_before is not None:
            try:
                info.space_before_pt = pf.space_before.pt
            except:
                pass
        if pf.space_after is not None:
            try:
                info.space_after_pt = pf.space_after.pt
            except:
                pass
        
        return info

    def _read_first_line_chars(self, style) -> Optional[int]:
        """从样式定义的 XML 读取字符缩进 (w:firstLineChars)"""
        current = style
        while current is not None:
            pPr = current.element.find(qn('w:pPr'))
            if pPr is not None:
                ind = pPr.find(qn('w:ind'))
                if ind is not None:
                    chars_attr = ind.get(qn('w:firstLineChars'))
                    if chars_attr is not None:
                        try:
                            return int(chars_attr) // 100
                        except ValueError:
                            pass
            current = getattr(current, 'base_style', None)
        return None

    def _extract_page_settings(self) -> PageSettingsInfo:
        """提取页面设置"""
        info = PageSettingsInfo()
        
        if not self.document.sections:
            return info
        
        section = self.document.sections[0]
        
        try:
            info.page_width = section.page_width.cm
            info.page_height = section.page_height.cm
            info.margin_top = section.top_margin.cm
            info.margin_bottom = section.bottom_margin.cm
            info.margin_left = section.left_margin.cm
            info.margin_right = section.right_margin.cm
            
            from docx.enum.section import WD_ORIENT
            info.orientation = "landscape" if section.orientation == WD_ORIENT.LANDSCAPE else "portrait"
        except Exception as e:
            logger.warning(f"提取页面设置失败: {e}")
        
        return info

    def _extract_header_footer(self) -> HeaderFooterInfo:
        """提取页眉页脚设置"""
        info = HeaderFooterInfo()
        
        if not self.document.sections:
            return info
        
        section = self.document.sections[0]
        
        try:
            # 页眉
            header = section.header
            if header.paragraphs and header.paragraphs[0].text.strip():
                info.header_text = header.paragraphs[0].text
                alignment_map = {
                    WD_ALIGN_PARAGRAPH.LEFT: 0,
                    WD_ALIGN_PARAGRAPH.CENTER: 1,
                    WD_ALIGN_PARAGRAPH.RIGHT: 2,
                }
                info.header_alignment = alignment_map.get(header.paragraphs[0].alignment, 1)
            
            # 页脚
            footer = section.footer
            if footer.paragraphs and footer.paragraphs[0].text.strip():
                info.footer_text = footer.paragraphs[0].text
                alignment_map = {
                    WD_ALIGN_PARAGRAPH.LEFT: 0,
                    WD_ALIGN_PARAGRAPH.CENTER: 1,
                    WD_ALIGN_PARAGRAPH.RIGHT: 2,
                }
                info.footer_alignment = alignment_map.get(footer.paragraphs[0].alignment, 1)
        except Exception as e:
            logger.warning(f"提取页眉页脚失败: {e}")
        
        return info

    def _detect_special_sections(self) -> List[str]:
        """检测特殊节类型"""
        special_keywords = {
            'abstract': ['摘要', 'abstract', '内容摘要'],
            'acknowledgement': ['致谢', 'acknowledgements', '感谢'],
            'reference': ['参考文献', 'references', '引用'],
            'toc': ['目录', 'table of contents', 'toc'],
        }
        
        detected = []
        
        for para in self.document.paragraphs:
            text = para.text.strip().lower()
            for section_type, keywords in special_keywords.items():
                if any(kw in text for kw in keywords):
                    if section_type not in detected:
                        detected.append(section_type)
        
        return detected

    def _build_convenience_mappings(self, registry: TemplateStyleRegistry):
        """构建便捷映射：语义类型 → 样式名"""
        for style_def in registry.styles.values():
            if style_def.semantic_type.startswith("heading"):
                level = style_def.level
                if level not in registry.heading_names:
                    registry.heading_names[level] = style_def.name
            elif style_def.semantic_type == "body":
                registry.body_name = style_def.name
            elif style_def.semantic_type == "list":
                registry.list_name = style_def.name
            elif style_def.semantic_type == "quote":
                registry.quote_name = style_def.name

    def _generate_ai_prompt(self, registry: TemplateStyleRegistry) -> str:
        """生成 AI 提示词，描述模板的样式信息"""
        lines = ["# 模板样式信息\n"]
        
        # 可用样式列表
        lines.append("## 可用样式")
        for style_def in registry.styles.values():
            if style_def.semantic_type != "unknown":
                lines.append(f"- {style_def.name} ({style_def.semantic_type})")
        
        # 标题样式映射
        lines.append("\n## 标题样式映射")
        for level, name in sorted(registry.heading_names.items()):
            lines.append(f"- 一级标题: {name}")
        
        # 正文样式
        lines.append(f"\n## 正文样式: {registry.body_name}")
        
        # 页面设置
        ps = registry.page_settings
        lines.append(f"\n## 页面设置")
        lines.append(f"- 纸张: {ps.page_width}cm x {ps.page_height}cm")
        lines.append(f"- 页边距: 上{ps.margin_top}cm 下{ps.margin_bottom}cm 左{ps.margin_left}cm 右{ps.margin_right}cm")
        
        return "\n".join(lines)

    def get_style_for_semantic(self, semantic_type: str, level: int = 0) -> Tuple[str, str]:
        """
        根据语义类型获取样式名，带退级策略
        
        Returns:
            (style_name, match_quality): match_quality 为 "exact", "fuzzy", "default"
        """
        available = set(self._registry.styles.keys()) if self._registry else set()
        
        # 1. 精确匹配
        if semantic_type.startswith("heading"):
            target_level = int(semantic_type[-1]) if semantic_type[-1].isdigit() else level
            for pattern in HEADING_LEVEL_PATTERNS.get(target_level, []):
                if pattern in available:
                    return pattern, "exact"
        
        # 2. 模糊匹配
        if semantic_type.startswith("heading"):
            target_level = int(semantic_type[-1]) if semantic_type[-1].isdigit() else level
            for name in available:
                if re.search(rf'(标题|Heading)\s*{target_level}', name, re.IGNORECASE):
                    return name, "fuzzy"
        
        # 3. 兜底到正文或 Normal
        for pattern in BODY_STYLE_PATTERNS:
            if pattern in available:
                return pattern, "default"
        
        return "Normal", "default"


# ──────────────────────────── 异常定义 ────────────────────────────

class TemplateLoadError(Exception):
    """模板加载失败"""
    pass


# ──────────────────────────── 便捷函数 ────────────────────────────

def extract_template_registry(template_path: str) -> TemplateStyleRegistry:
    """便捷函数：提取模板样式注册表"""
    extractor = TemplateStyleExtractor(template_path)
    return extractor.extract()


def find_style_name(template_doc: Document, semantic_type: str, level: int = 0) -> str:
    """
    在模板中查找最匹配的样式名（不依赖 TemplateStyleExtractor）
    
    Args:
        template_doc: 已加载的 Document 对象
        semantic_type: "heading1"-"heading6", "body", "list", "quote", etc.
        level: 标题级别（仅 heading 类型有效）
    
    Returns:
        匹配的样式名
    """
    available = {s.name for s in template_doc.styles if s.type == 1}
    
    # 提取标题级别
    if semantic_type.startswith("heading"):
        try:
            target_level = int(semantic_type[-1])
        except:
            target_level = level
        
        # 精确匹配
        for pattern in HEADING_LEVEL_PATTERNS.get(target_level, []):
            if pattern in available:
                return pattern
        
        # 模糊匹配
        for name in available:
            if re.search(rf'(标题|Heading)\s*{target_level}', name, re.IGNORECASE):
                return name
    
    # 正文
    if semantic_type == "body":
        for pattern in BODY_STYLE_PATTERNS:
            if pattern in available:
                return pattern
    
    # 列表
    if semantic_type == "list":
        for pattern in LIST_STYLE_PATTERNS:
            if pattern in available:
                return pattern
    
    # 引用
    if semantic_type == "quote":
        for pattern in QUOTE_STYLE_PATTERNS:
            if pattern in available:
                return pattern
    
    return "Normal"


def has_heading_styles(registry: TemplateStyleRegistry) -> bool:
    """检查模板是否有标题样式"""
    return len(registry.heading_names) > 0


if __name__ == "__main__":
    import sys
    
    if len(sys.argv) < 2:
        print("用法: python template_style_registry.py <template.docx>")
        sys.exit(1)
    
    template_path = sys.argv[1]
    
    try:
        registry = extract_template_registry(template_path)
        print(f"样式数量: {len(registry.styles)}")
        print(f"标题样式: {registry.heading_names}")
        print(f"正文样式: {registry.body_name}")
        print(f"页面设置: {registry.page_settings.page_width}cm x {registry.page_settings.page_height}cm")
        print(f"特殊节: {registry.special_sections}")
        print("\n--- AI 提示词 ---")
        print(registry.ai_prompt)
    except Exception as e:
        print(f"错误: {e}")
        sys.exit(1)
