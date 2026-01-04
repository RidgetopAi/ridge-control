//! LSP Manager - coordinates multiple language servers
//!
//! Handles server lifecycle, routing requests to appropriate servers,
//! and document synchronization.
//!
//! Note: Used indirectly through LSP tools in llm::ToolExecutor.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;

use super::client::{LspClient, LspError};
use super::document::LspDocumentTracker;
use super::types::*;
use crate::config::lsp::LspConfig;

/// Manages multiple LSP servers
pub struct LspManager {
    /// Configuration
    config: LspConfig,
    /// Active clients by server name
    clients: HashMap<String, Arc<RwLock<LspClient>>>,
    /// Document tracker
    documents: LspDocumentTracker,
    /// Current working directory
    working_dir: PathBuf,
}

impl LspManager {
    /// Create a new LSP manager
    pub fn new(config: LspConfig, working_dir: PathBuf) -> Self {
        Self {
            config,
            clients: HashMap::new(),
            documents: LspDocumentTracker::new(),
            working_dir,
        }
    }

    /// Update configuration (for hot-reload)
    pub fn set_config(&mut self, config: LspConfig) {
        self.config = config;
    }

    /// Check if LSP is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the working directory
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    /// Set the working directory
    pub fn set_working_dir(&mut self, path: PathBuf) {
        self.working_dir = path;
    }

    /// Get or start the appropriate server for a file
    pub async fn get_client_for_file(
        &mut self,
        file_path: &str,
    ) -> Result<Arc<RwLock<LspClient>>, LspError> {
        if !self.config.enabled {
            return Err(LspError::InitFailed("LSP is disabled".into()));
        }

        let ext = Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let (server_name, server_config) = self
            .config
            .server_for_extension(&ext)
            .ok_or_else(|| {
                LspError::InitFailed(format!("No LSP server configured for .{} files", ext))
            })?;

        // Return existing client if available and running
        if let Some(client) = self.clients.get(server_name) {
            let client_read = client.read().await;
            if client_read.is_running() {
                drop(client_read);
                return Ok(client.clone());
            }
            // Client exists but not running, remove it
            drop(client_read);
            self.clients.remove(server_name);
        }

        // Find workspace root
        let workspace_root = self.find_workspace_root(file_path, &server_config.root_patterns);

        // Start new server
        tracing::info!(
            "Starting LSP server '{}' for {} files in {}",
            server_name,
            ext,
            workspace_root.display()
        );

        let client = LspClient::spawn(
            server_name,
            &server_config.command,
            &server_config.args,
            &workspace_root,
            server_config.init_options.clone(),
            server_config.timeout_secs,
            &server_config.env,
        )
        .await?;

        let client = Arc::new(RwLock::new(client));
        self.clients.insert(server_name.to_string(), client.clone());

        Ok(client)
    }

    /// Find workspace root by looking for root pattern files
    fn find_workspace_root(&self, file_path: &str, root_patterns: &[String]) -> PathBuf {
        let file_path = Path::new(file_path);

        // Start from file's directory and walk up
        let mut current = if file_path.is_file() {
            file_path.parent().map(|p| p.to_path_buf())
        } else {
            Some(file_path.to_path_buf())
        };

        while let Some(dir) = current {
            for pattern in root_patterns {
                if dir.join(pattern).exists() {
                    return dir;
                }
            }
            current = dir.parent().map(|p| p.to_path_buf());
        }

        // Fall back to working directory
        self.working_dir.clone()
    }

    /// Ensure a document is open in the appropriate server
    pub async fn ensure_document_open(&mut self, file_path: &str) -> Result<(), LspError> {
        let uri = LspDocumentTracker::path_to_uri(file_path);

        if self.documents.is_open(&uri) {
            return Ok(());
        }

        // Read file content
        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| LspError::IoError(format!("Failed to read {}: {}", file_path, e)))?;

        let language_id = LspDocumentTracker::extension_to_language_id(file_path);

        // Get client and open document
        let client = self.get_client_for_file(file_path).await?;
        {
            let client = client.read().await;
            client.did_open(&uri, language_id, 1, &content).await?;
        }

        self.documents.mark_open(&uri, language_id, &content);
        Ok(())
    }

    /// Close a document
    pub async fn close_document(&mut self, file_path: &str) -> Result<(), LspError> {
        let uri = LspDocumentTracker::path_to_uri(file_path);

        if !self.documents.is_open(&uri) {
            return Ok(());
        }

        // Get client and close document
        if let Ok(client) = self.get_client_for_file(file_path).await {
            let client = client.read().await;
            let _ = client.did_close(&uri).await;
        }

        self.documents.mark_closed(&uri);
        Ok(())
    }

    // ========== LSP Operations ==========

    /// Go to definition
    pub async fn goto_definition(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationDisplay>, LspError> {
        self.ensure_document_open(file_path).await?;

        let uri = LspDocumentTracker::path_to_uri(file_path);
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        // Convert from 1-indexed to 0-indexed
        let locations = client
            .goto_definition(&uri, line.saturating_sub(1), character.saturating_sub(1))
            .await?;

        Ok(locations.into_iter().map(|l| l.to_display()).collect())
    }

    /// Find references
    pub async fn find_references(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
        include_declaration: bool,
    ) -> Result<Vec<LocationDisplay>, LspError> {
        self.ensure_document_open(file_path).await?;

        let uri = LspDocumentTracker::path_to_uri(file_path);
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let locations = client
            .find_references(
                &uri,
                line.saturating_sub(1),
                character.saturating_sub(1),
                include_declaration,
            )
            .await?;

        Ok(locations.into_iter().map(|l| l.to_display()).collect())
    }

    /// Get hover info
    pub async fn hover(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<String>, LspError> {
        self.ensure_document_open(file_path).await?;

        let uri = LspDocumentTracker::path_to_uri(file_path);
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let hover = client
            .hover(&uri, line.saturating_sub(1), character.saturating_sub(1))
            .await?;

        Ok(hover.map(|h| h.contents.to_text()))
    }

    /// Get document symbols
    pub async fn document_symbols(
        &mut self,
        file_path: &str,
    ) -> Result<Vec<SymbolInfo>, LspError> {
        self.ensure_document_open(file_path).await?;

        let uri = LspDocumentTracker::path_to_uri(file_path);
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let symbols = client.document_symbols(&uri).await?;

        Ok(symbols
            .into_iter()
            .map(|s| SymbolInfo {
                name: s.name,
                kind: s.kind.as_str().to_string(),
                file: s.location.file_path().to_string(),
                line: s.location.range.start.line + 1,
                character: s.location.range.start.character + 1,
                container: s.container_name,
            })
            .collect())
    }

    /// Search workspace symbols
    pub async fn workspace_symbols(
        &mut self,
        query: &str,
        file_path: &str,
    ) -> Result<Vec<SymbolInfo>, LspError> {
        // Use file_path to determine which server to query
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let symbols = client.workspace_symbols(query).await?;

        Ok(symbols
            .into_iter()
            .map(|s| SymbolInfo {
                name: s.name,
                kind: s.kind.as_str().to_string(),
                file: s.location.file_path().to_string(),
                line: s.location.range.start.line + 1,
                character: s.location.range.start.character + 1,
                container: s.container_name,
            })
            .collect())
    }

    /// Go to implementation
    pub async fn goto_implementation(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<LocationDisplay>, LspError> {
        self.ensure_document_open(file_path).await?;

        let uri = LspDocumentTracker::path_to_uri(file_path);
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        let locations = client
            .goto_implementation(&uri, line.saturating_sub(1), character.saturating_sub(1))
            .await?;

        Ok(locations.into_iter().map(|l| l.to_display()).collect())
    }

    /// Get call hierarchy (incoming or outgoing)
    pub async fn call_hierarchy(
        &mut self,
        file_path: &str,
        line: u32,
        character: u32,
        incoming: bool,
    ) -> Result<Vec<CallInfo>, LspError> {
        self.ensure_document_open(file_path).await?;

        let uri = LspDocumentTracker::path_to_uri(file_path);
        let client = self.get_client_for_file(file_path).await?;
        let mut client = client.write().await;

        // First, prepare call hierarchy
        let items = client
            .prepare_call_hierarchy(&uri, line.saturating_sub(1), character.saturating_sub(1))
            .await?;

        if items.is_empty() {
            return Ok(vec![]);
        }

        let item = &items[0];

        if incoming {
            let calls = client.incoming_calls(item).await?;
            Ok(calls
                .into_iter()
                .map(|c| CallInfo {
                    name: c.from.name,
                    kind: c.from.kind.as_str().to_string(),
                    file: LspDocumentTracker::uri_to_path(&c.from.uri).to_string(),
                    line: c.from.selection_range.start.line + 1,
                    character: c.from.selection_range.start.character + 1,
                })
                .collect())
        } else {
            let calls = client.outgoing_calls(item).await?;
            Ok(calls
                .into_iter()
                .map(|c| CallInfo {
                    name: c.to.name,
                    kind: c.to.kind.as_str().to_string(),
                    file: LspDocumentTracker::uri_to_path(&c.to.uri).to_string(),
                    line: c.to.selection_range.start.line + 1,
                    character: c.to.selection_range.start.character + 1,
                })
                .collect())
        }
    }

    // ========== Indexing State ==========

    /// Check if the server for a file is currently indexing
    pub async fn is_indexing_for_file(&mut self, file_path: &str) -> bool {
        if let Ok(client) = self.get_client_for_file(file_path).await {
            let client = client.read().await;
            client.is_indexing().await
        } else {
            false
        }
    }

    /// Get indexing status for a file's language server
    pub async fn get_indexing_status(&mut self, file_path: &str) -> Option<IndexingState> {
        if let Ok(client) = self.get_client_for_file(file_path).await {
            let client = client.read().await;
            Some(client.get_indexing_state().await)
        } else {
            None
        }
    }

    /// Get a human-readable indexing status string for a file's language server
    pub async fn indexing_status_string(&mut self, file_path: &str) -> String {
        if let Ok(client) = self.get_client_for_file(file_path).await {
            let client = client.read().await;
            client.indexing_status().await
        } else {
            "No server".to_string()
        }
    }

    /// Check if any running server is currently indexing
    pub async fn any_server_indexing(&self) -> bool {
        for client in self.clients.values() {
            let client = client.read().await;
            if client.is_indexing().await {
                return true;
            }
        }
        false
    }

    /// Get indexing status for all running servers
    pub async fn all_indexing_status(&self) -> Vec<(String, IndexingState)> {
        let mut statuses = Vec::new();
        for (name, client) in &self.clients {
            let client = client.read().await;
            let state = client.get_indexing_state().await;
            statuses.push((name.clone(), state));
        }
        statuses
    }

    // ========== Lifecycle ==========

    /// Get list of running servers
    pub fn running_servers(&self) -> Vec<&str> {
        self.clients.keys().map(|s| s.as_str()).collect()
    }

    /// Shutdown a specific server
    pub async fn shutdown_server(&mut self, name: &str) {
        if let Some(client) = self.clients.remove(name) {
            let mut client = client.write().await;
            let _ = client.shutdown().await;
        }
    }

    /// Shutdown all servers
    pub async fn shutdown_all(&mut self) {
        let names: Vec<_> = self.clients.keys().cloned().collect();
        for name in names {
            self.shutdown_server(&name).await;
        }
        self.documents.clear();
    }
}

/// Symbol information for tool output
#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub character: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

/// Call information for tool output
#[derive(Debug, Clone, serde::Serialize)]
pub struct CallInfo {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: u32,
    pub character: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::lsp::LspConfig;

    #[test]
    fn test_manager_creation() {
        let config = LspConfig::default();
        let manager = LspManager::new(config, PathBuf::from("/tmp"));
        assert!(manager.is_enabled());
        assert!(manager.running_servers().is_empty());
    }

    #[test]
    fn test_find_workspace_root() {
        let config = LspConfig::default();
        let manager = LspManager::new(config, PathBuf::from("/tmp"));

        // When file is in project with Cargo.toml, should find it
        // (This would require actual files, so just test fallback)
        let root = manager.find_workspace_root("/nonexistent/path/file.rs", &["Cargo.toml".into()]);
        assert_eq!(root, PathBuf::from("/tmp"));
    }
}
