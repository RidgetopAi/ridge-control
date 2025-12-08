use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamsConfig {
    #[serde(default)]
    pub streams: Vec<StreamDefinition>,
}

impl Default for StreamsConfig {
    fn default() -> Self {
        Self {
            streams: vec![
                StreamDefinition {
                    id: "example-ws".to_string(),
                    name: "Example WebSocket".to_string(),
                    protocol: StreamProtocol::WebSocket,
                    url: "wss://echo.websocket.org".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 5000,
                    headers: Default::default(),
                },
            ],
        }
    }
}

impl StreamsConfig {
    pub fn load() -> Self {
        let config_path = Self::config_path();
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => return config,
                    Err(e) => {
                        eprintln!("Failed to parse streams.toml: {}", e);
                    }
                },
                Err(e) => {
                    eprintln!("Failed to read streams.toml: {}", e);
                }
            }
        }
        Self::default()
    }

    fn config_path() -> PathBuf {
        directories::BaseDirs::new()
            .map(|dirs| dirs.config_dir().join("ridge-control").join("streams.toml"))
            .unwrap_or_else(|| PathBuf::from("~/.config/ridge-control/streams.toml"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::upper_case_acronyms)]
pub enum StreamProtocol {
    #[default]
    WebSocket,
    #[serde(rename = "sse")]
    SSE,
    Rest,
    Unix,
    Tcp,
}

impl std::fmt::Display for StreamProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamProtocol::WebSocket => write!(f, "WS"),
            StreamProtocol::SSE => write!(f, "SSE"),
            StreamProtocol::Rest => write!(f, "REST"),
            StreamProtocol::Unix => write!(f, "UNIX"),
            StreamProtocol::Tcp => write!(f, "TCP"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub protocol: StreamProtocol,
    pub url: String,
    #[serde(default)]
    pub auto_connect: bool,
    #[serde(default = "default_reconnect")]
    pub reconnect: bool,
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_ms: u64,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
}

fn default_reconnect() -> bool {
    true
}

fn default_reconnect_delay() -> u64 {
    5000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed,
}

impl std::fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "⭘"),
            ConnectionState::Connecting => write!(f, "◌"),
            ConnectionState::Connected => write!(f, "●"),
            ConnectionState::Reconnecting { attempt } => write!(f, "↻{}", attempt),
            ConnectionState::Failed => write!(f, "✕"),
        }
    }
}
