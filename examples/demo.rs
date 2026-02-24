use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use schema_tui::{EditMode, NodeFilter, SchemaTree, TreeState, handle_key_event};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize)]
struct AppConfig {
    /// The application name
    app_name: String,
    /// Server configuration
    server: ServerConfig,
    /// Logging settings
    logging: LogConfig,
    /// Run mode
    mode: RunMode,
    /// Maximum retry count
    max_retries: u32,
    /// Timeout in seconds
    timeout: f64,
    /// Enable experimental features
    experimental: bool,
    /// Optional description
    description: Option<String>,
    /// Optional nickname
    nickname: Option<String>,
    /// Optional backup server
    backup_server: Option<ServerConfig>,
    /// Enabled features
    features: Vec<Feature>,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize)]
struct ServerConfig {
    /// Hostname to bind to
    hostname: String,
    /// Port number
    port: u16,
    /// Use TLS
    tls: bool,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize)]
struct LogConfig {
    /// Log level
    level: LogLevel,
    /// Log file path
    file: Option<String>,
    /// Use colors in output
    use_color: bool,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize)]
enum RunMode {
    Debug,
    Release,
    Test,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, JsonSchema, Serialize, Deserialize, PartialEq)]
enum Feature {
    Logging,
    Metrics,
    Tracing,
    Auth,
    Caching,
}

/// Example filter: makes `app_name` and `experimental` read-only,
/// and disables choosing individual enum variants under `mode`.
struct DemoFilter;

impl NodeFilter for DemoFilter {
    fn enabled(&self, path: &str) -> bool {
        !matches!(
            path,
            "app_name" | "experimental" | "mode.Debug" | "mode.Release" | "mode.Test"
        )
    }
}

fn main() -> io::Result<()> {
    let config = AppConfig {
        app_name: "my-app".into(),
        server: ServerConfig {
            hostname: "localhost".into(),
            port: 8080,
            tls: false,
        },
        logging: LogConfig {
            level: LogLevel::Info,
            file: None,
            use_color: true,
        },
        mode: RunMode::Release,
        max_retries: 3,
        timeout: 30.0,
        experimental: false,
        description: Some("A sample application".into()),
        nickname: None,
        backup_server: Some(ServerConfig {
            hostname: "backup.local".into(),
            port: 9090,
            tls: true,
        }),
        features: vec![Feature::Logging, Feature::Auth],
    };

    let schema = schemars::schema_for!(AppConfig);
    let value = serde_json::to_value(&config).unwrap();
    let mut state = TreeState::new(&schema, &value);
    state.set_filter(DemoFilter);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut state);
    ratatui::restore();

    match result {
        Ok(true) => {
            let edited: AppConfig = state.to_config().unwrap();
            println!("\nEdited config:");
            println!("{}", serde_json::to_string_pretty(&edited).unwrap());
        }
        Ok(false) => {
            println!("\nCancelled.");
        }
        Err(e) => return Err(e),
    }
    Ok(())
}

fn run(terminal: &mut DefaultTerminal, state: &mut TreeState) -> io::Result<bool> {
    loop {
        terminal.draw(|frame: &mut Frame| {
            let widget =
                SchemaTree::default().title(" Config Editor (q: save & quit, Ctrl+C: cancel) ");
            frame.render_stateful_widget(widget, frame.area(), state);

            // Show cursor in edit mode
            if let Some((cx, cy)) = state.cursor_position {
                frame.set_cursor_position((cx, cy));
            }
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            // Ctrl+C: cancel
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(false);
            }
            // q in normal mode: save & quit
            if key.code == KeyCode::Char('q') && state.edit_mode == EditMode::Normal {
                return Ok(true);
            }

            handle_key_event(state, key);
        }
    }
}
