use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SirkSession {
    pub run_name: String,
    pub instance_number: u32,
    pub total_instances: u32,
    pub project: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityMessage {
    Thinking(ThinkingActivity),
    ToolCall(ToolCallActivity),
    ToolResult(ToolResultActivity),
    Text(TextActivity),
    Error(ErrorActivity),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingActivity {
    pub content: String,
    pub timestamp: String,
    pub session: Option<SirkSession>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallActivity {
    pub tool_name: String,
    pub tool_id: String,
    pub input: Value,
    pub timestamp: String,
    pub session: Option<SirkSession>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultActivity {
    pub tool_id: String,
    pub content: Value,
    pub is_error: bool,
    pub timestamp: String,
    pub session: Option<SirkSession>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextActivity {
    pub content: String,
    pub timestamp: String,
    pub session: Option<SirkSession>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorActivity {
    pub message: String,
    pub code: Option<String>,
    pub timestamp: String,
    pub session: Option<SirkSession>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityConnectionAck {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WebSocketMessage {
    Activity(ActivityMessage),
    ConnectionAck(ActivityConnectionAck),
}

impl ActivityMessage {
    pub fn timestamp(&self) -> &str {
        match self {
            ActivityMessage::Thinking(a) => &a.timestamp,
            ActivityMessage::ToolCall(a) => &a.timestamp,
            ActivityMessage::ToolResult(a) => &a.timestamp,
            ActivityMessage::Text(a) => &a.timestamp,
            ActivityMessage::Error(a) => &a.timestamp,
        }
    }

    pub fn session(&self) -> Option<&SirkSession> {
        match self {
            ActivityMessage::Thinking(a) => a.session.as_ref(),
            ActivityMessage::ToolCall(a) => a.session.as_ref(),
            ActivityMessage::ToolResult(a) => a.session.as_ref(),
            ActivityMessage::Text(a) => a.session.as_ref(),
            ActivityMessage::Error(a) => a.session.as_ref(),
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ActivityMessage::Thinking(_) => "💭",
            ActivityMessage::ToolCall(tc) => match tc.tool_name.as_str() {
                "Read" | "file_read" => "📖",
                "Edit" | "edit_file" | "file_edit" => "✏️",
                "Write" | "file_write" => "📝",
                "Bash" | "bash" => "⚡",
                "Grep" | "grep" => "🔍",
                "Glob" | "glob" => "📂",
                "Task" => "🤖",
                "WebFetch" | "WebSearch" => "🌐",
                // Mandrel MCP tools
                name if name.starts_with("mcp__") || name.starts_with("context_")
                    || name.starts_with("project_") || name.starts_with("task_")
                    || name.starts_with("decision_") || name.starts_with("mandrel_") => "🔮",
                _ => "🛠️",
            },
            ActivityMessage::ToolResult(tr) => {
                if tr.is_error {
                    "❌"
                } else {
                    "✅"
                }
            }
            ActivityMessage::Text(_) => "💬",
            ActivityMessage::Error(_) => "❌",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thinking_activity() {
        let json = r#"{"type":"thinking","content":"Let me analyze this...","timestamp":"2026-01-17T12:00:00Z","session":null}"#;
        let msg: ActivityMessage = serde_json::from_str(json).unwrap();
        match msg {
            ActivityMessage::Thinking(a) => {
                assert_eq!(a.content, "Let me analyze this...");
                assert!(a.session.is_none());
            }
            _ => panic!("Expected Thinking"),
        }
    }

    #[test]
    fn test_parse_tool_call_activity() {
        let json = r#"{"type":"tool_call","toolName":"Read","toolId":"tc_123","input":{"path":"/foo/bar.rs"},"timestamp":"2026-01-17T12:00:00Z","session":{"runName":"test","instanceNumber":1,"totalInstances":5,"project":"myproj"}}"#;
        let msg: ActivityMessage = serde_json::from_str(json).unwrap();
        match msg {
            ActivityMessage::ToolCall(a) => {
                assert_eq!(a.tool_name, "Read");
                assert_eq!(a.tool_id, "tc_123");
                assert!(a.session.is_some());
                assert_eq!(a.session.unwrap().run_name, "test");
            }
            _ => panic!("Expected ToolCall"),
        }
    }

    #[test]
    fn test_parse_tool_result_activity() {
        let json = r#"{"type":"tool_result","toolId":"tc_123","content":"file contents here","isError":false,"timestamp":"2026-01-17T12:00:00Z","session":null}"#;
        let msg: ActivityMessage = serde_json::from_str(json).unwrap();
        match msg {
            ActivityMessage::ToolResult(a) => {
                assert_eq!(a.tool_id, "tc_123");
                assert!(!a.is_error);
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_parse_error_activity() {
        let json = r#"{"type":"error","message":"Something went wrong","code":"ERR001","timestamp":"2026-01-17T12:00:00Z","session":null}"#;
        let msg: ActivityMessage = serde_json::from_str(json).unwrap();
        match msg {
            ActivityMessage::Error(a) => {
                assert_eq!(a.message, "Something went wrong");
                assert_eq!(a.code.as_deref(), Some("ERR001"));
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_connection_ack() {
        let json = r#"{"type":"connection_ack","timestamp":"2026-01-17T12:00:00Z"}"#;
        let msg: ActivityConnectionAck = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, "connection_ack");
    }

    #[test]
    fn test_activity_icon() {
        let thinking = ActivityMessage::Thinking(ThinkingActivity {
            content: "test".to_string(),
            timestamp: "".to_string(),
            session: None,
        });
        assert_eq!(thinking.icon(), "💭");
    }
}
