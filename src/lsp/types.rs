//! LSP type definitions
//!
//! Core types used in Language Server Protocol communication.
//!
//! Note: These types are used indirectly through LSP tools in ToolExecutor
//! (lsp_definition, lsp_references, lsp_hover, lsp_symbols). The dead_code
//! warnings are false positives.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// A position in a text document (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// Line number (0-indexed)
    pub line: u32,
    /// Character offset in the line (0-indexed)
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }

    /// Convert from 1-indexed (user-facing) to 0-indexed (LSP)
    pub fn from_one_indexed(line: u32, character: u32) -> Self {
        Self {
            line: line.saturating_sub(1),
            character: character.saturating_sub(1),
        }
    }

    /// Convert to 1-indexed (user-facing) from 0-indexed (LSP)
    pub fn to_one_indexed(&self) -> (u32, u32) {
        (self.line + 1, self.character + 1)
    }
}

/// A range in a text document
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Start position (inclusive)
    pub start: Position,
    /// End position (exclusive)
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// A location in a document (URI + range)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// Document URI (file:// scheme)
    pub uri: String,
    /// Range within the document
    pub range: Range,
}

impl Location {
    /// Get the file path from the URI
    pub fn file_path(&self) -> &str {
        self.uri.strip_prefix("file://").unwrap_or(&self.uri)
    }

    /// Convert to user-facing format with 1-indexed positions
    pub fn to_display(&self) -> LocationDisplay {
        let (start_line, start_char) = self.range.start.to_one_indexed();
        let (end_line, end_char) = self.range.end.to_one_indexed();
        LocationDisplay {
            file: self.file_path().to_string(),
            line: start_line,
            character: start_char,
            end_line,
            end_character: end_char,
        }
    }
}

/// User-facing location format (1-indexed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDisplay {
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// Text document identifier
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

impl TextDocumentIdentifier {
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }

    pub fn from_path(path: &str) -> Self {
        Self {
            uri: format!("file://{}", path),
        }
    }
}

/// Text document with position
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentPositionParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

/// Symbol kinds in LSP (integers per LSP spec)
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde_repr::Serialize_repr, serde_repr::Deserialize_repr)]
#[repr(u8)]
pub enum SymbolKind {
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,
    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,
    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,
    String = 15,
    Number = 16,
    Boolean = 17,
    Array = 18,
    Object = 19,
    Key = 20,
    Null = 21,
    EnumMember = 22,
    Struct = 23,
    Event = 24,
    Operator = 25,
    TypeParameter = 26,
}

impl SymbolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SymbolKind::File => "file",
            SymbolKind::Module => "module",
            SymbolKind::Namespace => "namespace",
            SymbolKind::Package => "package",
            SymbolKind::Class => "class",
            SymbolKind::Method => "method",
            SymbolKind::Property => "property",
            SymbolKind::Field => "field",
            SymbolKind::Constructor => "constructor",
            SymbolKind::Enum => "enum",
            SymbolKind::Interface => "interface",
            SymbolKind::Function => "function",
            SymbolKind::Variable => "variable",
            SymbolKind::Constant => "constant",
            SymbolKind::String => "string",
            SymbolKind::Number => "number",
            SymbolKind::Boolean => "boolean",
            SymbolKind::Array => "array",
            SymbolKind::Object => "object",
            SymbolKind::Key => "key",
            SymbolKind::Null => "null",
            SymbolKind::EnumMember => "enum_member",
            SymbolKind::Struct => "struct",
            SymbolKind::Event => "event",
            SymbolKind::Operator => "operator",
            SymbolKind::TypeParameter => "type_parameter",
        }
    }
}

/// Symbol information from document/workspace symbols
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInformation {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_name: Option<String>,
}

/// Document symbol (hierarchical)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DocumentSymbol>,
}

/// Hover information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hover {
    pub contents: HoverContents,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}

/// Hover content variants
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContents {
    Scalar(String),
    MarkedString(MarkedString),
    Array(Vec<MarkedString>),
    Markup(MarkupContent),
}

impl HoverContents {
    /// Extract text content from hover
    pub fn to_text(&self) -> String {
        match self {
            HoverContents::Scalar(s) => s.clone(),
            HoverContents::MarkedString(m) => m.value.clone(),
            HoverContents::Array(arr) => arr
                .iter()
                .map(|m| m.value.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
            HoverContents::Markup(m) => m.value.clone(),
        }
    }
}

/// Marked string (language + code)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkedString {
    pub language: String,
    pub value: String,
}

/// Markup content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkupContent {
    pub kind: String,
    pub value: String,
}

/// Call hierarchy item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyItem {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: String,
    pub range: Range,
    pub selection_range: Range,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Incoming call
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyIncomingCall {
    pub from: CallHierarchyItem,
    pub from_ranges: Vec<Range>,
}

/// Outgoing call
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallHierarchyOutgoingCall {
    pub to: CallHierarchyItem,
    pub from_ranges: Vec<Range>,
}

/// Server capabilities (subset we care about)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_symbol_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_symbol_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementation_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_hierarchy_provider: Option<bool>,
}

/// Initialize result from server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

/// Server info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

// ========== Progress Notification Types ==========

/// Progress token - can be string or number
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProgressToken {
    String(String),
    Number(i64),
}

impl std::fmt::Display for ProgressToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProgressToken::String(s) => write!(f, "{}", s),
            ProgressToken::Number(n) => write!(f, "{}", n),
        }
    }
}

/// Progress notification params from $/progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressParams {
    /// The progress token
    pub token: ProgressToken,
    /// The progress value
    pub value: ProgressValue,
}

/// Progress value - different kinds for begin/report/end
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProgressValue {
    Begin {
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cancellable: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        percentage: Option<u32>,
    },
    Report {
        #[serde(skip_serializing_if = "Option::is_none")]
        cancellable: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        percentage: Option<u32>,
    },
    End {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

impl ProgressValue {
    /// Get percentage if available
    pub fn percentage(&self) -> Option<u32> {
        match self {
            ProgressValue::Begin { percentage, .. } => *percentage,
            ProgressValue::Report { percentage, .. } => *percentage,
            ProgressValue::End { .. } => Some(100),
        }
    }

    /// Get message if available
    pub fn message(&self) -> Option<&str> {
        match self {
            ProgressValue::Begin { message, .. } => message.as_deref(),
            ProgressValue::Report { message, .. } => message.as_deref(),
            ProgressValue::End { message, .. } => message.as_deref(),
        }
    }

    /// Get title (only for Begin)
    pub fn title(&self) -> Option<&str> {
        match self {
            ProgressValue::Begin { title, .. } => Some(title),
            _ => None,
        }
    }

    /// Check if this is the end of progress
    pub fn is_end(&self) -> bool {
        matches!(self, ProgressValue::End { .. })
    }
}

/// Indexing state for a language server
#[derive(Debug, Clone, Default)]
pub struct IndexingState {
    /// Whether the server is currently indexing
    pub is_indexing: bool,
    /// Current progress percentage (0-100)
    pub percentage: Option<u32>,
    /// Current status message
    pub message: Option<String>,
    /// Title of current operation
    pub title: Option<String>,
    /// Active progress tokens being tracked (for concurrent operations)
    pub active_tokens: std::collections::HashSet<String>,
}

impl IndexingState {
    /// Create a new indexing state (server just started, assume indexing)
    pub fn new_starting() -> Self {
        Self {
            is_indexing: true,
            percentage: Some(0),
            message: Some("Starting...".to_string()),
            title: Some("Initializing".to_string()),
            active_tokens: std::collections::HashSet::new(),
        }
    }

    /// Create a completed state
    pub fn completed() -> Self {
        Self {
            is_indexing: false,
            percentage: Some(100),
            message: None,
            title: None,
            active_tokens: std::collections::HashSet::new(),
        }
    }

    /// Start tracking a progress token
    pub fn begin_token(&mut self, token: String) {
        self.active_tokens.insert(token);
        self.is_indexing = true;
    }

    /// Stop tracking a progress token, returns true if no more active tokens
    pub fn end_token(&mut self, token: &str) -> bool {
        self.active_tokens.remove(token);
        if self.active_tokens.is_empty() {
            self.is_indexing = false;
            true
        } else {
            false
        }
    }

    /// Check if any tokens are still active
    pub fn has_active_tokens(&self) -> bool {
        !self.active_tokens.is_empty()
    }

    /// Format as a user-friendly status string
    pub fn to_status_string(&self) -> String {
        if !self.is_indexing {
            return "Ready".to_string();
        }

        let mut parts = Vec::new();

        if let Some(title) = &self.title {
            parts.push(title.clone());
        } else if !self.active_tokens.is_empty() {
            // Show first active token as title
            if let Some(token) = self.active_tokens.iter().next() {
                parts.push(token.clone());
            } else {
                parts.push("Working".to_string());
            }
        } else {
            parts.push("Indexing".to_string());
        }

        if let Some(pct) = self.percentage {
            parts.push(format!("{}%", pct));
        }

        if let Some(msg) = &self.message {
            if !msg.is_empty() {
                parts.push(format!("({})", msg));
            }
        }

        // Show how many concurrent operations if more than one
        if self.active_tokens.len() > 1 {
            parts.push(format!("[{} ops]", self.active_tokens.len()));
        }

        parts.join(" ")
    }
}

/// Incoming request from server (has id, expects response)
#[derive(Debug, Clone, Deserialize)]
pub struct ServerRequest {
    pub id: serde_json::Value, // Can be number or string
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

/// Incoming notification from server (no id)
#[derive(Debug, Clone, Deserialize)]
pub struct IncomingNotification {
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_indexing() {
        let pos = Position::from_one_indexed(10, 5);
        assert_eq!(pos.line, 9);
        assert_eq!(pos.character, 4);

        let (line, char) = pos.to_one_indexed();
        assert_eq!(line, 10);
        assert_eq!(char, 5);
    }

    #[test]
    fn test_location_file_path() {
        let loc = Location {
            uri: "file:///home/user/test.rs".to_string(),
            range: Range {
                start: Position::new(0, 0),
                end: Position::new(0, 10),
            },
        };
        assert_eq!(loc.file_path(), "/home/user/test.rs");
    }

    #[test]
    fn test_symbol_kind_str() {
        assert_eq!(SymbolKind::Function.as_str(), "function");
        assert_eq!(SymbolKind::Class.as_str(), "class");
        assert_eq!(SymbolKind::Struct.as_str(), "struct");
    }
}
