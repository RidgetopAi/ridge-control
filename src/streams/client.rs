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
}

#[derive(Debug, Clone)]
pub enum StreamData {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone)]
pub struct StreamClient {
    definition: StreamDefinition,
    state: ConnectionState,
    buffer: Vec<StreamData>,
}

impl StreamClient {
    pub fn new(definition: StreamDefinition) -> Self {
        Self {
            definition,
            state: ConnectionState::Disconnected,
            buffer: Vec::with_capacity(1000),
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

    pub fn state(&self) -> ConnectionState {
        self.state
    }

    pub fn set_state(&mut self, state: ConnectionState) {
        self.state = state;
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
        if let Some(client) = self.get_client_mut(id) {
            if matches!(client.state(), ConnectionState::Connected | ConnectionState::Connecting) {
                return;
            }

            client.set_state(ConnectionState::Connecting);
            let definition = client.definition.clone();
            let event_tx = self.event_tx.clone();

            match definition.protocol {
                StreamProtocol::WebSocket => {
                    tokio::spawn(async move {
                        Self::websocket_connect(definition, event_tx).await;
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
            let _ = self.event_tx.send(StreamEvent::Disconnected(id.to_string(), None));
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
}
