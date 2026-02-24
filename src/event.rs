use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::node::NodeKind;
use crate::state::{EditMode, TreeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    Consumed { value_changed: bool },
    Ignored,
}

pub fn handle_key_event(state: &mut TreeState, event: KeyEvent) -> EventResult {
    match &state.edit_mode {
        EditMode::Editing { .. } => handle_editing_key(state, event),
        EditMode::Normal => handle_normal_key(state, event),
    }
}

fn handle_normal_key(state: &mut TreeState, event: KeyEvent) -> EventResult {
    // Ignore key events with modifiers (except shift)
    if event
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return EventResult::Ignored;
    }

    match event.code {
        KeyCode::Up | KeyCode::Char('k') => {
            state.select_prev();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.select_next();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            state.expand_selected();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            state.collapse_selected();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Enter => {
            let visible = state.visible_nodes();
            let Some(vn) = visible.get(state.selected) else {
                return EventResult::Ignored;
            };
            let kind = vn.node.kind.clone();
            let path = vn.path.clone();
            drop(visible);

            // Navigation (expand/collapse) always allowed; mutations require enabled
            match kind {
                NodeKind::Struct { .. }
                | NodeKind::RadioGroup { .. }
                | NodeKind::Checkboxes { .. } => {
                    state.toggle_expand();
                    EventResult::Consumed {
                        value_changed: false,
                    }
                }
                _ if !state.is_enabled(&path) => EventResult::Ignored,
                NodeKind::Bool => {
                    state.toggle_bool();
                    EventResult::Consumed {
                        value_changed: true,
                    }
                }
                NodeKind::Option {
                    is_some,
                    inner_kind: Some(_),
                } => {
                    if !is_some {
                        state.toggle_option();
                    }
                    state.start_edit();
                    EventResult::Consumed {
                        value_changed: !is_some,
                    }
                }
                NodeKind::Option { .. } => {
                    state.toggle_option();
                    EventResult::Consumed {
                        value_changed: true,
                    }
                }
                NodeKind::String | NodeKind::Integer | NodeKind::Float => {
                    state.start_edit();
                    EventResult::Consumed {
                        value_changed: false,
                    }
                }
                NodeKind::RadioItem { .. } => {
                    let changed = state.select_radio();
                    EventResult::Consumed {
                        value_changed: changed,
                    }
                }
                NodeKind::CheckboxItem { .. } => {
                    state.toggle_checkbox();
                    EventResult::Consumed {
                        value_changed: true,
                    }
                }
            }
        }
        KeyCode::Char(' ') => {
            let visible = state.visible_nodes();
            let Some(vn) = visible.get(state.selected) else {
                return EventResult::Ignored;
            };
            let kind = vn.node.kind.clone();
            let path = vn.path.clone();
            drop(visible);

            // Navigation always allowed; mutations require enabled
            match kind {
                NodeKind::Struct { .. }
                | NodeKind::RadioGroup { .. }
                | NodeKind::Checkboxes { .. } => {
                    state.toggle_expand();
                    EventResult::Consumed {
                        value_changed: false,
                    }
                }
                _ if !state.is_enabled(&path) => EventResult::Ignored,
                NodeKind::Bool => {
                    state.toggle_bool();
                    EventResult::Consumed {
                        value_changed: true,
                    }
                }
                NodeKind::Option { .. } => {
                    state.toggle_option();
                    EventResult::Consumed {
                        value_changed: true,
                    }
                }
                NodeKind::RadioItem { .. } => {
                    let changed = state.select_radio();
                    EventResult::Consumed {
                        value_changed: changed,
                    }
                }
                NodeKind::CheckboxItem { .. } => {
                    state.toggle_checkbox();
                    EventResult::Consumed {
                        value_changed: true,
                    }
                }
                _ => EventResult::Ignored,
            }
        }
        KeyCode::Delete | KeyCode::Backspace => {
            let visible = state.visible_nodes();
            let Some(vn) = visible.get(state.selected) else {
                return EventResult::Ignored;
            };
            let is_some_option = matches!(
                vn.node.kind,
                NodeKind::Option {
                    is_some: true,
                    ..
                }
            );
            let path = vn.path.clone();
            drop(visible);

            if is_some_option && state.is_enabled(&path) {
                state.toggle_option();
                EventResult::Consumed {
                    value_changed: true,
                }
            } else {
                EventResult::Ignored
            }
        }
        _ => EventResult::Ignored,
    }
}

fn handle_editing_key(state: &mut TreeState, event: KeyEvent) -> EventResult {
    match event.code {
        KeyCode::Esc => {
            state.cancel_edit();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Enter => {
            let changed = state.confirm_edit();
            EventResult::Consumed {
                value_changed: changed,
            }
        }
        KeyCode::Backspace => {
            state.edit_backspace();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Delete => {
            state.edit_delete();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Left => {
            state.edit_cursor_left();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Right => {
            state.edit_cursor_right();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Home => {
            state.edit_cursor_home();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::End => {
            state.edit_cursor_end();
            EventResult::Consumed {
                value_changed: false,
            }
        }
        KeyCode::Char(c) => {
            state.edit_insert_char(c);
            EventResult::Consumed {
                value_changed: false,
            }
        }
        _ => EventResult::Ignored,
    }
}
