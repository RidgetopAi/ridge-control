//! LSP client for a single language server
//!
//! Handles spawning, communication, and lifecycle of one language server.
//!
//! Note: Used indirectly through LSP tools in ToolExecutor.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::timeout;

use super::protocol::{IdGenerator, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseOut};
use super::types::*;

/// LSP client errors
#[derive(Debug)]
pub enum LspError {
    /// Server process not running
    NotRunning,
    /// Request timed out
    Timeout(u64),
    /// JSON-RPC error from server
    RpcError { code: i32, message: String },
    /// I/O error
    IoError(String),
    /// Parse error
    ParseError(String),
    /// Server initialization failed
    InitFailed(String),
    /// Server not initialized yet
    NotInitialized,
    /// Channel communication error
    ChannelError(String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspError::NotRunning => write!(f, "LSP server not running"),
            LspError::Timeout(secs) => write!(f, "Request timed out after {}s", secs),
            LspError::RpcError { code, message } => {
                write!(f, "JSON-RPC error {}: {}", code, message)
            }
            LspError::IoError(msg) => write!(f, "IO error: {}", msg),
            LspError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            LspError::InitFailed(msg) => write!(f, "Initialization failed: {}", msg),
            LspError::NotInitialized => write!(f, "Server not initialized"),
            LspError::ChannelError(msg) => write!(f, "Channel error: {}", msg),
        }
    }
}

impl std::error::Error for LspError {}

/// Pending request awaiting response
struct PendingRequest {
    tx: oneshot::Sender<Result<serde_json::Value, LspError>>,
}

/// LSP client for a single language server
pub struct LspClient {
    /// Server name (for logging)
    name: String,
    /// Server process handle
    process: Option<Child>,
    /// Channel to send requests to writer task
    request_tx: Option<mpsc::UnboundedSender<Vec<u8>>>,
    /// Pending requests by ID
    pending: Arc<RwLock<HashMap<i64, PendingRequest>>>,
    /// ID generator
    id_gen: IdGenerator,
    /// Workspace root URI
    root_uri: String,
    /// Request timeout
    timeout_secs: u64,
    /// Whether server is initialized
    initialized: bool,
    /// Server capabilities
    capabilities: ServerCapabilities,
    /// Indexing state - shared with reader task
    indexing_state: Arc<RwLock<IndexingState>>,
}

impl LspClient {
    /// Spawn a new language server
    pub async fn spawn(
        name: &str,
        command: &str,
        args: &[String],
        root_path: &PathBuf,
        init_options: serde_json::Value,
        timeout_secs: u64,
        env: &HashMap<String, String>,
    ) -> Result<Self, LspError> {
        tracing::info!("Spawning LSP server '{}': {} {:?}", name, command, args);

        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(root_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // Add custom environment variables
        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| LspError::IoError(format!("Failed to spawn {}: {}", command, e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::IoError("Failed to get stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::IoError("Failed to get stdout".into()))?;

        let pending: Arc<RwLock<HashMap<i64, PendingRequest>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Initialize indexing state - assume server is indexing until we know otherwise
        let indexing_state = Arc::new(RwLock::new(IndexingState::new_starting()));

        // Create channel for sending requests
        let (request_tx, request_rx) = mpsc::unbounded_channel::<Vec<u8>>();

        // Spawn reader task (also needs request_tx to respond to server requests)
        let pending_clone = pending.clone();
        let indexing_state_clone = indexing_state.clone();
        let name_clone = name.to_string();
        let request_tx_clone = request_tx.clone();
        tokio::spawn(async move {
            Self::reader_loop(stdout, pending_clone, indexing_state_clone, request_tx_clone, &name_clone).await;
        });

        // Spawn writer task
        let name_clone2 = name.to_string();
        tokio::spawn(async move {
            Self::writer_loop(stdin, request_rx, &name_clone2).await;
        });

        let root_uri = format!("file://{}", root_path.display());

        let mut client = Self {
            name: name.to_string(),
            process: Some(child),
            request_tx: Some(request_tx),
            pending,
            id_gen: IdGenerator::new(),
            root_uri,
            timeout_secs,
            initialized: false,
            capabilities: ServerCapabilities::default(),
            indexing_state,
        };

        // Initialize the server
        client.initialize(init_options).await?;

        Ok(client)
    }

    /// Reader loop - reads responses from stdout
    async fn reader_loop(
        stdout: ChildStdout,
        pending: Arc<RwLock<HashMap<i64, PendingRequest>>>,
        indexing_state: Arc<RwLock<IndexingState>>,
        request_tx: mpsc::UnboundedSender<Vec<u8>>,
        name: &str,
    ) {
        let mut reader = BufReader::new(stdout);

        loop {
            // Read headers until empty line
            let mut content_length: Option<usize> = None;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        tracing::debug!("LSP {} reader: EOF", name);
                        return;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            break;
                        }
                        if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                            if let Ok(len) = len_str.trim().parse::<usize>() {
                                content_length = Some(len);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("LSP {} reader error: {}", name, e);
                        return;
                    }
                }
            }

            // Read content
            let content_len = match content_length {
                Some(len) => len,
                None => {
                    tracing::warn!("LSP {} missing Content-Length header", name);
                    continue;
                }
            };

            let mut content = vec![0u8; content_len];
            if let Err(e) = reader.read_exact(&mut content).await {
                tracing::error!("LSP {} reader: failed to read body: {}", name, e);
                continue;
            }

            // Log raw message for debugging (truncated)
            let content_str = String::from_utf8_lossy(&content);
            let preview = if content_str.len() > 200 { &content_str[..200] } else { &content_str };
            tracing::debug!("LSP {} raw message: {}", name, preview);

            // First, try to detect server-to-client REQUEST (has both id and method)
            // These require us to send a response back
            if let Ok(server_request) = serde_json::from_slice::<ServerRequest>(&content) {
                tracing::info!("LSP {} detected server request: {} (id={:?})", name, server_request.method, server_request.id);
                Self::handle_server_request(&server_request, &request_tx, name).await;
                continue; // Don't process as response or notification
            }

            // Parse JSON-RPC message - could be response or notification
            match serde_json::from_slice::<JsonRpcResponse>(&content) {
                Ok(response) => {
                    if let Some(id) = response.id {
                        // This is a response to a request we sent
                        let mut pending = pending.write().await;
                        if let Some(req) = pending.remove(&id) {
                            let result = if let Some(err) = response.error {
                                Err(LspError::RpcError {
                                    code: err.code,
                                    message: err.message,
                                })
                            } else {
                                Ok(response.result.unwrap_or(serde_json::Value::Null))
                            };
                            let _ = req.tx.send(result);
                        } else {
                            // Response for unknown/timed-out request
                            tracing::debug!("LSP {} received response for unknown id: {:?}", name, id);
                        }
                        // Responses with id are NOT notifications - don't try to parse as one
                        continue;
                    }
                    // If no id, it's parsed as response but might be notification - fall through
                }
                Err(_) => {
                    // Not a valid response, try as notification
                }
            }

            // Try to parse as notification (server -> client message with no id)
            if let Ok(notification) = serde_json::from_slice::<IncomingNotification>(&content) {
                tracing::debug!("LSP {} notification: {}", name, notification.method);
                Self::handle_notification(&notification, &indexing_state, name).await;
            } else {
                tracing::warn!("LSP {} could not parse message as notification: {}", name, preview);
            }
        }
    }

    /// Handle server-to-client request (requires response)
    async fn handle_server_request(
        request: &ServerRequest,
        request_tx: &mpsc::UnboundedSender<Vec<u8>>,
        name: &str,
    ) {
        tracing::debug!("LSP {} server request: {} (id={:?})", name, request.method, request.id);

        match request.method.as_str() {
            "window/workDoneProgress/create" => {
                // Server wants to create a progress token - acknowledge it
                tracing::info!("LSP {} acknowledging workDoneProgress/create", name);
                let response = JsonRpcResponseOut::success_null(request.id.clone());
                if let Ok(encoded) = response.encode() {
                    let _ = request_tx.send(encoded);
                }
            }
            "client/registerCapability" => {
                // Server wants to register dynamic capabilities - acknowledge
                tracing::debug!("LSP {} acknowledging registerCapability", name);
                let response = JsonRpcResponseOut::success_null(request.id.clone());
                if let Ok(encoded) = response.encode() {
                    let _ = request_tx.send(encoded);
                }
            }
            "window/showMessageRequest" => {
                // Server wants to show a message with actions - respond with null (no action taken)
                tracing::debug!("LSP {} responding to showMessageRequest with null", name);
                let response = JsonRpcResponseOut::success_null(request.id.clone());
                if let Ok(encoded) = response.encode() {
                    let _ = request_tx.send(encoded);
                }
            }
            _ => {
                // Unknown request - respond with null to avoid blocking the server
                tracing::warn!("LSP {} unknown server request: {}", name, request.method);
                let response = JsonRpcResponseOut::success_null(request.id.clone());
                if let Ok(encoded) = response.encode() {
                    let _ = request_tx.send(encoded);
                }
            }
        }
    }

    /// Handle incoming notification from server
    async fn handle_notification(
        notification: &IncomingNotification,
        indexing_state: &Arc<RwLock<IndexingState>>,
        name: &str,
    ) {
        match notification.method.as_str() {
            "$/progress" => {
                if let Some(params) = &notification.params {
                    match serde_json::from_value::<ProgressParams>(params.clone()) {
                        Ok(progress) => {
                            tracing::info!("LSP {} progress received: token={}", name, progress.token);
                            Self::handle_progress(&progress, indexing_state, name).await;
                        }
                        Err(e) => {
                            tracing::warn!("LSP {} failed to parse progress params: {} - raw: {:?}", name, e, params);
                        }
                    }
                } else {
                    tracing::warn!("LSP {} progress notification without params", name);
                }
            }
            "window/logMessage" | "window/showMessage" => {
                // Log but don't spam - these are informational
                if let Some(params) = &notification.params {
                    if let Some(msg) = params.get("message").and_then(|v| v.as_str()) {
                        tracing::debug!("LSP {} message: {}", name, msg);
                    }
                }
            }
            "textDocument/publishDiagnostics" => {
                // Ignore diagnostics for now - we could track them later
                tracing::trace!("LSP {} diagnostics received", name);
            }
            _ => {
                tracing::trace!("LSP {} unhandled notification: {}", name, notification.method);
            }
        }
    }

    /// Handle progress notification - update indexing state
    async fn handle_progress(
        progress: &ProgressParams,
        indexing_state: &Arc<RwLock<IndexingState>>,
        name: &str,
    ) {
        let token_str = progress.token.to_string();

        // Check if token itself indicates indexing work (string tokens)
        let is_indexing_token = token_str.contains("Indexing")
            || token_str.contains("Building")
            || token_str.contains("Roots Scanned")
            || token_str.contains("Fetching")
            || token_str.contains("Loading")
            || token_str.starts_with("rustAnalyzer/")
            || token_str.starts_with("rust-analyzer/");

        // Check the title in Begin messages - tokens are often numeric!
        let title_indicates_work = match &progress.value {
            ProgressValue::Begin { title, .. } => {
                title.contains("Indexing")
                    || title.contains("Building")
                    || title.contains("Loading")
                    || title.contains("Fetching")
                    || title.contains("Roots Scanned")
                    || title.contains("cargo")
                    || title.contains("Analyzing")
            }
            _ => false,
        };

        // Check if this token is already being tracked (for Report/End of numeric tokens)
        let already_tracking = {
            let state = indexing_state.read().await;
            state.active_tokens.contains(&token_str)
        };

        // Process if: token name matches, title indicates work, or we're already tracking it
        let should_process = is_indexing_token || title_indicates_work || already_tracking;

        if should_process {
            let mut state = indexing_state.write().await;

            match &progress.value {
                ProgressValue::Begin { title, message, percentage, .. } => {
                    // Track this token as active
                    state.begin_token(token_str.clone());
                    state.title = Some(title.clone());
                    state.message = message.clone();
                    state.percentage = *percentage;
                    tracing::info!(
                        "LSP {} work started [{}]: {} ({}%) - {} active",
                        name,
                        token_str,
                        title,
                        percentage.unwrap_or(0),
                        state.active_tokens.len()
                    );
                }
                ProgressValue::Report { message, percentage, .. } => {
                    // Update progress info
                    if message.is_some() {
                        state.message = message.clone();
                    }
                    if percentage.is_some() {
                        state.percentage = *percentage;
                    }
                    tracing::debug!(
                        "LSP {} progress [{}]: {}% - {:?}",
                        name,
                        token_str,
                        percentage.unwrap_or(0),
                        message
                    );
                }
                ProgressValue::End { message } => {
                    // Remove this token from active set
                    let all_done = state.end_token(&token_str);
                    state.message = message.clone();

                    if all_done {
                        state.percentage = Some(100);
                        state.title = None;
                        tracing::info!(
                            "LSP {} all work complete (last: {}): {:?}",
                            name,
                            token_str,
                            message
                        );
                    } else {
                        tracing::debug!(
                            "LSP {} work ended [{}]: {:?} - {} still active",
                            name,
                            token_str,
                            message,
                            state.active_tokens.len()
                        );
                    }
                }
            }
        }
    }

    /// Writer loop - writes requests to stdin
    async fn writer_loop(
        mut stdin: ChildStdin,
        mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
        name: &str,
    ) {
        while let Some(data) = rx.recv().await {
            if let Err(e) = stdin.write_all(&data).await {
                tracing::error!("LSP {} writer error: {}", name, e);
                break;
            }
            if let Err(e) = stdin.flush().await {
                tracing::error!("LSP {} flush error: {}", name, e);
                break;
            }
        }
        tracing::debug!("LSP {} writer loop ended", name);
    }

    /// Send a request and wait for response
    async fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, LspError> {
        let tx = self
            .request_tx
            .as_ref()
            .ok_or(LspError::NotRunning)?;

        let id = self.id_gen.next();
        let request = JsonRpcRequest::new(id, method, params);
        let encoded = request
            .encode()
            .map_err(|e| LspError::ParseError(e.to_string()))?;

        // Register pending request
        let (response_tx, response_rx) = oneshot::channel();
        self.pending
            .write()
            .await
            .insert(id, PendingRequest { tx: response_tx });

        // Send request
        tx.send(encoded)
            .map_err(|e| LspError::ChannelError(e.to_string()))?;

        // Wait with timeout
        timeout(Duration::from_secs(self.timeout_secs), response_rx)
            .await
            .map_err(|_| LspError::Timeout(self.timeout_secs))?
            .map_err(|_| LspError::ChannelError("Response channel closed".into()))?
    }

    /// Send a notification (no response expected)
    async fn send_notification(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), LspError> {
        let tx = self
            .request_tx
            .as_ref()
            .ok_or(LspError::NotRunning)?;

        let notification = JsonRpcNotification::new(method, params);
        let encoded = notification
            .encode()
            .map_err(|e| LspError::ParseError(e.to_string()))?;

        tx.send(encoded)
            .map_err(|e| LspError::ChannelError(e.to_string()))
    }

    /// Initialize the server
    async fn initialize(&mut self, init_options: serde_json::Value) -> Result<(), LspError> {
        let params = serde_json::json!({
            "processId": std::process::id(),
            "rootUri": self.root_uri,
            "capabilities": {
                "textDocument": {
                    "definition": { "linkSupport": true },
                    "references": {},
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "documentSymbol": {
                        "hierarchicalDocumentSymbolSupport": true
                    },
                    "implementation": {},
                    "callHierarchy": {}
                },
                "workspace": {
                    "symbol": { "dynamicRegistration": false }
                }
            },
            "initializationOptions": init_options
        });

        tracing::debug!("LSP {} sending initialize", self.name);

        let result = self.send_request("initialize", Some(params)).await?;

        // Parse capabilities
        if let Ok(init_result) = serde_json::from_value::<InitializeResult>(result) {
            self.capabilities = init_result.capabilities;
            if let Some(info) = init_result.server_info {
                tracing::info!(
                    "LSP {} initialized: {} {}",
                    self.name,
                    info.name,
                    info.version.unwrap_or_default()
                );
            }
        }

        // Send initialized notification
        self.send_notification("initialized", Some(serde_json::json!({})))
            .await?;
        self.initialized = true;

        // Clear the initial "Starting..." state if no progress tokens are active
        // The server may not send any progress notifications (small project, already indexed)
        // If it does send them, they will override this state
        {
            let mut state = self.indexing_state.write().await;
            if state.active_tokens.is_empty() {
                state.is_indexing = false;
                state.percentage = Some(100);
                state.message = None;
                state.title = None;
                tracing::debug!("LSP {} initialized - no active work, marking ready", self.name);
            }
        }

        Ok(())
    }

    /// Check if server is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get server name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get server capabilities
    pub fn capabilities(&self) -> &ServerCapabilities {
        &self.capabilities
    }

    /// Check if the server is currently indexing
    pub async fn is_indexing(&self) -> bool {
        self.indexing_state.read().await.is_indexing
    }

    /// Get the current indexing state
    pub async fn get_indexing_state(&self) -> IndexingState {
        self.indexing_state.read().await.clone()
    }

    /// Get indexing progress as a percentage (0-100), or None if unknown
    pub async fn indexing_progress(&self) -> Option<u32> {
        self.indexing_state.read().await.percentage
    }

    /// Get human-readable indexing status
    pub async fn indexing_status(&self) -> String {
        self.indexing_state.read().await.to_status_string()
    }

    // ========== LSP Operations ==========

    /// textDocument/definition
    pub async fn goto_definition(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let result = self
            .send_request("textDocument/definition", Some(params))
            .await?;

        // Result can be Location, Location[], or null
        if result.is_null() {
            return Ok(vec![]);
        }

        // Try as array first
        if let Ok(locations) = serde_json::from_value::<Vec<Location>>(result.clone()) {
            return Ok(locations);
        }

        // Try as single location
        if let Ok(location) = serde_json::from_value::<Location>(result) {
            return Ok(vec![location]);
        }

        Ok(vec![])
    }

    /// textDocument/references
    pub async fn find_references(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<Location>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        });

        let result = self
            .send_request("textDocument/references", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(result).map_err(|e| LspError::ParseError(e.to_string()))
    }

    /// textDocument/hover
    pub async fn hover(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Hover>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let result = self
            .send_request("textDocument/hover", Some(params))
            .await?;

        if result.is_null() {
            return Ok(None);
        }

        serde_json::from_value(result)
            .map(Some)
            .map_err(|e| LspError::ParseError(e.to_string()))
    }

    /// textDocument/documentSymbol
    pub async fn document_symbols(
        &mut self,
        uri: &str,
    ) -> Result<Vec<SymbolInformation>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });

        let result = self
            .send_request("textDocument/documentSymbol", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        // Response can be SymbolInformation[] or DocumentSymbol[]
        // Try SymbolInformation first
        if let Ok(symbols) = serde_json::from_value::<Vec<SymbolInformation>>(result.clone()) {
            return Ok(symbols);
        }

        // Try DocumentSymbol and flatten
        if let Ok(doc_symbols) = serde_json::from_value::<Vec<DocumentSymbol>>(result) {
            return Ok(Self::flatten_document_symbols(&doc_symbols, uri));
        }

        Ok(vec![])
    }

    /// Flatten hierarchical DocumentSymbols to flat SymbolInformation
    fn flatten_document_symbols(
        symbols: &[DocumentSymbol],
        uri: &str,
    ) -> Vec<SymbolInformation> {
        let mut result = Vec::new();
        Self::flatten_recursive(symbols, uri, None, &mut result);
        result
    }

    fn flatten_recursive(
        symbols: &[DocumentSymbol],
        uri: &str,
        container: Option<&str>,
        result: &mut Vec<SymbolInformation>,
    ) {
        for sym in symbols {
            result.push(SymbolInformation {
                name: sym.name.clone(),
                kind: sym.kind,
                location: Location {
                    uri: uri.to_string(),
                    range: sym.selection_range,
                },
                container_name: container.map(String::from),
            });

            if !sym.children.is_empty() {
                Self::flatten_recursive(&sym.children, uri, Some(&sym.name), result);
            }
        }
    }

    /// workspace/symbol
    pub async fn workspace_symbols(
        &mut self,
        query: &str,
    ) -> Result<Vec<SymbolInformation>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({ "query": query });

        let result = self
            .send_request("workspace/symbol", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(result).map_err(|e| LspError::ParseError(e.to_string()))
    }

    /// textDocument/implementation
    pub async fn goto_implementation(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<Location>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let result = self
            .send_request("textDocument/implementation", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        // Same pattern as definition
        if let Ok(locations) = serde_json::from_value::<Vec<Location>>(result.clone()) {
            return Ok(locations);
        }

        if let Ok(location) = serde_json::from_value::<Location>(result) {
            return Ok(vec![location]);
        }

        Ok(vec![])
    }

    /// textDocument/prepareCallHierarchy
    pub async fn prepare_call_hierarchy(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<CallHierarchyItem>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let result = self
            .send_request("textDocument/prepareCallHierarchy", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(result).map_err(|e| LspError::ParseError(e.to_string()))
    }

    /// callHierarchy/incomingCalls
    pub async fn incoming_calls(
        &mut self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyIncomingCall>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({ "item": item });

        let result = self
            .send_request("callHierarchy/incomingCalls", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(result).map_err(|e| LspError::ParseError(e.to_string()))
    }

    /// callHierarchy/outgoingCalls
    pub async fn outgoing_calls(
        &mut self,
        item: &CallHierarchyItem,
    ) -> Result<Vec<CallHierarchyOutgoingCall>, LspError> {
        if !self.initialized {
            return Err(LspError::NotInitialized);
        }

        let params = serde_json::json!({ "item": item });

        let result = self
            .send_request("callHierarchy/outgoingCalls", Some(params))
            .await?;

        if result.is_null() {
            return Ok(vec![]);
        }

        serde_json::from_value(result).map_err(|e| LspError::ParseError(e.to_string()))
    }

    // ========== Document Sync ==========

    /// textDocument/didOpen
    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i32,
        text: &str,
    ) -> Result<(), LspError> {
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text
            }
        });
        self.send_notification("textDocument/didOpen", Some(params))
            .await
    }

    /// textDocument/didClose
    pub async fn did_close(&self, uri: &str) -> Result<(), LspError> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });
        self.send_notification("textDocument/didClose", Some(params))
            .await
    }

    /// textDocument/didChange (full document sync)
    pub async fn did_change(
        &self,
        uri: &str,
        version: i32,
        text: &str,
    ) -> Result<(), LspError> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }]
        });
        self.send_notification("textDocument/didChange", Some(params))
            .await
    }

    // ========== Lifecycle ==========

    /// Shutdown the server gracefully
    pub async fn shutdown(&mut self) -> Result<(), LspError> {
        if !self.initialized {
            return Ok(());
        }

        tracing::info!("Shutting down LSP server: {}", self.name);

        // Send shutdown request
        let _ = self.send_request("shutdown", None).await;

        // Send exit notification
        let _ = self.send_notification("exit", None).await;

        // Close channel
        self.request_tx = None;

        // Kill process if still running
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
        }

        self.initialized = false;
        Ok(())
    }

    /// Check if the server process is still running
    pub fn is_running(&self) -> bool {
        self.request_tx.is_some()
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Best-effort cleanup - can't do async in Drop
        if let Some(mut process) = self.process.take() {
            let _ = process.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_error_display() {
        let err = LspError::Timeout(30);
        assert_eq!(err.to_string(), "Request timed out after 30s");

        let err = LspError::RpcError {
            code: -32600,
            message: "Invalid Request".into(),
        };
        assert!(err.to_string().contains("-32600"));
        assert!(err.to_string().contains("Invalid Request"));
    }
}
