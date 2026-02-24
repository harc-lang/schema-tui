use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    String,
    Integer,
    Float,
    Bool,
    /// `inner_kind` is `Some(String|Integer|Float)` for scalar optionals
    /// (rendered inline, no children), or `None` for struct optionals
    /// (rendered with expandable children).
    Option {
        is_some: bool,
        inner_kind: std::option::Option<Box<NodeKind>>,
    },
    Struct { type_name: std::string::String },
    /// An exclusive enum displayed as a radio-button group.
    /// Children are `RadioItem` nodes; exactly one is selected.
    RadioGroup { variants: Vec<std::string::String> },
    /// A single item inside a `RadioGroup` container.
    /// `is_struct` is true for externally-tagged enum variants that carry
    /// struct data (children hold the variant's fields when selected).
    RadioItem { selected: bool, is_struct: bool },
    /// A `Vec<UnitEnum>` displayed as non-exclusive checkboxes.
    /// Children are `CheckboxItem` nodes.
    Checkboxes { variants: Vec<std::string::String> },
    /// A single item inside a `Checkboxes` container.
    CheckboxItem { checked: bool },
}

#[derive(Debug, Clone)]
pub struct ConfigNode {
    pub key: std::string::String,
    pub kind: NodeKind,
    pub value: Value,
    pub children: Vec<ConfigNode>,
    pub depth: usize,
    pub description: Option<std::string::String>,
    /// For Option nodes: the resolved schema of the inner type, used to
    /// construct a default value when toggling from None to Some.
    pub inner_schema: Option<Value>,
}

/// Build a tree of `ConfigNode` from a root schemars `Schema` and the current
/// config as a `serde_json::Value`.
pub fn build_tree(schema: &schemars::Schema, value: &Value) -> Vec<ConfigNode> {
    let root = schema.as_value();
    let defs = root
        .get("$defs")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let properties = match root.get("properties").and_then(|v| v.as_object()) {
        Some(props) => props,
        None => return Vec::new(),
    };
    let value_obj = value.as_object();
    build_children(properties, root, &defs, value_obj, 0)
}

/// Public entry point for building children from properties, used by state.rs
/// for reconstructing Option<Struct> children.
pub fn build_tree_from_properties(
    properties: &Map<String, Value>,
    parent_schema: &Value,
    defs: &Map<String, Value>,
    value_obj: Option<&Map<String, Value>>,
    depth: usize,
) -> Vec<ConfigNode> {
    build_children(properties, parent_schema, defs, value_obj, depth)
}

/// Public entry point for building a single node, used by state.rs for
/// reconstructing scalar Option children.
pub fn build_node_pub(
    key: &str,
    schema: &Value,
    value: &Value,
    defs: &Map<String, Value>,
    depth: usize,
) -> ConfigNode {
    build_node(key, schema, value, defs, depth)
}

fn build_children(
    properties: &Map<std::string::String, Value>,
    parent_schema: &Value,
    defs: &Map<std::string::String, Value>,
    value_obj: Option<&Map<std::string::String, Value>>,
    depth: usize,
) -> Vec<ConfigNode> {
    // Determine field order from the schema's property order
    let order: Option<Vec<&str>> = parent_schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect());

    // Collect keys in order: required fields first (in order), then remaining
    let mut keys: Vec<&str> = Vec::new();
    if let Some(ref ordered) = order {
        for k in ordered {
            if properties.contains_key(*k) {
                keys.push(k);
            }
        }
    }
    for k in properties.keys() {
        if !keys.contains(&k.as_str()) {
            keys.push(k.as_str());
        }
    }

    keys.iter()
        .filter_map(|key| {
            let field_schema = properties.get(*key)?;
            let field_value = value_obj
                .and_then(|obj| obj.get(*key))
                .cloned()
                .unwrap_or(Value::Null);
            Some(build_node(key, field_schema, &field_value, defs, depth))
        })
        .collect()
}

fn build_node(
    key: &str,
    schema: &Value,
    value: &Value,
    defs: &Map<std::string::String, Value>,
    depth: usize,
) -> ConfigNode {
    let resolved = resolve_schema(schema, defs);
    let description = resolved
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Check for Option<T> pattern 1: anyOf with a null variant
    if let Some(any_of) = resolved.get("anyOf").and_then(|v| v.as_array())
        && let Some(option_node) = try_build_option(key, any_of, value, defs, depth, description.clone()) {
            return option_node;
        }

    // Check for Option<T> pattern 2: type is an array like ["string", "null"]
    // schemars 1.x uses this for Option<primitive>
    if let Some(type_array) = resolved.get("type").and_then(|v| v.as_array())
        && let Some(option_node) =
            try_build_option_from_type_array(key, type_array, resolved, value, defs, depth, description.clone())
        {
            return option_node;
        }

    // Check for enum pattern
    if let Some(enum_node) = try_build_enum(key, resolved, value, defs, depth, description.clone()) {
        return enum_node;
    }

    // Check for Vec<Enum> pattern: array whose items resolve to a unit enum
    if let Some(checkboxes) = try_build_checkboxes(key, resolved, value, defs, depth, description.clone()) {
        return checkboxes;
    }

    // Check type field (string or array with single element)
    let type_str = match resolved.get("type") {
        Some(Value::String(s)) => s.as_str(),
        Some(Value::Array(arr)) if arr.len() == 1 => {
            arr[0].as_str().unwrap_or("")
        }
        _ => "",
    };

    match type_str {
        "string" => ConfigNode {
            key: key.to_string(),
            kind: NodeKind::String,
            value: value.clone(),
            children: Vec::new(),
            depth,
            description,
            inner_schema: None,
        },
        "integer" => ConfigNode {
            key: key.to_string(),
            kind: NodeKind::Integer,
            value: value.clone(),
            children: Vec::new(),
            depth,
            description,
            inner_schema: None,
        },
        "number" => ConfigNode {
            key: key.to_string(),
            kind: NodeKind::Float,
            value: value.clone(),
            children: Vec::new(),
            depth,
            description,
            inner_schema: None,
        },
        "boolean" => ConfigNode {
            key: key.to_string(),
            kind: NodeKind::Bool,
            value: value.clone(),
            children: Vec::new(),
            depth,
            description,
            inner_schema: None,
        },
        "object" => {
            let type_name = resolved
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("object")
                .to_string();
            let props = resolved.get("properties").and_then(|v| v.as_object());
            let children = match props {
                Some(p) => build_children(p, resolved, defs, value.as_object(), depth + 1),
                None => Vec::new(),
            };
            ConfigNode {
                key: key.to_string(),
                kind: NodeKind::Struct { type_name },
                value: value.clone(),
                children,
                depth,
                description,
                inner_schema: None,
            }
        }
        _ => {
            // Fallback: treat as string
            ConfigNode {
                key: key.to_string(),
                kind: NodeKind::String,
                value: value.clone(),
                children: Vec::new(),
                depth,
                description,
                inner_schema: None,
            }
        }
    }
}

fn try_build_option(
    key: &str,
    any_of: &[Value],
    value: &Value,
    defs: &Map<std::string::String, Value>,
    depth: usize,
    description: Option<std::string::String>,
) -> Option<ConfigNode> {
    // Find the null variant and the non-null variant
    let has_null = any_of
        .iter()
        .any(|v| v.get("type").and_then(|t| t.as_str()) == Some("null"));

    if !has_null || any_of.len() < 2 {
        return None;
    }

    let inner_schema_val = any_of
        .iter()
        .find(|v| v.get("type").and_then(|t| t.as_str()) != Some("null"))?;

    let resolved_inner = resolve_schema(inner_schema_val, defs);
    let is_some = !value.is_null();
    let inner_kind = scalar_node_kind(resolved_inner);

    let children = if inner_kind.is_some() {
        // Scalar optional: no children, value lives on this node
        Vec::new()
    } else if is_some {
        build_inner_children(resolved_inner, value, defs, depth + 1)
    } else {
        Vec::new()
    };

    Some(ConfigNode {
        key: key.to_string(),
        kind: NodeKind::Option {
            is_some,
            inner_kind: inner_kind.map(Box::new),
        },
        value: value.clone(),
        children,
        depth,
        description,
        inner_schema: Some(inner_schema_val.clone()),
    })
}

/// Handle `"type": ["string", "null"]` pattern for Option<primitive>.
fn try_build_option_from_type_array(
    key: &str,
    type_array: &[Value],
    _resolved: &Value,
    value: &Value,
    _defs: &Map<std::string::String, Value>,
    depth: usize,
    description: Option<std::string::String>,
) -> Option<ConfigNode> {
    let types: Vec<&str> = type_array.iter().filter_map(|v| v.as_str()).collect();
    if !types.contains(&"null") || types.len() < 2 {
        return None;
    }
    // Find the non-null type
    let inner_type = types.iter().find(|&&t| t != "null")?;
    let is_some = !value.is_null();

    // Build a synthetic schema for the inner type
    let inner_schema = serde_json::json!({ "type": *inner_type });
    let inner_kind = scalar_node_kind(&inner_schema);

    // type-array options are always scalar (string, integer, number, boolean)
    // so we never create children
    Some(ConfigNode {
        key: key.to_string(),
        kind: NodeKind::Option {
            is_some,
            inner_kind: inner_kind.map(Box::new),
        },
        value: value.clone(),
        children: Vec::new(),
        depth,
        description,
        inner_schema: Some(inner_schema),
    })
}

/// Build children for the inner value of an Option<T>.
/// If T is a struct, children are its fields. If T is a scalar, we create
/// a single child node representing the value itself.
fn build_inner_children(
    resolved_schema: &Value,
    value: &Value,
    defs: &Map<std::string::String, Value>,
    depth: usize,
) -> Vec<ConfigNode> {
    let type_str = resolved_schema
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if type_str == "object"
        && let Some(props) = resolved_schema.get("properties").and_then(|v| v.as_object()) {
            return build_children(props, resolved_schema, defs, value.as_object(), depth);
        }

    // For scalar optional types, create a single "value" child node
    vec![build_node("value", resolved_schema, value, defs, depth)]
}

/// Detect `{ "type": "array", "items": <enum-schema> }` and build a Checkboxes
/// container with one CheckboxItem child per variant.
fn try_build_checkboxes(
    key: &str,
    resolved: &Value,
    value: &Value,
    defs: &Map<std::string::String, Value>,
    depth: usize,
    description: Option<std::string::String>,
) -> Option<ConfigNode> {
    let type_str = resolved.get("type").and_then(|v| v.as_str())?;
    if type_str != "array" {
        return None;
    }
    let items_schema = resolved.get("items")?;
    let resolved_items = resolve_schema(items_schema, defs);
    let variants = extract_enum_variants(resolved_items)?;

    let current_values: Vec<std::string::String> = value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let children: Vec<ConfigNode> = variants
        .iter()
        .map(|variant| {
            let checked = current_values.contains(variant);
            ConfigNode {
                key: variant.clone(),
                kind: NodeKind::CheckboxItem { checked },
                value: Value::Bool(checked),
                children: Vec::new(),
                depth: depth + 1,
                description: None,
                inner_schema: None,
            }
        })
        .collect();

    Some(ConfigNode {
        key: key.to_string(),
        kind: NodeKind::Checkboxes {
            variants: variants.clone(),
        },
        value: value.clone(),
        children,
        depth,
        description,
        inner_schema: None,
    })
}

/// Extract unit-enum variants from a resolved schema that represents a
/// string enum (either `"enum": [...]` or `"oneOf": [{"const": ...}, ...]`).
fn extract_enum_variants(schema: &Value) -> Option<Vec<std::string::String>> {
    // Pattern 1: "enum" array of strings (most common for schemars 1.x)
    if let Some(enum_arr) = schema.get("enum").and_then(|v| v.as_array()) {
        let variants: Vec<std::string::String> = enum_arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !variants.is_empty() {
            return Some(variants);
        }
    }
    // Pattern 2: "oneOf" with "const" values
    if let Some(one_of) = schema.get("oneOf").and_then(|v| v.as_array()) {
        let variants: Vec<std::string::String> = one_of
            .iter()
            .filter_map(|v| v.get("const").and_then(|c| c.as_str()).map(|s| s.to_string()))
            .collect();
        if !variants.is_empty() {
            return Some(variants);
        }
    }
    None
}

/// Describes one variant of an externally-tagged enum.
#[derive(Clone)]
struct VariantInfo {
    name: std::string::String,
    /// `None` for unit variants, `Some(schema)` for struct variants.
    inner_schema: Option<Value>,
}

/// Build a RadioGroup container with RadioItem children for enums
/// (both pure-unit and mixed unit/struct variants).
fn try_build_enum(
    key: &str,
    resolved: &Value,
    value: &Value,
    defs: &Map<std::string::String, Value>,
    depth: usize,
    description: Option<std::string::String>,
) -> Option<ConfigNode> {
    let variant_infos = extract_variant_infos(resolved, defs)?;

    // Determine which variant is currently selected and get its inner value.
    // Unit variant: value is `"Sod"` → current_name = "Sod", inner_value = None
    // Struct variant: value is `{"SodCustom": {"n": 100}}` → current_name = "SodCustom",
    //   inner_value = Some({"n": 100})
    let (current_name, current_inner_value) = match value {
        Value::String(s) => (s.as_str().to_string(), None),
        Value::Object(obj) if obj.len() == 1 => {
            let (k, v) = obj.iter().next().unwrap();
            (k.clone(), Some(v.clone()))
        }
        _ => (std::string::String::new(), None),
    };

    let variants: Vec<std::string::String> =
        variant_infos.iter().map(|vi| vi.name.clone()).collect();

    let children: Vec<ConfigNode> = variant_infos
        .iter()
        .map(|vi| {
            let is_selected = vi.name == current_name;
            let is_struct = vi.inner_schema.is_some();

            // For a selected struct variant, build its field children from
            // the current value (or defaults if unavailable).
            let field_children = if is_selected && is_struct {
                if let Some(ref schema_val) = vi.inner_schema {
                    let resolved_inner = resolve_schema(schema_val, defs);
                    let inner_val = current_inner_value.as_ref().unwrap_or(&Value::Null);
                    build_struct_variant_children(resolved_inner, inner_val, defs, depth + 2)
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            ConfigNode {
                key: vi.name.clone(),
                kind: NodeKind::RadioItem { selected: is_selected, is_struct },
                value: Value::Bool(is_selected),
                children: field_children,
                depth: depth + 1,
                description: None,
                inner_schema: vi.inner_schema.clone(),
            }
        })
        .collect();

    Some(ConfigNode {
        key: key.to_string(),
        kind: NodeKind::RadioGroup { variants },
        value: value.clone(),
        children,
        depth,
        description,
        inner_schema: None,
    })
}

/// Build the field children for a selected struct variant.
fn build_struct_variant_children(
    resolved_schema: &Value,
    value: &Value,
    defs: &Map<std::string::String, Value>,
    depth: usize,
) -> Vec<ConfigNode> {
    if let Some(props) = resolved_schema.get("properties").and_then(|v| v.as_object()) {
        build_children(props, resolved_schema, defs, value.as_object(), depth)
    } else {
        Vec::new()
    }
}

/// Extract per-variant information from an enum schema.
/// Returns `None` if the schema is not an enum.
fn extract_variant_infos(
    resolved: &Value,
    defs: &Map<std::string::String, Value>,
) -> Option<Vec<VariantInfo>> {
    // Pattern 1: "oneOf" array
    if let Some(one_of) = resolved.get("oneOf").and_then(|v| v.as_array()) {
        let mut infos = Vec::new();
        for entry in one_of {
            let resolved_entry = resolve_schema(entry, defs);

            // Unit variant: {"const": "Sod"} or {"enum": ["Sod"], "type": "string"}
            if let Some(c) = resolved_entry.get("const").and_then(|c| c.as_str()) {
                infos.push(VariantInfo { name: c.to_string(), inner_schema: None });
                continue;
            }
            if let Some(enum_arr) = resolved_entry.get("enum").and_then(|v| v.as_array()) {
                for ev in enum_arr {
                    if let Some(s) = ev.as_str() {
                        infos.push(VariantInfo { name: s.to_string(), inner_schema: None });
                    }
                }
                continue;
            }

            // Struct variant: {"properties": {"SodCustom": {inner_schema}}, "type": "object"}
            if let Some(props) = resolved_entry.get("properties").and_then(|p| p.as_object())
                && props.len() == 1
            {
                let (variant_name, variant_schema) = props.iter().next().unwrap();
                let resolved_variant = resolve_schema(variant_schema, defs);
                infos.push(VariantInfo {
                    name: variant_name.clone(),
                    inner_schema: Some(resolved_variant.clone()),
                });
                continue;
            }
        }
        if !infos.is_empty() {
            return Some(infos);
        }
    }

    // Pattern 2: "enum" array of strings (pure unit enum)
    if let Some(enum_values) = resolved.get("enum").and_then(|v| v.as_array()) {
        let infos: Vec<VariantInfo> = enum_values
            .iter()
            .filter_map(|v| v.as_str().map(|s| VariantInfo { name: s.to_string(), inner_schema: None }))
            .collect();
        if !infos.is_empty() {
            return Some(infos);
        }
    }

    None
}

/// If the schema describes a scalar type, return the corresponding `NodeKind`.
fn scalar_node_kind(schema: &Value) -> Option<NodeKind> {
    let type_str = schema.get("type").and_then(|v| v.as_str())?;
    match type_str {
        "string" => Some(NodeKind::String),
        "integer" => Some(NodeKind::Integer),
        "number" => Some(NodeKind::Float),
        "boolean" => Some(NodeKind::Bool),
        _ => None,
    }
}

fn resolve_schema<'a>(schema: &'a Value, defs: &'a Map<std::string::String, Value>) -> &'a Value {
    if let Some(Value::String(ref_path)) = schema.get("$ref") {
        let type_name = ref_path.strip_prefix("#/$defs/").unwrap_or(ref_path);
        defs.get(type_name).unwrap_or(schema)
    } else {
        schema
    }
}

/// Create a default `serde_json::Value` for a given schema.
pub fn default_value_for_schema(
    schema: &Value,
    defs: &Map<std::string::String, Value>,
) -> Value {
    let resolved = resolve_schema(schema, defs);

    // If the schema specifies an explicit default, use it
    if let Some(default) = resolved.get("default") {
        return default.clone();
    }

    // Check for anyOf (Option pattern) — default to null
    if resolved.get("anyOf").is_some() {
        return Value::Null;
    }

    // Check for type array with null (Option pattern) — default to null
    if let Some(type_arr) = resolved.get("type").and_then(|v| v.as_array()) {
        let types: Vec<&str> = type_arr.iter().filter_map(|v| v.as_str()).collect();
        if types.contains(&"null") {
            return Value::Null;
        }
    }

    // Check for enum
    if let Some(one_of) = resolved.get("oneOf").and_then(|v| v.as_array())
        && let Some(first) = one_of.first()
            && let Some(c) = first.get("const") {
                return c.clone();
            }
    if let Some(enum_values) = resolved.get("enum").and_then(|v| v.as_array())
        && let Some(first) = enum_values.first() {
            return first.clone();
        }

    let type_str = match resolved.get("type") {
        Some(Value::String(s)) => s.as_str(),
        Some(Value::Array(arr)) if arr.len() == 1 => {
            arr[0].as_str().unwrap_or("")
        }
        _ => "",
    };
    match type_str {
        "string" => Value::String(std::string::String::new()),
        "integer" => Value::Number(0.into()),
        "number" => Value::Number(serde_json::Number::from_f64(0.0).unwrap()),
        "boolean" => Value::Bool(false),
        "array" => Value::Array(vec![]),
        "object" => {
            let mut obj = Map::new();
            if let Some(props) = resolved.get("properties").and_then(|v| v.as_object()) {
                for (k, v) in props {
                    obj.insert(k.clone(), default_value_for_schema(v, defs));
                }
            }
            Value::Object(obj)
        }
        _ => Value::Null,
    }
}
