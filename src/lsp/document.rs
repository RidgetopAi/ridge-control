//! Document synchronization tracking for LSP
//!
//! Tracks which documents are "open" in the LSP sense,
//! managing didOpen/didClose notifications.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// State of an open document
#[derive(Debug, Clone)]
struct DocumentState {
    /// Version number for incremental updates
    version: i32,
    /// Hash of content for change detection
    content_hash: u64,
    /// Language identifier
    language_id: String,
}

/// Tracks open documents for LSP synchronization
#[derive(Debug, Default)]
pub struct LspDocumentTracker {
    /// Open documents: uri -> state
    open_docs: HashMap<String, DocumentState>,
}

impl LspDocumentTracker {
    pub fn new() -> Self {
        Self {
            open_docs: HashMap::new(),
        }
    }

    /// Check if a document is currently open
    pub fn is_open(&self, uri: &str) -> bool {
        self.open_docs.contains_key(uri)
    }

    /// Mark a document as open
    pub fn mark_open(&mut self, uri: &str, language_id: &str, content: &str) {
        let hash = Self::hash_content(content);
        self.open_docs.insert(
            uri.to_string(),
            DocumentState {
                version: 1,
                content_hash: hash,
                language_id: language_id.to_string(),
            },
        );
    }

    /// Mark a document as closed
    pub fn mark_closed(&mut self, uri: &str) {
        self.open_docs.remove(uri);
    }

    /// Get the current version of a document
    pub fn version(&self, uri: &str) -> Option<i32> {
        self.open_docs.get(uri).map(|s| s.version)
    }

    /// Check if document content has changed
    pub fn needs_sync(&self, uri: &str, content: &str) -> bool {
        match self.open_docs.get(uri) {
            Some(state) => state.content_hash != Self::hash_content(content),
            None => true,
        }
    }

    /// Update document version after a change
    pub fn update_version(&mut self, uri: &str, content: &str) -> Option<i32> {
        if let Some(state) = self.open_docs.get_mut(uri) {
            state.version += 1;
            state.content_hash = Self::hash_content(content);
            Some(state.version)
        } else {
            None
        }
    }

    /// Get list of open document URIs
    pub fn open_documents(&self) -> Vec<&str> {
        self.open_docs.keys().map(|s| s.as_str()).collect()
    }

    /// Clear all tracked documents
    pub fn clear(&mut self) {
        self.open_docs.clear();
    }

    /// Hash content for change detection
    fn hash_content(content: &str) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut hasher);
        hasher.finish()
    }

    /// Convert a file path to a file:// URI
    pub fn path_to_uri(path: &str) -> String {
        // Normalize path
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            // Try to make it absolute
            std::path::Path::new(path)
                .canonicalize()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.to_string())
        };
        format!("file://{}", path)
    }

    /// Extract file path from a file:// URI
    pub fn uri_to_path(uri: &str) -> &str {
        uri.strip_prefix("file://").unwrap_or(uri)
    }

    /// Get language ID from file extension
    pub fn extension_to_language_id(path: &str) -> &'static str {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext.to_lowercase().as_str() {
            // Rust
            "rs" => "rust",

            // TypeScript / JavaScript
            "ts" => "typescript",
            "tsx" => "typescriptreact",
            "js" => "javascript",
            "jsx" => "javascriptreact",
            "mjs" => "javascript",
            "cjs" => "javascript",

            // Python
            "py" => "python",
            "pyi" => "python",
            "pyw" => "python",

            // Go
            "go" => "go",

            // Java / Kotlin
            "java" => "java",
            "kt" => "kotlin",
            "kts" => "kotlin",

            // C / C++
            "c" => "c",
            "h" => "c",
            "cpp" | "cc" | "cxx" | "c++" => "cpp",
            "hpp" | "hh" | "hxx" | "h++" => "cpp",

            // C#
            "cs" => "csharp",

            // Ruby
            "rb" => "ruby",

            // PHP
            "php" => "php",

            // Swift
            "swift" => "swift",

            // Zig
            "zig" => "zig",

            // Web
            "html" | "htm" => "html",
            "css" => "css",
            "scss" => "scss",
            "less" => "less",
            "json" => "json",
            "yaml" | "yml" => "yaml",
            "toml" => "toml",
            "xml" => "xml",

            // Shell
            "sh" | "bash" => "shellscript",
            "zsh" => "shellscript",
            "fish" => "fish",

            // Config
            "md" | "markdown" => "markdown",
            "sql" => "sql",
            "dockerfile" => "dockerfile",

            // Default
            _ => "plaintext",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_lifecycle() {
        let mut tracker = LspDocumentTracker::new();
        let uri = "file:///test.rs";

        assert!(!tracker.is_open(uri));

        tracker.mark_open(uri, "rust", "fn main() {}");
        assert!(tracker.is_open(uri));
        assert_eq!(tracker.version(uri), Some(1));

        tracker.update_version(uri, "fn main() { println!() }");
        assert_eq!(tracker.version(uri), Some(2));

        tracker.mark_closed(uri);
        assert!(!tracker.is_open(uri));
    }

    #[test]
    fn test_needs_sync() {
        let mut tracker = LspDocumentTracker::new();
        let uri = "file:///test.rs";
        let content = "fn main() {}";

        // Not open, needs sync
        assert!(tracker.needs_sync(uri, content));

        tracker.mark_open(uri, "rust", content);

        // Same content, no sync needed
        assert!(!tracker.needs_sync(uri, content));

        // Different content, sync needed
        assert!(tracker.needs_sync(uri, "fn main() { changed }"));
    }

    #[test]
    fn test_path_to_uri() {
        assert_eq!(
            LspDocumentTracker::path_to_uri("/home/user/test.rs"),
            "file:///home/user/test.rs"
        );
    }

    #[test]
    fn test_uri_to_path() {
        assert_eq!(
            LspDocumentTracker::uri_to_path("file:///home/user/test.rs"),
            "/home/user/test.rs"
        );
    }

    #[test]
    fn test_language_id() {
        assert_eq!(LspDocumentTracker::extension_to_language_id("test.rs"), "rust");
        assert_eq!(LspDocumentTracker::extension_to_language_id("test.ts"), "typescript");
        assert_eq!(LspDocumentTracker::extension_to_language_id("test.tsx"), "typescriptreact");
        assert_eq!(LspDocumentTracker::extension_to_language_id("test.py"), "python");
        assert_eq!(LspDocumentTracker::extension_to_language_id("test.go"), "go");
        assert_eq!(LspDocumentTracker::extension_to_language_id("test.unknown"), "plaintext");
    }
}
