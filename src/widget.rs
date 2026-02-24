use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, StatefulWidget};

use crate::node::NodeKind;
use crate::state::{EditMode, TreeState};

pub struct SchemaTree<'a> {
    title: Option<&'a str>,
    border: bool,
    highlight_style: Style,
    key_style: Style,
    value_style: Style,
    edit_style: Style,
    disabled_style: Style,
}

impl<'a> Default for SchemaTree<'a> {
    fn default() -> Self {
        Self {
            title: None,
            border: true,
            highlight_style: Style::default().bg(Color::DarkGray),
            key_style: Style::default().fg(Color::Cyan),
            value_style: Style::default().fg(Color::White),
            edit_style: Style::default().fg(Color::Yellow).bg(Color::DarkGray),
            disabled_style: Style::default().fg(Color::DarkGray),
        }
    }
}

impl<'a> SchemaTree<'a> {
    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }

    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = style;
        self
    }

    pub fn key_style(mut self, style: Style) -> Self {
        self.key_style = style;
        self
    }

    pub fn value_style(mut self, style: Style) -> Self {
        self.value_style = style;
        self
    }

    pub fn edit_style(mut self, style: Style) -> Self {
        self.edit_style = style;
        self
    }

    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }
}

impl<'a> StatefulWidget for SchemaTree<'a> {
    type State = TreeState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut TreeState) {
        let inner = if self.border {
            let mut block = Block::default().borders(Borders::ALL);
            if let Some(t) = self.title {
                block = block.title(t);
            }
            let inner = block.inner(area);
            block.render(area, buf);
            inner
        } else {
            area
        };

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let height = inner.height as usize;

        // Adjust scroll so selected is visible (before borrowing visible nodes)
        if state.selected < state.scroll_offset {
            state.scroll_offset = state.selected;
        }
        if state.selected >= state.scroll_offset + height {
            state.scroll_offset = state.selected - height + 1;
        }

        let visible = state.visible_nodes();

        // Render help line at bottom if there's room
        let (tree_height, help_area) = if inner.height >= 3 {
            (
                inner.height - 1,
                Some(Rect {
                    x: inner.x,
                    y: inner.y + inner.height - 1,
                    width: inner.width,
                    height: 1,
                }),
            )
        } else {
            (inner.height, None)
        };

        // Store cursor position for edit mode
        let mut cursor_pos: Option<(u16, u16)> = None;

        for i in 0..tree_height as usize {
            let idx = state.scroll_offset + i;
            let Some(vn) = visible.get(idx) else {
                break;
            };
            let node = vn.node;
            let y = inner.y + i as u16;
            let is_selected = idx == state.selected;
            let is_disabled = !state.is_enabled(&vn.path);

            // Resolve styles: disabled nodes use dimmed style
            let ks = if is_disabled { self.disabled_style } else { self.key_style };
            let vs = if is_disabled { self.disabled_style } else { self.value_style };

            // Build the line
            let indent = "  ".repeat(node.depth);

            // Render
            let mut x = inner.x;
            let max_x = inner.x + inner.width;

            // Fill background for selected row
            if is_selected {
                for col in inner.x..max_x {
                    buf[(col, y)].set_style(self.highlight_style);
                }
            }

            // Indent
            x = render_str(buf, x, y, max_x, &indent, Style::default());

            // Items with left-aligned indicators: render indicator then key, no ": value"
            match &node.kind {
                NodeKind::Bool => {
                    let checked = node.value.as_bool().unwrap_or(false);
                    let indicator = if checked { "■ " } else { "□ " };
                    x = render_str(buf, x, y, max_x, indicator, vs);
                    render_str(buf, x, y, max_x, &node.key, ks);
                    continue;
                }
                NodeKind::RadioItem { selected } => {
                    let indicator = if *selected { "● " } else { "○ " };
                    x = render_str(buf, x, y, max_x, indicator, vs);
                    render_str(buf, x, y, max_x, &node.key, ks);
                    continue;
                }
                NodeKind::CheckboxItem { checked } => {
                    let indicator = if *checked { "■ " } else { "□ " };
                    x = render_str(buf, x, y, max_x, indicator, vs);
                    render_str(buf, x, y, max_x, &node.key, ks);
                    continue;
                }
                _ => {}
            }

            // Expand/collapse marker for containers
            let marker = match &node.kind {
                NodeKind::Struct { .. }
                | NodeKind::RadioGroup { .. }
                | NodeKind::Checkboxes { .. } => {
                    if state.expanded.contains(&vn.path) {
                        "▼ "
                    } else {
                        "▶ "
                    }
                }
                NodeKind::Option { is_some: true, .. } if !node.children.is_empty() => {
                    if state.expanded.contains(&vn.path) {
                        "▼ "
                    } else {
                        "▶ "
                    }
                }
                _ => "  ",
            };

            x = render_str(buf, x, y, max_x, marker, ks);

            // Key
            x = render_str(buf, x, y, max_x, &node.key, ks);
            x = render_str(buf, x, y, max_x, ": ", Style::default());

            // Value (with edit mode handling)
            if is_selected && !is_disabled && matches!(&state.edit_mode, EditMode::Editing { .. }) {
                if let EditMode::Editing {
                    buffer, cursor_pos: cpos,
                } = &state.edit_mode
                {
                    let edit_x = x;
                    let _ = render_str(buf, x, y, max_x, buffer, self.edit_style);
                    // Calculate cursor screen position
                    let cursor_offset = buffer[..*cpos].chars().count() as u16;
                    cursor_pos = Some((edit_x + cursor_offset, y));
                    // Render a cursor indicator
                    let cursor_screen_x = edit_x + cursor_offset;
                    if cursor_screen_x < max_x {
                        let cursor_char = if *cpos < buffer.len() {
                            buffer[*cpos..].chars().next().unwrap_or(' ')
                        } else {
                            ' '
                        };
                        buf[(cursor_screen_x, y)].set_char(cursor_char);
                        buf[(cursor_screen_x, y)]
                            .set_style(Style::default().fg(Color::Black).bg(Color::Yellow));
                    }
                }
            } else {
                let value_str = format_value(node);
                render_str(buf, x, y, max_x, &value_str, vs);
            }
        }

        // Store cursor position in state for the frame
        state.cursor_position = cursor_pos;

        // Render help line
        if let Some(help_rect) = help_area {
            let help_text = match &state.edit_mode {
                EditMode::Normal => "↑↓ navigate  ←→ collapse/expand  Enter edit  Space toggle  q quit",
                EditMode::Editing { .. } => "Enter confirm  Esc cancel  ←→ move cursor",
            };
            render_str(
                buf,
                help_rect.x,
                help_rect.y,
                help_rect.x + help_rect.width,
                help_text,
                Style::default().fg(Color::DarkGray),
            );
        }
    }
}

fn format_value(node: &crate::node::ConfigNode) -> String {
    match &node.kind {
        NodeKind::String => node.value.as_str().unwrap_or("").to_string(),
        NodeKind::Integer => match &node.value {
            serde_json::Value::Number(n) => n.to_string(),
            _ => "0".to_string(),
        },
        NodeKind::Float => match &node.value {
            serde_json::Value::Number(n) => n.to_string(),
            _ => "0.0".to_string(),
        },
        NodeKind::Bool => {
            let b = node.value.as_bool().unwrap_or(false);
            b.to_string()
        }
        NodeKind::Option {
            is_some,
            inner_kind,
        } => {
            if !is_some {
                "None".to_string()
            } else if inner_kind.is_some() {
                // Scalar option: show Some(value) inline
                let val = match inner_kind.as_deref() {
                    Some(NodeKind::String) => {
                        node.value.as_str().unwrap_or("").to_string()
                    }
                    Some(NodeKind::Integer | NodeKind::Float) => match &node.value {
                        serde_json::Value::Number(n) => n.to_string(),
                        _ => "0".to_string(),
                    },
                    Some(NodeKind::Bool) => {
                        node.value.as_bool().unwrap_or(false).to_string()
                    }
                    _ => node.value.to_string(),
                };
                format!("Some({})", val)
            } else {
                // Struct option: no extra label, expand marker suffices
                String::new()
            }
        }
        NodeKind::RadioGroup { .. } => {
            // Show the currently selected variant
            node.children
                .iter()
                .find(|c| matches!(c.kind, NodeKind::RadioItem { selected: true }))
                .map(|c| c.key.clone())
                .unwrap_or_else(|| "?".to_string())
        }
        NodeKind::RadioItem { .. } | NodeKind::CheckboxItem { .. } => {
            // Rendered inline by the main loop; should not reach here
            String::new()
        }
        NodeKind::Struct { .. } => String::new(),
        NodeKind::Checkboxes { variants } => {
            let checked_count = node
                .children
                .iter()
                .filter(|c| matches!(c.kind, NodeKind::CheckboxItem { checked: true }))
                .count();
            format!("[{}/{}]", checked_count, variants.len())
        }
    }
}

fn render_str(buf: &mut Buffer, x: u16, y: u16, max_x: u16, s: &str, style: Style) -> u16 {
    let mut cx = x;
    for ch in s.chars() {
        if cx >= max_x {
            break;
        }
        buf[(cx, y)].set_char(ch);
        buf[(cx, y)].set_style(style);
        cx += 1;
    }
    cx
}
