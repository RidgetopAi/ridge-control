use std::sync::Arc;

use futures::StreamExt;
use tokio::sync::mpsc;

use crate::config::{KeyId, KeyStore, SecretString};

use super::anthropic::AnthropicProvider;
use super::gemini::GeminiProvider;
use super::grok::GrokProvider;
use super::groq::GroqProvider;
use super::openai::OpenAIProvider;
use super::provider::{Provider, ProviderRegistry};
use super::types::{LLMError, LLMRequest, Message, StreamChunk, ToolUse, ContentBlock, ToolResult};

/// Event from the LLM subsystem
#[derive(Debug, Clone)]
pub enum LLMEvent {
    Chunk(StreamChunk),
    Complete,
    Error(LLMError),
    /// Tool use detected, needs handling
    ToolUseDetected(ToolUse),
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

    pub fn register_gemini(&mut self, api_key: impl Into<String>) {
        let provider = Arc::new(GeminiProvider::new(api_key));
        let default_model = provider.default_model().to_string();
        let name = provider.name().to_string();

        self.registry.register(provider);

        if self.current_provider.is_empty() {
            self.current_provider = name;
            self.current_model = default_model;
        }
    }

    pub fn register_grok(&mut self, api_key: impl Into<String>) {
        let provider = Arc::new(GrokProvider::new(api_key));
        let default_model = provider.default_model().to_string();
        let name = provider.name().to_string();

        self.registry.register(provider);

        if self.current_provider.is_empty() {
            self.current_provider = name;
            self.current_model = default_model;
        }
    }

    pub fn register_openai(&mut self, api_key: impl Into<String>) {
        let provider = Arc::new(OpenAIProvider::new(api_key));
        let default_model = provider.default_model().to_string();
        let name = provider.name().to_string();

        self.registry.register(provider);

        if self.current_provider.is_empty() {
            self.current_provider = name;
            self.current_model = default_model;
        }
    }

    pub fn register_groq(&mut self, api_key: impl Into<String>) {
        let provider = Arc::new(GroqProvider::new(api_key));
        let default_model = provider.default_model().to_string();
        let name = provider.name().to_string();

        self.registry.register(provider);

        if self.current_provider.is_empty() {
            self.current_provider = name;
            self.current_model = default_model;
        }
    }

    /// Register all providers from a KeyStore
    /// Returns a list of successfully registered provider names
    pub fn register_from_keystore(&mut self, keystore: &KeyStore) -> Vec<String> {
        let mut registered = Vec::new();

        // Try to get each known provider's key
        let providers = [
            (KeyId::Anthropic, "anthropic"),
            (KeyId::OpenAI, "openai"),
            (KeyId::Gemini, "gemini"),
            (KeyId::Grok, "grok"),
            (KeyId::Groq, "groq"),
        ];

        for (key_id, name) in providers {
            if let Ok(Some(secret)) = keystore.get(&key_id) {
                match key_id {
                    KeyId::Anthropic => self.register_anthropic(secret.expose()),
                    KeyId::OpenAI => self.register_openai(secret.expose()),
                    KeyId::Gemini => self.register_gemini(secret.expose()),
                    KeyId::Grok => self.register_grok(secret.expose()),
                    KeyId::Groq => self.register_groq(secret.expose()),
                    KeyId::Custom(_) => continue,
                }
                registered.push(name.to_string());
                tracing::info!("Registered {} provider from keystore", name);
            }
        }

        registered
    }

    /// Check if a specific provider is registered
    pub fn has_provider(&self, name: &str) -> bool {
        self.registry.get(name).is_some()
    }

    /// Get list of registered provider names
    pub fn registered_providers(&self) -> Vec<String> {
        self.registry.list().into_iter().map(|s| s.to_string()).collect()
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
    
    /// Add a tool use from the assistant to the conversation
    pub fn add_tool_use(&mut self, tool_use: ToolUse) {
        // If the last message is from the assistant, add the tool use to it
        if let Some(last) = self.conversation.last_mut() {
            if matches!(last.role, super::types::Role::Assistant) {
                last.content.push(ContentBlock::ToolUse(tool_use));
                return;
            }
        }
        // Otherwise create a new assistant message with the tool use
        self.conversation.push(Message {
            role: super::types::Role::Assistant,
            content: vec![ContentBlock::ToolUse(tool_use)],
        });
    }
    
    /// Add a tool result from the user to the conversation
    pub fn add_tool_result(&mut self, result: ToolResult) {
        // Tool results are added as user messages
        self.conversation.push(Message {
            role: super::types::Role::User,
            content: vec![ContentBlock::ToolResult(result)],
        });
    }
    
    /// Continue the conversation after a tool result (re-send to get LLM response)
    pub fn continue_after_tool(&mut self, system_prompt: Option<String>) {
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
