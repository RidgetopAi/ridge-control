use std::time::Duration;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

use super::activity_store::SharedActivityStore;
use super::types::{ActivityMessage, WebSocketMessage};

const DEFAULT_SPINDLES_URL: &str = "ws://localhost:8083/spindles";
const DEFAULT_RECONNECT_DELAY_MS: u64 = 5000;
const MAX_RECONNECT_ATTEMPTS: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpindlesConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed,
}

impl std::fmt::Display for SpindlesConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpindlesConnectionState::Disconnected => write!(f, "⭘ Disconnected"),
            SpindlesConnectionState::Connecting => write!(f, "◌ Connecting..."),
            SpindlesConnectionState::Connected => write!(f, "● Connected"),
            SpindlesConnectionState::Reconnecting { attempt } => {
                write!(f, "↻ Reconnecting ({}/{})", attempt, MAX_RECONNECT_ATTEMPTS)
            }
            SpindlesConnectionState::Failed => write!(f, "✕ Failed"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum SpindlesEvent {
    Connected,
    Disconnected(Option<String>),
    Activity(ActivityMessage),
    ConnectionAck { timestamp: String },
    Error(String),
    StateChanged(SpindlesConnectionState),
    ReconnectAttempt(u32),
    ReconnectGaveUp,
}

pub struct SpindlesStream {
    url: String,
    store: SharedActivityStore,
    state: SpindlesConnectionState,
    reconnect_enabled: bool,
    reconnect_delay_ms: u64,
    reconnect_attempt: u32,
    event_tx: mpsc::UnboundedSender<SpindlesEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<SpindlesEvent>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl SpindlesStream {
    pub fn new(store: SharedActivityStore) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            url: DEFAULT_SPINDLES_URL.to_string(),
            store,
            state: SpindlesConnectionState::Disconnected,
            reconnect_enabled: true,
            reconnect_delay_ms: DEFAULT_RECONNECT_DELAY_MS,
            reconnect_attempt: 0,
            event_tx,
            event_rx: Some(event_rx),
            shutdown_tx: None,
        }
    }

    pub fn with_url(mut self, url: &str) -> Self {
        self.url = url.to_string();
        self
    }

    pub fn with_reconnect(mut self, enabled: bool) -> Self {
        self.reconnect_enabled = enabled;
        self
    }

    pub fn with_reconnect_delay(mut self, delay_ms: u64) -> Self {
        self.reconnect_delay_ms = delay_ms;
        self
    }

    pub fn take_event_rx(&mut self) -> Option<mpsc::UnboundedReceiver<SpindlesEvent>> {
        self.event_rx.take()
    }

    pub fn event_tx(&self) -> mpsc::UnboundedSender<SpindlesEvent> {
        self.event_tx.clone()
    }

    pub fn state(&self) -> SpindlesConnectionState {
        self.state
    }

    pub fn set_state(&mut self, state: SpindlesConnectionState) {
        self.state = state;
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, SpindlesConnectionState::Connected)
    }

    pub fn connect(&mut self) {
        if matches!(
            self.state,
            SpindlesConnectionState::Connected | SpindlesConnectionState::Connecting
        ) {
            return;
        }

        self.state = SpindlesConnectionState::Connecting;
        self.reconnect_attempt = 0;

        let url = self.url.clone();
        let store = self.store.clone();
        let event_tx = self.event_tx.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        tokio::spawn(async move {
            Self::run_connection(url, store, event_tx, shutdown_rx).await;
        });
    }

    pub fn disconnect(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }
        self.state = SpindlesConnectionState::Disconnected;
        self.reconnect_attempt = 0;
        let _ = self.event_tx.send(SpindlesEvent::Disconnected(None));
        let _ = self
            .event_tx
            .send(SpindlesEvent::StateChanged(SpindlesConnectionState::Disconnected));
    }

    pub fn start_reconnect(&mut self) {
        if !self.reconnect_enabled {
            return;
        }

        self.reconnect_attempt += 1;

        if self.reconnect_attempt > MAX_RECONNECT_ATTEMPTS {
            self.state = SpindlesConnectionState::Failed;
            let _ = self.event_tx.send(SpindlesEvent::ReconnectGaveUp);
            let _ = self
                .event_tx
                .send(SpindlesEvent::StateChanged(SpindlesConnectionState::Failed));
            return;
        }

        self.state = SpindlesConnectionState::Reconnecting {
            attempt: self.reconnect_attempt,
        };
        let _ = self
            .event_tx
            .send(SpindlesEvent::ReconnectAttempt(self.reconnect_attempt));
        let _ = self
            .event_tx
            .send(SpindlesEvent::StateChanged(self.state));

        let delay = self.backoff_delay();
        let url = self.url.clone();
        let store = self.store.clone();
        let event_tx = self.event_tx.clone();
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            Self::run_connection(url, store, event_tx, shutdown_rx).await;
        });
    }

    fn backoff_delay(&self) -> Duration {
        let attempt = self.reconnect_attempt.min(10);
        let exponential_delay = self.reconnect_delay_ms * (2_u64.pow(attempt));
        let max_delay = 60_000;
        let delay = exponential_delay.min(max_delay);

        let jitter_range = delay / 5;
        let jitter = if jitter_range > 0 {
            fastrand::u64(0..jitter_range * 2).saturating_sub(jitter_range)
        } else {
            0
        };
        Duration::from_millis(delay.saturating_add(jitter))
    }

    async fn run_connection(
        url: String,
        store: SharedActivityStore,
        event_tx: mpsc::UnboundedSender<SpindlesEvent>,
        mut shutdown_rx: mpsc::Receiver<()>,
    ) {
        let connect_result = connect_async(&url).await;

        match connect_result {
            Ok((ws_stream, _response)) => {
                let _ = event_tx.send(SpindlesEvent::StateChanged(SpindlesConnectionState::Connected));
                let _ = event_tx.send(SpindlesEvent::Connected);

                let (_write, mut read) = ws_stream.split();

                loop {
                    tokio::select! {
                        _ = shutdown_rx.recv() => {
                            break;
                        }
                        msg_opt = read.next() => {
                            match msg_opt {
                                Some(Ok(msg)) => {
                                    Self::handle_message(msg, &store, &event_tx);
                                }
                                Some(Err(e)) => {
                                    let _ = event_tx.send(SpindlesEvent::Error(e.to_string()));
                                    let _ = event_tx.send(SpindlesEvent::Disconnected(Some(e.to_string())));
                                    break;
                                }
                                None => {
                                    let _ = event_tx.send(SpindlesEvent::Disconnected(Some("Connection closed".to_string())));
                                    break;
                                }
                            }
                        }
                    }
                }

                let _ = event_tx.send(SpindlesEvent::StateChanged(SpindlesConnectionState::Disconnected));
            }
            Err(e) => {
                let _ = event_tx.send(SpindlesEvent::StateChanged(SpindlesConnectionState::Failed));
                let _ = event_tx.send(SpindlesEvent::Error(e.to_string()));
            }
        }
    }

    fn handle_message(
        msg: WsMessage,
        store: &SharedActivityStore,
        event_tx: &mpsc::UnboundedSender<SpindlesEvent>,
    ) {
        let text = match msg {
            WsMessage::Text(t) => t.to_string(),
            WsMessage::Binary(b) => match String::from_utf8(b.to_vec()) {
                Ok(s) => s,
                Err(_) => return,
            },
            WsMessage::Close(_) => {
                let _ = event_tx.send(SpindlesEvent::Disconnected(Some("Close frame received".to_string())));
                return;
            }
            _ => return,
        };

        match serde_json::from_str::<WebSocketMessage>(&text) {
            Ok(WebSocketMessage::Activity(activity)) => {
                if let Ok(mut guard) = store.lock() {
                    guard.push(activity.clone());
                }
                let _ = event_tx.send(SpindlesEvent::Activity(activity));
            }
            Ok(WebSocketMessage::ConnectionAck(ack)) => {
                let _ = event_tx.send(SpindlesEvent::ConnectionAck {
                    timestamp: ack.timestamp,
                });
            }
            Err(e) => {
                let _ = event_tx.send(SpindlesEvent::Error(format!(
                    "Failed to parse message: {} - {}",
                    e,
                    &text[..text.len().min(100)]
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spindles::activity_store::new_shared_store;

    #[test]
    fn test_spindles_stream_new() {
        let store = new_shared_store(100);
        let stream = SpindlesStream::new(store);
        assert_eq!(stream.state(), SpindlesConnectionState::Disconnected);
        assert!(!stream.is_connected());
    }

    #[test]
    fn test_spindles_stream_with_url() {
        let store = new_shared_store(100);
        let stream = SpindlesStream::new(store)
            .with_url("ws://custom:9999/path");
        assert_eq!(stream.url, "ws://custom:9999/path");
    }

    #[test]
    fn test_spindles_stream_with_reconnect() {
        let store = new_shared_store(100);
        let stream = SpindlesStream::new(store)
            .with_reconnect(false);
        assert!(!stream.reconnect_enabled);
    }

    #[test]
    fn test_state_display() {
        assert!(SpindlesConnectionState::Disconnected.to_string().contains("Disconnected"));
        assert!(SpindlesConnectionState::Connecting.to_string().contains("Connecting"));
        assert!(SpindlesConnectionState::Connected.to_string().contains("Connected"));
        assert!(SpindlesConnectionState::Reconnecting { attempt: 3 }
            .to_string()
            .contains("3/10"));
        assert!(SpindlesConnectionState::Failed.to_string().contains("Failed"));
    }

    #[test]
    fn test_backoff_delay_increases() {
        let store = new_shared_store(100);
        let mut stream = SpindlesStream::new(store);
        
        stream.reconnect_attempt = 1;
        let delay1 = stream.backoff_delay();
        
        stream.reconnect_attempt = 3;
        let delay3 = stream.backoff_delay();
        
        assert!(delay3 > delay1);
    }

    #[test]
    fn test_parse_activity_message() {
        let store = new_shared_store(100);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let json = r#"{"type":"thinking","content":"Let me analyze...","timestamp":"2026-01-17T12:00:00Z","session":null}"#;
        let msg = WsMessage::Text(json.into());
        SpindlesStream::handle_message(msg, &store, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            SpindlesEvent::Activity(ActivityMessage::Thinking(a)) => {
                assert_eq!(a.content, "Let me analyze...");
            }
            _ => panic!("Expected Activity event"),
        }

        let guard = store.lock().unwrap();
        assert_eq!(guard.len(), 1);
    }

    #[test]
    fn test_parse_connection_ack() {
        let store = new_shared_store(100);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let json = r#"{"type":"connection_ack","timestamp":"2026-01-17T12:00:00Z"}"#;
        let msg = WsMessage::Text(json.into());
        SpindlesStream::handle_message(msg, &store, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            SpindlesEvent::ConnectionAck { timestamp } => {
                assert_eq!(timestamp, "2026-01-17T12:00:00Z");
            }
            _ => panic!("Expected ConnectionAck event"),
        }

        let guard = store.lock().unwrap();
        assert!(guard.is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let store = new_shared_store(100);
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let json = r#"{"invalid": "message"}"#;
        let msg = WsMessage::Text(json.into());
        SpindlesStream::handle_message(msg, &store, &event_tx);

        let event = event_rx.try_recv().unwrap();
        match event {
            SpindlesEvent::Error(e) => {
                assert!(e.contains("Failed to parse"));
            }
            _ => panic!("Expected Error event"),
        }
    }
}
