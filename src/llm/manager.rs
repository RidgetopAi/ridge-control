use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;

use super::anthropic::AnthropicProvider;
use super::provider::{Provider, ProviderRegistry};
use super::types::{LLMError, LLMRequest, Message, StreamChunk};

/// Event from the LLM subsystem
#[derive(Debug, Clone)]
pub enum LLMEvent {
    Chunk(StreamChunk),
    Complete,
    Error(LLMError),
}

/// Manages LLM providers and handles streaming requests
pub struct LLMManager {
    registry: ProviderRegistry,
    current_provider: String,
    current_model: String,
    conversation: Vec<Message>,
    event_tx: mpsc::UnboundedSender<LLMEvent>,
    event_rx: Option<mpsc::UnboundedReceiver<LLMEvent>>,
    cancel_tx: Option<mpsc::Sender<()>>,
}

impl LLMManager {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        Self {
            registry: ProviderRegistry::new(),
            current_provider: String::new(),
            current_model: String::new(),
            conversation: Vec::new(),
            event_tx,
            event_rx: Some(event_rx),
            cancel_tx: None,
        }
    }

    pub fn register_anthropic(&mut self, api_key: impl Into<String>) {
        let provider = Arc::new(AnthropicProvider::new(api_key));
        let default_model = provider.default_model().to_string();
        let name = provider.name().to_string();

        self.registry.register(provider);

        if self.current_provider.is_empty() {
            self.current_provider = name;
            self.current_model = default_model;
        }
    }

    pub fn take_event_rx(&mut self) -> Option<mpsc::UnboundedReceiver<LLMEvent>> {
        self.event_rx.take()
    }

    pub fn current_provider(&self) -> &str {
        &self.current_provider
    }

    pub fn current_model(&self) -> &str {
        &self.current_model
    }

    pub fn set_provider(&mut self, name: &str) {
        if let Some(provider) = self.registry.get(name) {
            self.current_provider = name.to_string();
            self.current_model = provider.default_model().to_string();
        }
    }

    pub fn set_model(&mut self, model: &str) {
        self.current_model = model.to_string();
    }

    pub fn conversation(&self) -> &[Message] {
        &self.conversation
    }

    pub fn clear_conversation(&mut self) {
        self.conversation.clear();
    }

    pub fn add_user_message(&mut self, text: String) {
        self.conversation.push(Message::user(text));
    }

    pub fn add_assistant_message(&mut self, text: String) {
        self.conversation.push(Message::assistant(text));
    }

    pub fn is_configured(&self) -> bool {
        !self.current_provider.is_empty() && self.registry.get(&self.current_provider).is_some()
    }

    pub fn cancel(&mut self) {
        if let Some(tx) = self.cancel_tx.take() {
            let _ = tx.try_send(());
        }
    }

    pub fn send_message(&mut self, user_message: String, system_prompt: Option<String>) {
        self.add_user_message(user_message);

        let provider = match self.registry.get(&self.current_provider) {
            Some(p) => p,
            None => {
                let _ = self.event_tx.send(LLMEvent::Error(LLMError::ProviderError {
                    status: 0,
                    message: "No provider configured".to_string(),
                }));
                return;
            }
        };

        let request = LLMRequest {
            model: self.current_model.clone(),
            system: system_prompt,
            messages: self.conversation.clone(),
            stream: true,
            ..Default::default()
        };

        let event_tx = self.event_tx.clone();
        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
        self.cancel_tx = Some(cancel_tx);

        tokio::spawn(async move {
            match provider.stream(request).await {
                Ok(mut stream) => {
                    loop {
                        tokio::select! {
                            chunk = stream.next() => {
                                match chunk {
                                    Some(Ok(c)) => {
                                        if event_tx.send(LLMEvent::Chunk(c)).is_err() {
                                            break;
                                        }
                                    }
                                    Some(Err(e)) => {
                                        let _ = event_tx.send(LLMEvent::Error(e));
                                        break;
                                    }
                                    None => {
                                        let _ = event_tx.send(LLMEvent::Complete);
                                        break;
                                    }
                                }
                            }
                            _ = cancel_rx.recv() => {
                                let _ = event_tx.send(LLMEvent::Error(LLMError::StreamInterrupted));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = event_tx.send(LLMEvent::Error(e));
                }
            }
        });
    }
}

impl Default for LLMManager {
    fn default() -> Self {
        Self::new()
    }
}
