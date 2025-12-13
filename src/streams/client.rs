// Stream client - some methods for future status tracking
#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpStream, UnixStream};
use tokio::sync::mpsc;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use crate::streams::config::{ConnectionState, StreamDefinition, StreamProtocol};

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Connected(String),
    Disconnected(String, Option<String>),
    Data(String, StreamData),
    Error(String, String),
    StateChanged(String, ConnectionState),
    /// Reconnection attempt started with attempt number
    ReconnectAttempt(String, u32),
    /// Reconnection gave up after max attempts
    ReconnectGaveUp(String),
}

#[derive(Debug, Clone)]
pub enum StreamData {
    Text(String),
    Binary(Vec<u8>),
}

/// Connection health information for graceful degradation
#[derive(Clone, Debug)]
pub struct ConnectionHealth {
    /// Last error message if any
    pub last_error: Option<String>,
    /// Time of last error
    pub last_error_time: Option<Instant>,
    /// Number of consecutive failures
    pub failure_count: u32,
    /// Time of last successful connection
    pub last_connected: Option<Instant>,
    /// Whether auto-reconnect is in progress
    pub reconnect_in_progress: bool,
    /// Current reconnect attempt number
    pub reconnect_attempt: u32,
    /// Max reconnect attempts (0 = infinite)
    pub max_reconnect_attempts: u32,
}

impl Default for ConnectionHealth {
    fn default() -> Self {
        Self {
            last_error: None,
            last_error_time: None,
            failure_count: 0,
            last_connected: None,
            reconnect_in_progress: false,
            reconnect_attempt: 0,
            max_reconnect_attempts: 5,
        }
    }
}

impl ConnectionHealth {
    pub fn record_error(&mut self, error: String) {
        self.last_error = Some(error);
        self.last_error_time = Some(Instant::now());
        self.failure_count += 1;
    }

    pub fn record_connected(&mut self) {
        self.last_connected = Some(Instant::now());
        self.failure_count = 0;
        self.reconnect_in_progress = false;
        self.reconnect_attempt = 0;
    }

    pub fn start_reconnect(&mut self) {
        self.reconnect_in_progress = true;
        self.reconnect_attempt += 1;
    }

    pub fn stop_reconnect(&mut self) {
        self.reconnect_in_progress = false;
    }

    pub fn reset(&mut self) {
        self.last_error = None;
        self.last_error_time = None;
        self.failure_count = 0;
        self.reconnect_in_progress = false;
        self.reconnect_attempt = 0;
    }

    /// Check if we should attempt reconnection
    pub fn should_reconnect(&self) -> bool {
        if self.max_reconnect_attempts == 0 {
            return true; // Infinite retries
        }
        self.reconnect_attempt < self.max_reconnect_attempts
    }

    /// Calculate backoff delay with exponential backoff and jitter
    pub fn backoff_delay(&self, base_delay_ms: u64) -> Duration {
        let attempt = self.reconnect_attempt.min(10);
        let exponential_delay = base_delay_ms * (2_u64.pow(attempt));
        let max_delay = 60_000; // Cap at 60 seconds
        let delay = exponential_delay.min(max_delay);
        
        // Add jitter (±20%)
        let jitter_range = delay / 5;
        let jitter = (fastrand::u64(0..jitter_range * 2)).saturating_sub(jitter_range);
        Duration::from_millis(delay.saturating_add(jitter))
    }

    /// Human-readable status for UI display
    pub fn status_message(&self) -> String {
        if self.reconnect_in_progress {
            format!("Reconnecting (attempt {}/{})", 
                self.reconnect_attempt,
                if self.max_reconnect_attempts == 0 { "∞".to_string() } else { self.max_reconnect_attempts.to_string() }
            )
        } else if let Some(ref err) = self.last_error {
            let truncated = if err.len() > 50 { 
                format!("{}...", &err[..47]) 
            } else { 
                err.clone() 
            };
            format!("Failed: {}", truncated)
        } else {
            "Disconnected".to_string()
        }
    }
}

#[derive(Clone)]
pub struct StreamClient {
    definition: StreamDefinition,
    state: ConnectionState,
    buffer: Vec<StreamData>,
    health: ConnectionHealth,
}

impl StreamClient {
    pub fn new(definition: StreamDefinition) -> Self {
        Self {
            definition,
            state: ConnectionState::Disconnected,
            buffer: Vec::with_capacity(1000),
            health: ConnectionHealth::default(),
        }
    }

    pub fn id(&self) -> &str {
        &self.definition.id
    }

    pub fn name(&self) -> &str {
        &self.definition.name
    }

    pub fn protocol(&self) -> StreamProtocol {
        self.definition.protocol
    }

    pub fn url(&self) -> &str {
        &self.definition.url
    }

    pub fn definition(&self) -> &StreamDefinition {
        &self.definition
    }

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
    }

    pub fn health(&self) -> &ConnectionHealth {
        &self.health
    }

    pub fn health_mut(&mut self) -> &mut ConnectionHealth {
        &mut self.health
    }

    pub fn buffer(&self) -> &[StreamData] {
        &self.buffer
    }

    pub fn push_data(&mut self, data: StreamData) {
        if self.buffer.len() >= 1000 {
            self.buffer.remove(0);
        }
        self.buffer.push(data);
    }

    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Check if stream is in a failed/degraded state
    pub fn is_degraded(&self) -> bool {
        matches!(self.state, ConnectionState::Failed) || self.health.failure_count > 0
    }

    /// Check if reconnection is enabled for this stream
    pub fn reconnect_enabled(&self) -> bool {
        self.definition.reconnect
    }

    /// Get reconnect delay from definition
    pub fn reconnect_delay_ms(&self) -> u64 {
        self.definition.reconnect_delay_ms
    }
}

impl Default for StreamManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct StreamManager {
    clients: Vec<StreamClient>,
    event_tx: mpsc::UnboundedSender<StreamEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<StreamEvent>>,
}

impl StreamManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            clients: Vec::new(),
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    pub fn take_event_rx(&mut self) -> Option<mpsc::UnboundedReceiver<StreamEvent>> {
        self.event_rx.take()
    }

    pub fn event_tx(&self) -> mpsc::UnboundedSender<StreamEvent> {
        self.event_tx.clone()
    }

    pub fn load_streams(&mut self, config: &super::config::StreamsConfig) {
        self.clients.clear();
        for def in &config.streams {
            self.clients.push(StreamClient::new(def.clone()));
        }
    }

    pub fn clients(&self) -> &[StreamClient] {
        &self.clients
    }

    pub fn clients_mut(&mut self) -> &mut [StreamClient] {
        &mut self.clients
    }

    pub fn get_client(&self, id: &str) -> Option<&StreamClient> {
        self.clients.iter().find(|c| c.id() == id)
    }

    pub fn get_client_mut(&mut self, id: &str) -> Option<&mut StreamClient> {
        self.clients.iter_mut().find(|c| c.id() == id)
    }

    pub fn connect(&mut self, id: &str) {
        self.connect_internal(id, false);
    }

    /// Retry connection for a failed stream, resetting health state
    pub fn retry(&mut self, id: &str) {
        if let Some(client) = self.get_client_mut(id) {
            client.health_mut().reset();
        }
        self.connect_internal(id, false);
    }

    fn connect_internal(&mut self, id: &str, is_reconnect: bool) {
        if let Some(client) = self.get_client_mut(id) {
            if matches!(client.state(), ConnectionState::Connected | ConnectionState::Connecting) {
                return;
            }
            
            // Don't reconnect if already reconnecting from elsewhere
            if is_reconnect && matches!(client.state(), ConnectionState::Reconnecting { .. }) {
                return;
            }

            client.set_state(ConnectionState::Connecting);
            let definition = client.definition().clone();
            let event_tx = self.event_tx.clone();

            match definition.protocol {
                StreamProtocol::WebSocket => {
                    tokio::spawn(async move {
                        Self::websocket_connect(definition, event_tx).await;
                    });
                }
                StreamProtocol::Unix => {
                    tokio::spawn(async move {
                        Self::unix_socket_connect(definition, event_tx).await;
                    });
                }
                StreamProtocol::Tcp => {
                    tokio::spawn(async move {
                        Self::tcp_connect(definition, event_tx).await;
                    });
                }
                _ => {
                    let _ = event_tx.send(StreamEvent::Error(
                        id.to_string(),
                        format!("Protocol {:?} not yet implemented", definition.protocol),
                    ));
                }
            }
        }
    }

    pub fn disconnect(&mut self, id: &str) {
        if let Some(client) = self.get_client_mut(id) {
            client.set_state(ConnectionState::Disconnected);
            client.health_mut().stop_reconnect();
            let _ = self.event_tx.send(StreamEvent::Disconnected(id.to_string(), None));
        }
    }

    /// Start auto-reconnect for a stream (called after failure if enabled)
    pub fn start_reconnect(&mut self, id: &str) {
        if let Some(client) = self.get_client_mut(id) {
            if !client.reconnect_enabled() {
                return;
            }

            // Get immutable data before mutable borrow
            let reconnect_delay_ms = client.reconnect_delay_ms();
            
            // Check if we should reconnect (read-only check first)
            if !client.health().should_reconnect() {
                // Give up after max attempts
                let _ = self.event_tx.send(StreamEvent::ReconnectGaveUp(id.to_string()));
                return;
            }

            // Now mutate health
            let health = client.health_mut();
            health.start_reconnect();
            let attempt = health.reconnect_attempt;
            let delay = health.backoff_delay(reconnect_delay_ms);
            
            client.set_state(ConnectionState::Reconnecting { attempt });
            
            let protocol = client.protocol();
            let definition = client.definition().clone();
            let event_tx = self.event_tx.clone();
            let stream_id = id.to_string();

            // Notify about reconnect attempt
            let _ = event_tx.send(StreamEvent::ReconnectAttempt(stream_id.clone(), attempt));

            Self::spawn_reconnect(protocol, definition, event_tx, delay);
        }
    }

    /// Cancel ongoing reconnection attempts for a stream
    pub fn cancel_reconnect(&mut self, id: &str) {
        if let Some(client) = self.get_client_mut(id) {
            client.health_mut().stop_reconnect();
            client.set_state(ConnectionState::Failed);
        }
    }

    async fn websocket_connect(definition: StreamDefinition, event_tx: mpsc::UnboundedSender<StreamEvent>) {
        let id = definition.id.clone();

        let connect_result = connect_async(&definition.url).await;

        match connect_result {
            Ok((ws_stream, _response)) => {
                let _ = event_tx.send(StreamEvent::StateChanged(id.clone(), ConnectionState::Connected));
                let _ = event_tx.send(StreamEvent::Connected(id.clone()));

                let (mut write, mut read) = ws_stream.split();
                let (_write_tx, mut write_rx) = mpsc::unbounded_channel::<WsMessage>();
                
                tokio::spawn(async move {
                    while let Some(msg) = write_rx.recv().await {
                        if write.send(msg).await.is_err() {
                            break;
                        }
                    }
                });

                while let Some(msg_result) = read.next().await {
                    match msg_result {
                        Ok(msg) => {
                            let data = match msg {
                                WsMessage::Text(text) => Some(StreamData::Text(text.to_string())),
                                WsMessage::Binary(bin) => Some(StreamData::Binary(bin.to_vec())),
                                WsMessage::Close(_) => {
                                    let _ = event_tx.send(StreamEvent::Disconnected(id.clone(), Some("Connection closed".to_string())));
                                    break;
                                }
                                _ => None,
                            };
                            if let Some(d) = data {
                                let _ = event_tx.send(StreamEvent::Data(id.clone(), d));
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.send(StreamEvent::Error(id.clone(), e.to_string()));
                            break;
                        }
                    }
                }

                let _ = event_tx.send(StreamEvent::StateChanged(id, ConnectionState::Disconnected));
            }
            Err(e) => {
                let _ = event_tx.send(StreamEvent::StateChanged(id.clone(), ConnectionState::Failed));
                let _ = event_tx.send(StreamEvent::Error(id, e.to_string()));
            }
        }
    }

    async fn unix_socket_connect(definition: StreamDefinition, event_tx: mpsc::UnboundedSender<StreamEvent>) {
        let id = definition.id.clone();
        let socket_path = PathBuf::from(&definition.url);

        match UnixStream::connect(&socket_path).await {
            Ok(stream) => {
                let _ = event_tx.send(StreamEvent::StateChanged(id.clone(), ConnectionState::Connected));
                let _ = event_tx.send(StreamEvent::Connected(id.clone()));

                let (read_half, _write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut line = String::new();

                loop {
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            let _ = event_tx.send(StreamEvent::Disconnected(
                                id.clone(),
                                Some("Socket closed (EOF)".to_string()),
                            ));
                            break;
                        }
                        Ok(_) => {
                            let data = std::mem::take(&mut line);
                            let trimmed = data.trim_end_matches('\n').to_string();
                            if !trimmed.is_empty() {
                                let _ = event_tx.send(StreamEvent::Data(id.clone(), StreamData::Text(trimmed)));
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.send(StreamEvent::Error(id.clone(), e.to_string()));
                            break;
                        }
                    }
                }

                let _ = event_tx.send(StreamEvent::StateChanged(id, ConnectionState::Disconnected));
            }
            Err(e) => {
                let _ = event_tx.send(StreamEvent::StateChanged(id.clone(), ConnectionState::Failed));
                let _ = event_tx.send(StreamEvent::Error(id, e.to_string()));
            }
        }
    }

    async fn tcp_connect(definition: StreamDefinition, event_tx: mpsc::UnboundedSender<StreamEvent>) {
        let id = definition.id.clone();
        let addr = &definition.url;

        match TcpStream::connect(addr).await {
            Ok(stream) => {
                let _ = event_tx.send(StreamEvent::StateChanged(id.clone(), ConnectionState::Connected));
                let _ = event_tx.send(StreamEvent::Connected(id.clone()));

                let (read_half, _write_half) = stream.into_split();
                let mut reader = BufReader::new(read_half);
                let mut line = String::new();

                loop {
                    match reader.read_line(&mut line).await {
                        Ok(0) => {
                            let _ = event_tx.send(StreamEvent::Disconnected(
                                id.clone(),
                                Some("TCP connection closed (EOF)".to_string()),
                            ));
                            break;
                        }
                        Ok(_) => {
                            let data = std::mem::take(&mut line);
                            let trimmed = data.trim_end_matches('\n').to_string();
                            if !trimmed.is_empty() {
                                let _ = event_tx.send(StreamEvent::Data(id.clone(), StreamData::Text(trimmed)));
                            }
                        }
                        Err(e) => {
                            let _ = event_tx.send(StreamEvent::Error(id.clone(), e.to_string()));
                            break;
                        }
                    }
                }

                let _ = event_tx.send(StreamEvent::StateChanged(id, ConnectionState::Disconnected));
            }
            Err(e) => {
                let _ = event_tx.send(StreamEvent::StateChanged(id.clone(), ConnectionState::Failed));
                let _ = event_tx.send(StreamEvent::Error(id, e.to_string()));
            }
        }
    }

    fn spawn_reconnect(protocol: StreamProtocol, definition: StreamDefinition, event_tx: mpsc::UnboundedSender<StreamEvent>, delay: Duration) {
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            match protocol {
                StreamProtocol::WebSocket => Self::websocket_connect(definition, event_tx).await,
                StreamProtocol::Unix => Self::unix_socket_connect(definition, event_tx).await,
                StreamProtocol::Tcp => Self::tcp_connect(definition, event_tx).await,
                _ => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_health_default() {
        let health = ConnectionHealth::default();
        assert!(health.last_error.is_none());
        assert_eq!(health.failure_count, 0);
        assert!(!health.reconnect_in_progress);
        assert_eq!(health.reconnect_attempt, 0);
        assert_eq!(health.max_reconnect_attempts, 5);
    }

    #[test]
    fn test_connection_health_record_error() {
        let mut health = ConnectionHealth::default();
        health.record_error("Connection refused".to_string());
        
        assert_eq!(health.last_error, Some("Connection refused".to_string()));
        assert!(health.last_error_time.is_some());
        assert_eq!(health.failure_count, 1);
        
        health.record_error("Timeout".to_string());
        assert_eq!(health.failure_count, 2);
    }

    #[test]
    fn test_connection_health_record_connected() {
        let mut health = ConnectionHealth::default();
        health.record_error("Some error".to_string());
        health.reconnect_in_progress = true;
        health.reconnect_attempt = 3;
        
        health.record_connected();
        
        assert!(health.last_connected.is_some());
        assert_eq!(health.failure_count, 0);
        assert!(!health.reconnect_in_progress);
        assert_eq!(health.reconnect_attempt, 0);
    }

    #[test]
    fn test_connection_health_should_reconnect() {
        let mut health = ConnectionHealth::default();
        assert!(health.should_reconnect());
        
        health.reconnect_attempt = 4;
        assert!(health.should_reconnect());
        
        health.reconnect_attempt = 5;
        assert!(!health.should_reconnect());
        
        // Infinite retries
        health.max_reconnect_attempts = 0;
        health.reconnect_attempt = 100;
        assert!(health.should_reconnect());
    }

    #[test]
    fn test_connection_health_backoff_delay() {
        let mut health = ConnectionHealth::default();
        health.reconnect_attempt = 0;
        
        // Base delay = 1000ms
        let delay0 = health.backoff_delay(1000);
        assert!(delay0.as_millis() >= 800 && delay0.as_millis() <= 1200);
        
        health.reconnect_attempt = 1;
        let delay1 = health.backoff_delay(1000);
        assert!(delay1.as_millis() >= 1600 && delay1.as_millis() <= 2400);
        
        health.reconnect_attempt = 3;
        let delay3 = health.backoff_delay(1000);
        assert!(delay3.as_millis() >= 6400 && delay3.as_millis() <= 9600);
        
        // Should cap at 60 seconds
        health.reconnect_attempt = 10;
        let delay_max = health.backoff_delay(1000);
        assert!(delay_max.as_millis() <= 72000); // 60s + 20% jitter
    }

    #[test]
    fn test_connection_health_status_message() {
        let mut health = ConnectionHealth::default();
        assert_eq!(health.status_message(), "Disconnected");
        
        health.record_error("Connection refused".to_string());
        assert!(health.status_message().contains("Failed:"));
        
        health.reconnect_in_progress = true;
        health.reconnect_attempt = 2;
        assert!(health.status_message().contains("Reconnecting"));
        assert!(health.status_message().contains("2/5"));
    }

    #[test]
    fn test_connection_health_reset() {
        let mut health = ConnectionHealth::default();
        health.record_error("Error".to_string());
        health.reconnect_in_progress = true;
        health.reconnect_attempt = 3;
        
        health.reset();
        
        assert!(health.last_error.is_none());
        assert_eq!(health.failure_count, 0);
        assert!(!health.reconnect_in_progress);
        assert_eq!(health.reconnect_attempt, 0);
    }

    #[test]
    fn test_stream_client_is_degraded() {
        let def = StreamDefinition {
            id: "test".to_string(),
            name: "Test".to_string(),
            protocol: StreamProtocol::WebSocket,
            url: "wss://test.example.com".to_string(),
            auto_connect: false,
            reconnect: true,
            reconnect_delay_ms: 1000,
            headers: Default::default(),
        };
        
        let mut client = StreamClient::new(def);
        assert!(!client.is_degraded());
        
        client.health_mut().record_error("Error".to_string());
        assert!(client.is_degraded());
        
        client.health_mut().reset();
        client.set_state(ConnectionState::Failed);
        assert!(client.is_degraded());
    }

    #[test]
    fn test_stream_manager_new() {
        let manager = StreamManager::new();
        assert!(manager.clients.is_empty());
        assert!(manager.event_rx.is_some());
    }

    #[test]
    fn test_stream_client_unix_socket() {
        let def = StreamDefinition {
            id: "unix-test".to_string(),
            name: "Unix Socket Test".to_string(),
            protocol: StreamProtocol::Unix,
            url: "/tmp/test.sock".to_string(),
            auto_connect: false,
            reconnect: true,
            reconnect_delay_ms: 1000,
            headers: Default::default(),
        };
        
        let client = StreamClient::new(def);
        assert_eq!(client.id(), "unix-test");
        assert_eq!(client.name(), "Unix Socket Test");
        assert_eq!(client.protocol(), StreamProtocol::Unix);
        assert_eq!(client.url(), "/tmp/test.sock");
        assert!(matches!(client.state(), ConnectionState::Disconnected));
    }

    #[test]
    fn test_stream_manager_load_unix_streams() {
        use crate::streams::config::StreamsConfig;
        
        let mut manager = StreamManager::new();
        
        let config = StreamsConfig {
            streams: vec![
                StreamDefinition {
                    id: "ws-stream".to_string(),
                    name: "WebSocket".to_string(),
                    protocol: StreamProtocol::WebSocket,
                    url: "wss://example.com".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 1000,
                    headers: Default::default(),
                },
                StreamDefinition {
                    id: "unix-stream".to_string(),
                    name: "Unix Socket".to_string(),
                    protocol: StreamProtocol::Unix,
                    url: "/var/run/app.sock".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 2000,
                    headers: Default::default(),
                },
            ],
        };
        
        manager.load_streams(&config);
        
        assert_eq!(manager.clients().len(), 2);
        
        let ws_client = manager.get_client("ws-stream").unwrap();
        assert_eq!(ws_client.protocol(), StreamProtocol::WebSocket);
        
        let unix_client = manager.get_client("unix-stream").unwrap();
        assert_eq!(unix_client.protocol(), StreamProtocol::Unix);
        assert_eq!(unix_client.url(), "/var/run/app.sock");
        assert_eq!(unix_client.reconnect_delay_ms(), 2000);
    }

    #[test]
    fn test_unix_socket_path_parsing() {
        let path_str = "/var/run/myapp/socket.sock";
        let path = PathBuf::from(path_str);
        assert_eq!(path.to_str().unwrap(), path_str);
        
        let relative_path = PathBuf::from("./local.sock");
        assert!(relative_path.to_str().unwrap().contains("local.sock"));
    }

    #[test]
    fn test_stream_client_tcp_socket() {
        let def = StreamDefinition {
            id: "tcp-test".to_string(),
            name: "TCP Socket Test".to_string(),
            protocol: StreamProtocol::Tcp,
            url: "127.0.0.1:9000".to_string(),
            auto_connect: false,
            reconnect: true,
            reconnect_delay_ms: 1000,
            headers: Default::default(),
        };
        
        let client = StreamClient::new(def);
        assert_eq!(client.id(), "tcp-test");
        assert_eq!(client.name(), "TCP Socket Test");
        assert_eq!(client.protocol(), StreamProtocol::Tcp);
        assert_eq!(client.url(), "127.0.0.1:9000");
        assert!(matches!(client.state(), ConnectionState::Disconnected));
    }

    #[test]
    fn test_stream_manager_load_tcp_streams() {
        use crate::streams::config::StreamsConfig;
        
        let mut manager = StreamManager::new();
        
        let config = StreamsConfig {
            streams: vec![
                StreamDefinition {
                    id: "ws-stream".to_string(),
                    name: "WebSocket".to_string(),
                    protocol: StreamProtocol::WebSocket,
                    url: "wss://example.com".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 1000,
                    headers: Default::default(),
                },
                StreamDefinition {
                    id: "tcp-stream".to_string(),
                    name: "TCP Socket".to_string(),
                    protocol: StreamProtocol::Tcp,
                    url: "192.168.1.100:8080".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 3000,
                    headers: Default::default(),
                },
            ],
        };
        
        manager.load_streams(&config);
        
        assert_eq!(manager.clients().len(), 2);
        
        let ws_client = manager.get_client("ws-stream").unwrap();
        assert_eq!(ws_client.protocol(), StreamProtocol::WebSocket);
        
        let tcp_client = manager.get_client("tcp-stream").unwrap();
        assert_eq!(tcp_client.protocol(), StreamProtocol::Tcp);
        assert_eq!(tcp_client.url(), "192.168.1.100:8080");
        assert_eq!(tcp_client.reconnect_delay_ms(), 3000);
    }

    #[test]
    fn test_tcp_address_formats() {
        let ipv4_port = "127.0.0.1:9000";
        assert!(ipv4_port.contains(':'));
        assert!(ipv4_port.split(':').count() == 2);
        
        let hostname_port = "localhost:8080";
        assert!(hostname_port.split(':').count() == 2);
        
        let ipv6_port = "[::1]:9000";
        assert!(ipv6_port.starts_with('['));
    }

    #[test]
    fn test_stream_manager_load_mixed_protocols() {
        use crate::streams::config::StreamsConfig;
        
        let mut manager = StreamManager::new();
        
        let config = StreamsConfig {
            streams: vec![
                StreamDefinition {
                    id: "ws".to_string(),
                    name: "WS".to_string(),
                    protocol: StreamProtocol::WebSocket,
                    url: "wss://example.com".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 1000,
                    headers: Default::default(),
                },
                StreamDefinition {
                    id: "unix".to_string(),
                    name: "Unix".to_string(),
                    protocol: StreamProtocol::Unix,
                    url: "/tmp/app.sock".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 1000,
                    headers: Default::default(),
                },
                StreamDefinition {
                    id: "tcp".to_string(),
                    name: "TCP".to_string(),
                    protocol: StreamProtocol::Tcp,
                    url: "10.0.0.1:5000".to_string(),
                    auto_connect: false,
                    reconnect: true,
                    reconnect_delay_ms: 1000,
                    headers: Default::default(),
                },
            ],
        };
        
        manager.load_streams(&config);
        
        assert_eq!(manager.clients().len(), 3);
        assert_eq!(manager.get_client("ws").unwrap().protocol(), StreamProtocol::WebSocket);
        assert_eq!(manager.get_client("unix").unwrap().protocol(), StreamProtocol::Unix);
        assert_eq!(manager.get_client("tcp").unwrap().protocol(), StreamProtocol::Tcp);
    }
}
