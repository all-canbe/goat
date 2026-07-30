//! Settings dialog — modal for viewing/editing `~/.goat/setting.json`.
//!
//! 第三档 #15：参考 `approval_dialog.rs` 的模态渲染模式。
//! 显示常用 Settings 字段，支持数值字段 +/- 调整、Ctrl+S 保存、Esc 关闭。
//! 字符串字段（provider/model 等）只读展示，需用户手动编辑 JSON 后 /reload。

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::components::theme::Theme;

/// 单条设置项的种类。
#[derive(Debug, Clone)]
pub enum ItemKind {
    /// 可读字符串（如 provider/model 名）。展示用，本对话框不编辑。
    Text,
    /// 可调数值字段。`(min, max, step)` 限定范围。
    Number { min: i64, max: i64, step: i64 },
}

/// 单条设置项。
#[derive(Debug, Clone)]
pub struct SettingsItem {
    pub key: String,
    pub label: String,
    pub value: String,
    pub kind: ItemKind,
}

/// 对话框状态：条目列表 + 当前光标 + 是否处于"已修改未保存"状态。
#[derive(Debug, Clone)]
pub struct SettingsDialog {
    pub items: Vec<SettingsItem>,
    pub cursor: usize,
    pub dirty: bool,
}

impl SettingsDialog {
    pub fn new(items: Vec<SettingsItem>) -> Self {
        Self { items, cursor: 0, dirty: false }
    }

    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.items.len() {
            self.cursor += 1;
        }
    }

    /// 对当前项执行"+step"操作（仅对 Number 字段有效）。
    pub fn inc(&mut self) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            if let ItemKind::Number { min, max, step } = &item.kind {
                let cur: i64 = item.value.parse().unwrap_or(*min);
                let next = (cur + step).clamp(*min, *max);
                item.value = next.to_string();
                self.dirty = true;
            }
        }
    }

    /// 对当前项执行"-step"操作（仅对 Number 字段有效）。
    pub fn dec(&mut self) {
        if let Some(item) = self.items.get_mut(self.cursor) {
            if let ItemKind::Number { min, max, step } = &item.kind {
                let cur: i64 = item.value.parse().unwrap_or(*min);
                let next = (cur - step).clamp(*min, *max);
                item.value = next.to_string();
                self.dirty = true;
            }
        }
    }

    /// 居中矩形（70% × 60%）。
    pub fn dialog_area(screen: Rect) -> Rect {
        let width = (screen.width as f32 * 0.7) as u16;
        let height = (screen.height as f32 * 0.6) as u16;
        let x = screen.width.saturating_sub(width) / 2;
        let y = screen.height.saturating_sub(height) / 2;
        Rect::new(
            x.saturating_add(screen.x),
            y.saturating_add(screen.y),
            width,
            height,
        )
    }

    pub fn render(&self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        Clear.render(area, buf);

        let title = if self.dirty {
            "⚙ Settings (modified — Ctrl+S to save, Esc to discard)"
        } else {
            "⚙ Settings (Esc to close)"
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Theme::style_approval_border())
            .title(Line::from(vec![Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD),
            )]));
        let inner = block.inner(area);
        block.render(area, buf);

        // 标题行 + 列表 + 提示行
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(2)])
            .split(inner);

        // 顶部说明
        let header = Paragraph::new(vec![
            Line::from(Span::styled(
                "Edit numeric fields with +/- · String fields are read-only (edit JSON directly, then /reload)",
                Style::default().fg(Theme::DIFF_CTX),
            )),
        ])
        .wrap(Wrap { trim: true });
        header.render(chunks[0], buf);

        // 列表
        let mut lines: Vec<Line> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            let marker = if i == self.cursor { "▶ " } else { "  " };
            let value_style = if i == self.cursor {
                Style::default().fg(Theme::USER_COLOR).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Theme::ASSISTANT_COLOR)
            };
            let kind_hint = match &item.kind {
                ItemKind::Text => String::new(),
                ItemKind::Number { min, max, .. } => format!("  [{}..{}]", min, max),
            };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<22}", item.label), Style::default().fg(Theme::DIFF_CTX)),
                Span::styled(item.value.clone(), value_style),
                Span::styled(kind_hint, Style::default().fg(Theme::DIFF_CTX)),
            ]));
        }
        Paragraph::new(lines).render(chunks[1], buf);

        // 底部提示
        let footer = Paragraph::new(Line::from(vec![
            Span::styled("[↑/↓] Navigate   ", Style::default().fg(Theme::RISK_LOW).add_modifier(Modifier::BOLD)),
            Span::styled("[+/-] Adjust number   ", Style::default().fg(Theme::RISK_LOW).add_modifier(Modifier::BOLD)),
            Span::styled("[Ctrl+S] Save   ", Style::default().fg(Theme::DIFF_ADD).add_modifier(Modifier::BOLD)),
            Span::styled("[Esc] Close", Style::default().fg(Theme::DIFF_CTX)),
        ]));
        footer.render(chunks[2], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_items() -> Vec<SettingsItem> {
        vec![
            SettingsItem {
                key: "provider".into(),
                label: "Default Provider".into(),
                value: "aliyuncs".into(),
                kind: ItemKind::Text,
            },
            SettingsItem {
                key: "max_agent_turns".into(),
                label: "Max Agent Turns".into(),
                value: "50".into(),
                kind: ItemKind::Number { min: 1, max: 500, step: 1 },
            },
            SettingsItem {
                key: "max_concurrency".into(),
                label: "Max Concurrency".into(),
                value: "3".into(),
                kind: ItemKind::Number { min: 1, max: 16, step: 1 },
            },
        ]
    }

    #[test]
    fn move_up_down_within_bounds() {
        let mut d = SettingsDialog::new(sample_items());
        assert_eq!(d.cursor, 0);
        d.move_up();
        assert_eq!(d.cursor, 0, "already at top, no move");
        d.move_down();
        assert_eq!(d.cursor, 1);
        d.move_down();
        assert_eq!(d.cursor, 2);
        d.move_down();
        assert_eq!(d.cursor, 2, "already at bottom, no move");
    }

    #[test]
    fn inc_dec_only_affects_number_fields() {
        let mut d = SettingsDialog::new(sample_items());
        // cursor=0 is Text — inc should be a no-op
        d.inc();
        assert_eq!(d.items[0].value, "aliyuncs");
        assert!(!d.dirty);
        // cursor=1 is Number(50, max 500)
        d.move_down();
        d.inc();
        assert_eq!(d.items[1].value, "51");
        assert!(d.dirty);
        // dec back
        d.dec();
        assert_eq!(d.items[1].value, "50");
        // clamp to min
        for _ in 0..1000 {
            d.dec();
        }
        assert_eq!(d.items[1].value, "1", "clamped to min");
        // clamp to max
        for _ in 0..1000 {
            d.inc();
        }
        assert_eq!(d.items[1].value, "500", "clamped to max");
    }

    #[test]
    fn step_size_respected() {
        let mut items = sample_items();
        items.push(SettingsItem {
            key: "step_test".into(),
            label: "Step Test".into(),
            value: "10".into(),
            kind: ItemKind::Number { min: 0, max: 100, step: 5 },
        });
        let mut d = SettingsDialog::new(items);
        d.cursor = 3; // step_test
        d.inc();
        assert_eq!(d.items[3].value, "15");
        d.dec();
        d.dec();
        assert_eq!(d.items[3].value, "5");
    }
}
