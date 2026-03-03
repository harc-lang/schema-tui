use crate::filter::NodeFilter;
use crate::node::{build_tree, default_value_for_schema, ConfigNode, NodeKind};
use serde_json::{Map, Value};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditMode {
    Normal,
    Editing { buffer: String, cursor_pos: usize },
}

/// A reference to a visible node in the flattened tree, along with its
/// path (for expansion tracking) and index in the flat list.
pub struct VisibleNode<'a> {
    pub node: &'a ConfigNode,
    pub path: String,
    pub flat_index: usize,
}

pub struct TreeState {
    pub nodes: Vec<ConfigNode>,
    pub expanded: HashSet<String>,
    pub selected: usize,
    pub edit_mode: EditMode,
    pub scroll_offset: usize,
    /// Set by the widget during rendering; used by the application to
    /// position the terminal cursor during edit mode.
    pub cursor_position: Option<(u16, u16)>,
    defs: Map<String, Value>,
    filter: Option<Box<dyn NodeFilter>>,
}

impl TreeState {
    pub fn new(schema: &schemars::Schema, value: &Value) -> Self {
        let root = schema.as_value();
        let defs = root
            .get("$defs")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        let nodes = build_tree(schema, value);

        let mut expanded = HashSet::new();
        collect_expanded_paths(&nodes, &mut String::new(), &mut expanded);

        TreeState {
            nodes,
            expanded,
            selected: 0,
            edit_mode: EditMode::Normal,
            scroll_offset: 0,
            cursor_position: None,
            defs,
            filter: None,
        }
    }

    /// Set a filter to control which nodes are visible and editable.
    pub fn set_filter(&mut self, filter: impl NodeFilter + 'static) {
        self.filter = Some(Box::new(filter));
    }

    /// Remove the current filter.
    pub fn clear_filter(&mut self) {
        self.filter = None;
    }

    pub fn visible_nodes(&self) -> Vec<VisibleNode<'_>> {
        let mut result = Vec::new();
        collect_visible(
            &self.nodes,
            &mut String::new(),
            &self.expanded,
            self.filter.as_deref(),
            &mut result,
        );
        result
    }

    /// Check whether the node at the given path is enabled for editing.
    pub fn is_enabled(&self, path: &str) -> bool {
        match &self.filter {
            Some(f) => f.enabled(path),
            None => true,
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_next(&mut self) {
        let count = self.visible_nodes().len();
        if self.selected + 1 < count {
            self.selected += 1;
        }
    }

    pub fn toggle_expand(&mut self) {
        let visible = self.visible_nodes();
        if let Some(vn) = visible.get(self.selected) {
            let has_children = !vn.node.children.is_empty();
            let is_container = matches!(
                vn.node.kind,
                NodeKind::Struct { .. }
                    | NodeKind::Option { is_some: true, .. }
                    | NodeKind::RadioGroup { .. }
                    | NodeKind::Checkboxes { .. }
            );
            if has_children || is_container {
                let path = vn.path.clone();
                if self.expanded.contains(&path) {
                    self.expanded.remove(&path);
                } else {
                    self.expanded.insert(path);
                }
            }
        }
    }

    pub fn expand_selected(&mut self) {
        let visible = self.visible_nodes();
        if let Some(vn) = visible.get(self.selected) {
            let is_expandable = !vn.node.children.is_empty()
                || matches!(
                    vn.node.kind,
                    NodeKind::Struct { .. }
                        | NodeKind::Option { is_some: true, .. }
                        | NodeKind::RadioGroup { .. }
                        | NodeKind::Checkboxes { .. }
                );
            if is_expandable {
                let path = vn.path.clone();
                self.expanded.insert(path);
            }
        }
    }

    pub fn collapse_selected(&mut self) {
        let visible = self.visible_nodes();
        if let Some(vn) = visible.get(self.selected) {
            let path = vn.path.clone();
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            }
        }
    }

    pub fn start_edit(&mut self) {
        let visible = self.visible_nodes();
        if let Some(vn) = visible.get(self.selected) {
            let effective_kind = match &vn.node.kind {
                NodeKind::Option {
                    is_some: true,
                    inner_kind: Some(ik),
                } => ik.as_ref(),
                other => other,
            };
            let buf = match effective_kind {
                NodeKind::String => vn.node.value.as_str().unwrap_or("").to_string(),
                NodeKind::Integer => match &vn.node.value {
                    Value::Number(n) => n.to_string(),
                    _ => "0".to_string(),
                },
                NodeKind::Float => match &vn.node.value {
                    Value::Number(n) => n.to_string(),
                    _ => "0.0".to_string(),
                },
                _ => return,
            };
            let cursor_pos = buf.len();
            self.edit_mode = EditMode::Editing {
                buffer: buf,
                cursor_pos,
            };
        }
    }

    pub fn cancel_edit(&mut self) {
        self.edit_mode = EditMode::Normal;
    }

    pub fn confirm_edit(&mut self) -> bool {
        let (buffer, _) = match &self.edit_mode {
            EditMode::Editing { buffer, cursor_pos } => (buffer.clone(), *cursor_pos),
            EditMode::Normal => return false,
        };

        let visible = self.visible_nodes();
        let Some(vn) = visible.get(self.selected) else {
            self.edit_mode = EditMode::Normal;
            return false;
        };
        let path = vn.path.clone();
        let kind = vn.node.kind.clone();
        drop(visible);

        let effective_kind = match &kind {
            NodeKind::Option {
                is_some: true,
                inner_kind: Some(ik),
            } => ik.as_ref(),
            other => other,
        };
        let new_value = match effective_kind {
            NodeKind::String => Some(Value::String(buffer.clone())),
            NodeKind::Integer => buffer
                .parse::<i64>()
                .ok()
                .map(|n| Value::Number(n.into())),
            NodeKind::Float => buffer
                .parse::<f64>()
                .ok()
                .and_then(serde_json::Number::from_f64)
                .map(Value::Number),
            _ => None,
        };

        self.edit_mode = EditMode::Normal;

        if let Some(val) = new_value {
            self.set_value_at_path(&path, val);
            return true;
        }
        false
    }

    pub fn edit_insert_char(&mut self, c: char) {
        if let EditMode::Editing { buffer, cursor_pos } = &mut self.edit_mode {
            buffer.insert(*cursor_pos, c);
            *cursor_pos += c.len_utf8();
        }
    }

    pub fn edit_backspace(&mut self) {
        if let EditMode::Editing { buffer, cursor_pos } = &mut self.edit_mode
            && *cursor_pos > 0 {
                let prev = buffer[..*cursor_pos]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                buffer.remove(prev);
                *cursor_pos = prev;
            }
    }

    pub fn edit_delete(&mut self) {
        if let EditMode::Editing { buffer, cursor_pos } = &mut self.edit_mode
            && *cursor_pos < buffer.len() {
                buffer.remove(*cursor_pos);
            }
    }

    pub fn edit_cursor_left(&mut self) {
        if let EditMode::Editing { buffer, cursor_pos } = &mut self.edit_mode
            && *cursor_pos > 0 {
                *cursor_pos = buffer[..*cursor_pos]
                    .char_indices()
                    .last()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
            }
    }

    pub fn edit_cursor_right(&mut self) {
        if let EditMode::Editing { buffer, cursor_pos } = &mut self.edit_mode
            && *cursor_pos < buffer.len() {
                *cursor_pos += buffer[*cursor_pos..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(0);
            }
    }

    pub fn edit_cursor_home(&mut self) {
        if let EditMode::Editing { cursor_pos, .. } = &mut self.edit_mode {
            *cursor_pos = 0;
        }
    }

    pub fn edit_cursor_end(&mut self) {
        if let EditMode::Editing { buffer, cursor_pos } = &mut self.edit_mode {
            *cursor_pos = buffer.len();
        }
    }

    pub fn toggle_bool(&mut self) {
        let visible = self.visible_nodes();
        if let Some(vn) = visible.get(self.selected)
            && matches!(vn.node.kind, NodeKind::Bool) {
                let path = vn.path.clone();
                let current = vn.node.value.as_bool().unwrap_or(false);
                drop(visible);
                self.set_value_at_path(&path, Value::Bool(!current));
            }
    }

    /// Select the currently highlighted RadioItem, deselecting its siblings.
    /// Returns `true` if the selection actually changed.
    pub fn select_radio(&mut self) -> bool {
        let visible = self.visible_nodes();
        let Some(vn) = visible.get(self.selected) else {
            return false;
        };
        let already_selected = matches!(vn.node.kind, NodeKind::RadioItem { selected: true, .. });
        if !matches!(vn.node.kind, NodeKind::RadioItem { .. }) {
            return false;
        }
        let path = vn.path.clone();
        let variant_name = vn.node.key.clone();
        drop(visible);

        if already_selected {
            // Already selected — just toggle expand/collapse
            self.toggle_expand();
            return false;
        }

        // Find the parent RadioGroup path
        if let Some(dot) = path.rfind('.') {
            let parent_path = &path[..dot];
            let parts: Vec<&str> = parent_path.split('.').collect();
            select_radio_in_nodes(&mut self.nodes, &parts, &variant_name, &self.defs.clone());

            // Auto-expand the selected struct variant's path
            let selected_path = format!("{}.{}", parent_path, variant_name);
            self.expanded.insert(selected_path);
        }
        true
    }

    pub fn toggle_option(&mut self) {
        let visible = self.visible_nodes();
        let Some(vn) = visible.get(self.selected) else {
            return;
        };
        let NodeKind::Option {
            is_some,
            inner_kind,
        } = &vn.node.kind
        else {
            return;
        };
        let path = vn.path.clone();
        let is_some = *is_some;
        let is_scalar = inner_kind.is_some();
        let inner_schema = vn.node.inner_schema.clone();
        drop(visible);

        if is_some {
            self.set_option_none(&path);
            self.expanded.remove(&path);
        } else if let Some(ref schema) = inner_schema {
            let default = default_value_for_schema(schema, &self.defs);
            if is_scalar {
                // Scalar option: just set the value, no children or expansion
                self.set_option_scalar_some(&path, default);
            } else {
                self.set_option_some(&path, default, schema);
                self.expanded.insert(path);
            }
        }
    }

    /// Toggle the currently selected CheckboxItem.
    pub fn toggle_checkbox(&mut self) {
        let visible = self.visible_nodes();
        let Some(vn) = visible.get(self.selected) else {
            return;
        };
        let NodeKind::CheckboxItem { checked } = &vn.node.kind else {
            return;
        };
        let new_checked = !checked;
        let path = vn.path.clone();
        drop(visible);

        let parts: Vec<&str> = path.split('.').collect();
        toggle_checkbox_in_nodes(&mut self.nodes, &parts, new_checked);

        if let Some(dot) = path.rfind('.') {
            let parent_path = &path[..dot];
            rebuild_checkboxes_value(&mut self.nodes, &parent_path.split('.').collect::<Vec<_>>());
        }
    }

    pub fn to_value(&self) -> Value {
        nodes_to_value(&self.nodes)
    }

    pub fn to_config<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.to_value())
    }

    // --- internal helpers ---

    fn set_value_at_path(&mut self, path: &str, value: Value) {
        let parts: Vec<&str> = path.split('.').collect();
        set_in_nodes(&mut self.nodes, &parts, value);
    }

    fn set_option_none(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('.').collect();
        set_option_state_in_nodes(&mut self.nodes, &parts, false, Value::Null, Vec::new());
    }

    fn set_option_scalar_some(&mut self, path: &str, default: Value) {
        let parts: Vec<&str> = path.split('.').collect();
        set_option_state_in_nodes(&mut self.nodes, &parts, true, default, Vec::new());
    }

    fn set_option_some(&mut self, path: &str, default: Value, schema: &Value) {
        let parts: Vec<&str> = path.split('.').collect();
        let resolved = resolve_schema_with_defs(schema, &self.defs);
        let children = build_inner_children_from_schema(resolved, &default, &self.defs, parts.len());
        set_option_state_in_nodes(&mut self.nodes, &parts, true, default, children);
    }
}

fn collect_expanded_paths(nodes: &[ConfigNode], prefix: &mut String, expanded: &mut HashSet<String>) {
    for node in nodes {
        let path = if prefix.is_empty() {
            node.key.clone()
        } else {
            format!("{}.{}", prefix, node.key)
        };

        match &node.kind {
            NodeKind::Struct { .. }
            | NodeKind::RadioGroup { .. }
            | NodeKind::Checkboxes { .. } => {
                expanded.insert(path.clone());
                collect_expanded_paths(&node.children, &mut path.clone(), expanded);
            }
            NodeKind::Option { is_some: true, .. } => {
                expanded.insert(path.clone());
                collect_expanded_paths(&node.children, &mut path.clone(), expanded);
            }
            NodeKind::RadioItem { selected: true, is_struct: true } if !node.children.is_empty() => {
                expanded.insert(path.clone());
                collect_expanded_paths(&node.children, &mut path.clone(), expanded);
            }
            _ => {}
        }
    }
}

fn collect_visible<'a>(
    nodes: &'a [ConfigNode],
    prefix: &mut String,
    expanded: &HashSet<String>,
    filter: Option<&dyn NodeFilter>,
    result: &mut Vec<VisibleNode<'a>>,
) {
    for node in nodes {
        let path = if prefix.is_empty() {
            node.key.clone()
        } else {
            format!("{}.{}", prefix, node.key)
        };

        // Skip nodes hidden by the filter
        if let Some(f) = filter
            && !f.visible(&path) {
                continue;
            }

        let idx = result.len();
        result.push(VisibleNode {
            node,
            path: path.clone(),
            flat_index: idx,
        });

        let is_expanded = expanded.contains(&path);
        if is_expanded && !node.children.is_empty() {
            collect_visible(&node.children, &mut path.clone(), expanded, filter, result);
        }
    }
}

fn set_in_nodes(nodes: &mut [ConfigNode], path_parts: &[&str], value: Value) {
    let Some((&first, rest)) = path_parts.split_first() else {
        return;
    };
    for node in nodes.iter_mut() {
        if node.key == first {
            if rest.is_empty() {
                node.value = value;
                return;
            } else {
                set_in_nodes(&mut node.children, rest, value);
                return;
            }
        }
    }
}

fn set_option_state_in_nodes(
    nodes: &mut [ConfigNode],
    path_parts: &[&str],
    is_some: bool,
    value: Value,
    children: Vec<ConfigNode>,
) {
    let Some((&first, rest)) = path_parts.split_first() else {
        return;
    };
    for node in nodes.iter_mut() {
        if node.key == first {
            if rest.is_empty() {
                if let NodeKind::Option { is_some: ref mut s, .. } = node.kind {
                    *s = is_some;
                }
                node.value = value;
                node.children = children;
                return;
            } else {
                set_option_state_in_nodes(&mut node.children, rest, is_some, value, children);
                return;
            }
        }
    }
}

/// Select `variant_name` in a RadioGroup at the given path,
/// deselecting all other siblings and updating the parent value.
/// For struct variants, builds field children from schema defaults.
fn select_radio_in_nodes(
    nodes: &mut [ConfigNode],
    path_parts: &[&str],
    variant_name: &str,
    defs: &Map<String, Value>,
) {
    let Some((&first, rest)) = path_parts.split_first() else {
        return;
    };
    for node in nodes.iter_mut() {
        if node.key == first {
            if rest.is_empty() {
                // This is the RadioGroup node — update children and value
                let child_depth = node.depth + 1;
                for child in &mut node.children {
                    if let NodeKind::RadioItem { selected: ref mut s, is_struct } = child.kind {
                        let is_match = child.key == variant_name;
                        *s = is_match;
                        child.value = Value::Bool(is_match);

                        if is_struct {
                            if is_match {
                                // Build field children from the variant's schema defaults
                                if let Some(ref schema) = child.inner_schema {
                                    let default_val = default_value_for_schema(schema, defs);
                                    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
                                        child.children = crate::node::build_tree_from_properties(
                                            props,
                                            schema,
                                            defs,
                                            default_val.as_object(),
                                            child_depth + 1,
                                        );
                                    }
                                }
                            } else {
                                // Clear children for deselected struct variants
                                child.children.clear();
                            }
                        }
                    }
                }

                // Update the RadioGroup value: unit → "Name", struct → {"Name": {defaults}}
                let selected_child = node.children.iter().find(|c| c.key == variant_name);
                if let Some(child) = selected_child {
                    if let NodeKind::RadioItem { is_struct: true, .. } = &child.kind {
                        if let Some(ref schema) = child.inner_schema {
                            let default_val = default_value_for_schema(schema, defs);
                            let mut obj = Map::new();
                            obj.insert(variant_name.to_string(), default_val);
                            node.value = Value::Object(obj);
                        }
                    } else {
                        node.value = Value::String(variant_name.to_string());
                    }
                }
                return;
            } else {
                select_radio_in_nodes(&mut node.children, rest, variant_name, defs);
                return;
            }
        }
    }
}

fn toggle_checkbox_in_nodes(nodes: &mut [ConfigNode], path_parts: &[&str], checked: bool) {
    let Some((&first, rest)) = path_parts.split_first() else {
        return;
    };
    for node in nodes.iter_mut() {
        if node.key == first {
            if rest.is_empty() {
                if let NodeKind::CheckboxItem { checked: ref mut c } = node.kind {
                    *c = checked;
                    node.value = Value::Bool(checked);
                }
                return;
            } else {
                toggle_checkbox_in_nodes(&mut node.children, rest, checked);
                return;
            }
        }
    }
}

fn rebuild_checkboxes_value(nodes: &mut [ConfigNode], path_parts: &[&str]) {
    let Some((&first, rest)) = path_parts.split_first() else {
        return;
    };
    for node in nodes.iter_mut() {
        if node.key == first {
            if rest.is_empty() {
                if matches!(node.kind, NodeKind::Checkboxes { .. }) {
                    node.value = Value::Array(
                        node.children
                            .iter()
                            .filter(|c| matches!(c.kind, NodeKind::CheckboxItem { checked: true }))
                            .map(|c| Value::String(c.key.clone()))
                            .collect(),
                    );
                }
                return;
            } else {
                rebuild_checkboxes_value(&mut node.children, rest);
                return;
            }
        }
    }
}

fn nodes_to_value(nodes: &[ConfigNode]) -> Value {
    let mut map = Map::new();
    for node in nodes {
        let val = match &node.kind {
            NodeKind::Struct { .. } => nodes_to_value(&node.children),
            NodeKind::Option { is_some, .. } => {
                if *is_some {
                    if node.children.len() == 1 && node.children[0].key == "value" {
                        node_to_value(&node.children[0])
                    } else if !node.children.is_empty() {
                        nodes_to_value(&node.children)
                    } else {
                        node.value.clone()
                    }
                } else {
                    Value::Null
                }
            }
            NodeKind::RadioGroup { .. } => radio_group_to_value(node),
            NodeKind::Checkboxes { .. } => Value::Array(
                node.children
                    .iter()
                    .filter(|c| matches!(c.kind, NodeKind::CheckboxItem { checked: true }))
                    .map(|c| Value::String(c.key.clone()))
                    .collect(),
            ),
            _ => node_to_value(node),
        };
        map.insert(node.key.clone(), val);
    }
    Value::Object(map)
}

/// Serialize a RadioGroup node to its JSON value.
/// Unit variants produce `"VariantName"`.
/// Struct variants produce `{"VariantName": {field_values...}}`.
fn radio_group_to_value(node: &ConfigNode) -> Value {
    let selected = node
        .children
        .iter()
        .find(|c| matches!(c.kind, NodeKind::RadioItem { selected: true, .. }));
    match selected {
        Some(child) => match &child.kind {
            NodeKind::RadioItem { is_struct: true, .. } if !child.children.is_empty() => {
                let mut obj = Map::new();
                obj.insert(child.key.clone(), nodes_to_value(&child.children));
                Value::Object(obj)
            }
            _ => Value::String(child.key.clone()),
        },
        None => node.value.clone(),
    }
}

fn node_to_value(node: &ConfigNode) -> Value {
    match &node.kind {
        NodeKind::RadioGroup { .. } => radio_group_to_value(node),
        NodeKind::Checkboxes { .. } => Value::Array(
            node.children
                .iter()
                .filter(|c| matches!(c.kind, NodeKind::CheckboxItem { checked: true }))
                .map(|c| Value::String(c.key.clone()))
                .collect(),
        ),
        _ => node.value.clone(),
    }
}

fn resolve_schema_with_defs<'a>(schema: &'a Value, defs: &'a Map<String, Value>) -> &'a Value {
    if let Some(Value::String(ref_path)) = schema.get("$ref") {
        let type_name = ref_path.strip_prefix("#/$defs/").unwrap_or(ref_path);
        defs.get(type_name).unwrap_or(schema)
    } else {
        schema
    }
}

fn build_inner_children_from_schema(
    schema: &Value,
    value: &Value,
    defs: &Map<String, Value>,
    depth: usize,
) -> Vec<ConfigNode> {
    let type_str = schema.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if type_str == "object"
        && let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
            return crate::node::build_tree_from_properties(props, schema, defs, value.as_object(), depth);
        }
    vec![crate::node::build_node_pub("value", schema, value, defs, depth)]
}
