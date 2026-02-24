# schema-tui

A Rust library for building interactive terminal configuration editors from JSON Schema. Define your config as a Rust struct, derive `JsonSchema`, and get a fully navigable tree-based TUI editor for free.

## Features

- **Schema-driven UI** -- automatically generates a tree editor from any `schemars::JsonSchema` type
- **Rich type support** -- structs, optional fields, enums (radio buttons), `Vec<Enum>` (checkboxes), booleans, strings, integers, and floats
- **Vim-style navigation** -- `j`/`k`/`h`/`l` or arrow keys to browse and expand/collapse
- **Inline editing** -- edit scalar values in-place with cursor movement
- **Filtering** -- hide fields or make them read-only with the `NodeFilter` trait
- **Serialization** -- extract the edited config back into typed Rust structs via `serde`

## Quick start

```rust
use schema_tui::{SchemaTree, TreeState, handle_key_event};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(JsonSchema, Serialize, Deserialize)]
struct MyConfig {
    hostname: String,
    port: u16,
    verbose: bool,
}

let schema = schemars::schema_for!(MyConfig);
let value = serde_json::to_value(&my_config)?;
let mut state = TreeState::new(&schema, &value);

// In your ratatui render loop:
let widget = SchemaTree::default().title(" Config Editor ");
frame.render_stateful_widget(widget, frame.area(), &mut state);

// Dispatch keyboard events:
handle_key_event(&mut state, key);

// When done, extract the edited config:
let edited: MyConfig = state.to_config()?;
```

## Running the demo

```sh
cargo run --example demo
```

## Key bindings

| Key | Action |
|---|---|
| `j` / `Down` | Move down |
| `k` / `Up` | Move up |
| `l` / `Right` | Expand node |
| `h` / `Left` | Collapse node |
| `Enter` / `Space` | Toggle or edit |
| `Delete` / `Backspace` | Set option to None |
| `Esc` | Cancel edit |

## License

[MIT](LICENSE)
