// CONTRACT.md 4.7: Helix-style keybindings - fully implemented but config loading not yet wired

#![allow(dead_code)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::action::Action;
use crate::input::mode::InputMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Modifier {
    #[serde(rename = "C")]
    Ctrl,
    #[serde(rename = "A")]
    Alt,
    #[serde(rename = "S")]
    Shift,
    #[serde(rename = "M")]
    Meta,
}

impl Modifier {
    pub fn to_key_modifiers(modifiers: &[Modifier]) -> KeyModifiers {
        let mut result = KeyModifiers::empty();
        for m in modifiers {
            match m {
                Modifier::Ctrl => result |= KeyModifiers::CONTROL,
                Modifier::Alt => result |= KeyModifiers::ALT,
                Modifier::Shift => result |= KeyModifiers::SHIFT,
                Modifier::Meta => result |= KeyModifiers::META,
            }
        }
        result
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub key: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    pub fn new(key: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }
    
    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.key == event.code && self.modifiers == event.modifiers
    }
    
    pub fn from_helix_notation(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        
        let mut modifiers = KeyModifiers::empty();
        let mut remaining = s;
        
        while let Some(rest) = remaining.strip_prefix("C-") {
            modifiers |= KeyModifiers::CONTROL;
            remaining = rest;
        }
        while let Some(rest) = remaining.strip_prefix("A-") {
            modifiers |= KeyModifiers::ALT;
            remaining = rest;
        }
        while let Some(rest) = remaining.strip_prefix("S-") {
            modifiers |= KeyModifiers::SHIFT;
            remaining = rest;
        }
        while let Some(rest) = remaining.strip_prefix("M-") {
            modifiers |= KeyModifiers::META;
            remaining = rest;
        }
        
        let key = Self::parse_key_name(remaining)?;
        
        Some(Self { key, modifiers })
    }
    
    fn parse_key_name(name: &str) -> Option<KeyCode> {
        match name.to_lowercase().as_str() {
            "space" | " " => Some(KeyCode::Char(' ')),
            "tab" => Some(KeyCode::Tab),
            "backtab" => Some(KeyCode::BackTab),
            "enter" | "ret" | "return" => Some(KeyCode::Enter),
            "esc" | "escape" => Some(KeyCode::Esc),
            "backspace" | "bs" => Some(KeyCode::Backspace),
            "del" | "delete" => Some(KeyCode::Delete),
            "ins" | "insert" => Some(KeyCode::Insert),
            "home" => Some(KeyCode::Home),
            "end" => Some(KeyCode::End),
            "pageup" | "pgup" => Some(KeyCode::PageUp),
            "pagedown" | "pgdown" | "pgdn" => Some(KeyCode::PageDown),
            "up" => Some(KeyCode::Up),
            "down" => Some(KeyCode::Down),
            "left" => Some(KeyCode::Left),
            "right" => Some(KeyCode::Right),
            "f1" => Some(KeyCode::F(1)),
            "f2" => Some(KeyCode::F(2)),
            "f3" => Some(KeyCode::F(3)),
            "f4" => Some(KeyCode::F(4)),
            "f5" => Some(KeyCode::F(5)),
            "f6" => Some(KeyCode::F(6)),
            "f7" => Some(KeyCode::F(7)),
            "f8" => Some(KeyCode::F(8)),
            "f9" => Some(KeyCode::F(9)),
            "f10" => Some(KeyCode::F(10)),
            "f11" => Some(KeyCode::F(11)),
            "f12" => Some(KeyCode::F(12)),
            s if s.len() == 1 => {
                let c = s.chars().next()?;
                Some(KeyCode::Char(c))
            }
            _ => None,
        }
    }
    
    pub fn to_helix_notation(&self) -> String {
        let mut result = String::new();
        
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            result.push_str("C-");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            result.push_str("A-");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            result.push_str("S-");
        }
        if self.modifiers.contains(KeyModifiers::META) {
            result.push_str("M-");
        }
        
        let key_name = match self.key {
            KeyCode::Char(' ') => "space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Tab => "tab".to_string(),
            KeyCode::BackTab => "backtab".to_string(),
            KeyCode::Enter => "ret".to_string(),
            KeyCode::Esc => "esc".to_string(),
            KeyCode::Backspace => "backspace".to_string(),
            KeyCode::Delete => "del".to_string(),
            KeyCode::Insert => "ins".to_string(),
            KeyCode::Home => "home".to_string(),
            KeyCode::End => "end".to_string(),
            KeyCode::PageUp => "pageup".to_string(),
            KeyCode::PageDown => "pagedown".to_string(),
            KeyCode::Up => "up".to_string(),
            KeyCode::Down => "down".to_string(),
            KeyCode::Left => "left".to_string(),
            KeyCode::Right => "right".to_string(),
            KeyCode::F(n) => format!("f{}", n),
            _ => "?".to_string(),
        };
        
        result.push_str(&key_name);
        result
    }
}

impl FromStr for KeyBinding {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_helix_notation(s).ok_or_else(|| format!("Invalid key binding: {}", s))
    }
}

impl<'de> Deserialize<'de> for KeyBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_helix_notation(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid key binding: {}", s)))
    }
}

impl Serialize for KeyBinding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_helix_notation())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActionBinding {
    pub action: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ModeBindings {
    #[serde(default)]
    pub bindings: HashMap<String, ActionBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub normal: ModeBindings,
    pub pty_raw: ModeBindings,
    pub insert: ModeBindings,
    pub command_palette: ModeBindings,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        let mut normal = ModeBindings::default();
        
        normal.bindings.insert(
            "q".to_string(),
            ActionBinding { action: "quit".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-c".to_string(),
            ActionBinding { action: "quit".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-q".to_string(),
            ActionBinding { action: "llm_cancel".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "tab".to_string(),
            ActionBinding { action: "focus_next".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "S-backtab".to_string(),
            ActionBinding { action: "focus_prev".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            ":".to_string(),
            ActionBinding { action: "open_command_palette".to_string(), args: vec![] },
        );
        // Ctrl+P also opens command palette (VS Code style, in addition to vim-style :)
        normal.bindings.insert(
            "C-p".to_string(),
            ActionBinding { action: "open_command_palette".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "ret".to_string(),
            ActionBinding { action: "enter_pty_mode".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "k".to_string(),
            ActionBinding { action: "scroll_up".to_string(), args: vec!["1".to_string()] },
        );
        normal.bindings.insert(
            "j".to_string(),
            ActionBinding { action: "scroll_down".to_string(), args: vec!["1".to_string()] },
        );
        normal.bindings.insert(
            "up".to_string(),
            ActionBinding { action: "scroll_up".to_string(), args: vec!["1".to_string()] },
        );
        normal.bindings.insert(
            "down".to_string(),
            ActionBinding { action: "scroll_down".to_string(), args: vec!["1".to_string()] },
        );
        normal.bindings.insert(
            "C-u".to_string(),
            ActionBinding { action: "scroll_page_up".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-d".to_string(),
            ActionBinding { action: "scroll_page_down".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "pageup".to_string(),
            ActionBinding { action: "scroll_page_up".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "pagedown".to_string(),
            ActionBinding { action: "scroll_page_down".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "g".to_string(),
            ActionBinding { action: "scroll_to_top".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "G".to_string(),
            ActionBinding { action: "scroll_to_bottom".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "y".to_string(),
            ActionBinding { action: "copy".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "p".to_string(),
            ActionBinding { action: "paste".to_string(), args: vec![] },
        );
        // Tab management
        normal.bindings.insert(
            "C-t".to_string(),
            ActionBinding { action: "tab_create".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-w".to_string(),
            ActionBinding { action: "tab_close".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "]".to_string(),
            ActionBinding { action: "tab_next".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "[".to_string(),
            ActionBinding { action: "tab_prev".to_string(), args: vec![] },
        );
        // TRC-029: Inline tab rename with Ctrl+R
        normal.bindings.insert(
            "C-r".to_string(),
            ActionBinding { action: "tab_start_rename".to_string(), args: vec![] },
        );
        // Conversation viewer
        normal.bindings.insert(
            "C-l".to_string(),
            ActionBinding { action: "conversation_toggle".to_string(), args: vec![] },
        );
        // Notification management (TRC-023)
        normal.bindings.insert(
            "n".to_string(),
            ActionBinding { action: "notify_dismiss".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "N".to_string(),
            ActionBinding { action: "notify_dismiss_all".to_string(), args: vec![] },
        );
        // Pane resizing (TRC-024)
        normal.bindings.insert(
            "C-right".to_string(),
            ActionBinding { action: "pane_resize_main_grow".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-left".to_string(),
            ActionBinding { action: "pane_resize_main_shrink".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-up".to_string(),
            ActionBinding { action: "pane_resize_right_grow".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-down".to_string(),
            ActionBinding { action: "pane_resize_right_shrink".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "A-up".to_string(),
            ActionBinding { action: "pane_resize_left_grow".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "A-down".to_string(),
            ActionBinding { action: "pane_resize_left_shrink".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "C-0".to_string(),
            ActionBinding { action: "pane_reset_layout".to_string(), args: vec![] },
        );
        // SIRK Panel and Activity Stream shortcuts
        normal.bindings.insert(
            "A-s".to_string(),
            ActionBinding { action: "sirk_panel_toggle".to_string(), args: vec![] },
        );
        normal.bindings.insert(
            "A-a".to_string(),
            ActionBinding { action: "activity_stream_toggle".to_string(), args: vec![] },
        );

        let mut pty_raw = ModeBindings::default();
        pty_raw.bindings.insert(
            "C-esc".to_string(),
            ActionBinding { action: "enter_normal_mode".to_string(), args: vec![] },
        );
        pty_raw.bindings.insert(
            "C-q".to_string(),
            ActionBinding { action: "llm_cancel".to_string(), args: vec![] },
        );
        pty_raw.bindings.insert(
            "C-v".to_string(),
            ActionBinding { action: "paste".to_string(), args: vec![] },
        );
        // Alt+D/U for page scroll in PtyRaw mode (Ctrl+D/U pass through to shell)
        pty_raw.bindings.insert(
            "A-d".to_string(),
            ActionBinding { action: "scroll_page_down".to_string(), args: vec![] },
        );
        pty_raw.bindings.insert(
            "A-u".to_string(),
            ActionBinding { action: "scroll_page_up".to_string(), args: vec![] },
        );
        // SIRK Panel and Activity Stream shortcuts (also in PtyRaw mode)
        pty_raw.bindings.insert(
            "A-s".to_string(),
            ActionBinding { action: "sirk_panel_toggle".to_string(), args: vec![] },
        );
        pty_raw.bindings.insert(
            "A-a".to_string(),
            ActionBinding { action: "activity_stream_toggle".to_string(), args: vec![] },
        );

        let mut command_palette = ModeBindings::default();
        command_palette.bindings.insert(
            "esc".to_string(),
            ActionBinding { action: "close_command_palette".to_string(), args: vec![] },
        );
        command_palette.bindings.insert(
            "ret".to_string(),
            ActionBinding { action: "execute_command".to_string(), args: vec![] },
        );
        command_palette.bindings.insert(
            "C-n".to_string(),
            ActionBinding { action: "select_next".to_string(), args: vec![] },
        );
        command_palette.bindings.insert(
            "C-p".to_string(),
            ActionBinding { action: "select_prev".to_string(), args: vec![] },
        );
        command_palette.bindings.insert(
            "down".to_string(),
            ActionBinding { action: "select_next".to_string(), args: vec![] },
        );
        command_palette.bindings.insert(
            "up".to_string(),
            ActionBinding { action: "select_prev".to_string(), args: vec![] },
        );
        
        Self {
            normal,
            pty_raw,
            insert: ModeBindings::default(),
            command_palette,
        }
    }
}

impl KeybindingsConfig {
    pub fn get_action(&self, mode: &InputMode, key: &KeyEvent) -> Option<Action> {
        let bindings = match mode {
            InputMode::Normal => &self.normal,
            InputMode::PtyRaw => &self.pty_raw,
            InputMode::Insert { .. } => &self.insert,
            InputMode::CommandPalette => &self.command_palette,
            InputMode::ThreadPicker => return None, // ThreadPicker handles its own keys
            InputMode::Confirm { .. } => return None,
        };
        
        for (key_str, action_binding) in &bindings.bindings {
            if let Some(binding) = KeyBinding::from_helix_notation(key_str) {
                if binding.matches(key) {
                    return Self::action_from_string(&action_binding.action, &action_binding.args);
                }
            }
        }
        
        None
    }
    
    fn action_from_string(action: &str, args: &[String]) -> Option<Action> {
        match action {
            "quit" => Some(Action::Quit),
            "force_quit" => Some(Action::ForceQuit),
            "focus_next" => Some(Action::FocusNext),
            "focus_prev" => Some(Action::FocusPrev),
            "enter_pty_mode" => Some(Action::EnterPtyMode),
            "enter_normal_mode" => Some(Action::EnterNormalMode),
            "open_command_palette" => Some(Action::OpenCommandPalette),
            "close_command_palette" => Some(Action::CloseCommandPalette),
            "scroll_up" => {
                let n = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
                Some(Action::ScrollUp(n))
            }
            "scroll_down" => {
                let n = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
                Some(Action::ScrollDown(n))
            }
            "scroll_page_up" => Some(Action::ScrollPageUp),
            "scroll_page_down" => Some(Action::ScrollPageDown),
            "scroll_to_top" => Some(Action::ScrollToTop),
            "scroll_to_bottom" => Some(Action::ScrollToBottom),
            "copy" => Some(Action::Copy),
            "paste" => Some(Action::Paste),
            "tick" => Some(Action::Tick),
            "menu_select_next" => Some(Action::MenuSelectNext),
            "menu_select_prev" => Some(Action::MenuSelectPrev),
            "process_refresh" => Some(Action::ProcessRefresh),
            "process_select_next" => Some(Action::ProcessSelectNext),
            "process_select_prev" => Some(Action::ProcessSelectPrev),
            "stream_refresh" => Some(Action::StreamRefresh),
            "llm_cancel" => Some(Action::LlmCancel),
            "llm_clear_conversation" => Some(Action::LlmClearConversation),
            "tool_toggle_dangerous_mode" => Some(Action::ToolToggleDangerousMode),
            "tab_create" => Some(Action::TabCreate),
            "tab_close" => Some(Action::TabClose),
            "tab_next" => Some(Action::TabNext),
            "tab_prev" => Some(Action::TabPrev),
            "tab_select" => {
                let idx = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                Some(Action::TabSelect(idx))
            }
            "tab_rename" => {
                let name = args.first().cloned().unwrap_or_default();
                Some(Action::TabRename(name))
            }
            // TRC-029: Inline tab rename
            "tab_start_rename" => Some(Action::TabStartRename),
            "tab_cancel_rename" => Some(Action::TabCancelRename),
            "config_reload" => Some(Action::ConfigReload),
            "conversation_toggle" => Some(Action::ConversationToggle),
            "conversation_scroll_up" => {
                let n = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
                Some(Action::ConversationScrollUp(n))
            }
            "conversation_scroll_down" => {
                let n = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
                Some(Action::ConversationScrollDown(n))
            }
            "conversation_scroll_to_top" => Some(Action::ConversationScrollToTop),
            "conversation_scroll_to_bottom" => Some(Action::ConversationScrollToBottom),
            // Notification actions (TRC-023)
            "notify_dismiss" => Some(Action::NotifyDismiss),
            "notify_dismiss_all" => Some(Action::NotifyDismissAll),
            // Pane resize actions (TRC-024)
            "pane_resize_main_grow" => Some(Action::PaneResizeMainGrow),
            "pane_resize_main_shrink" => Some(Action::PaneResizeMainShrink),
            "pane_resize_right_grow" => Some(Action::PaneResizeRightGrow),
            "pane_resize_right_shrink" => Some(Action::PaneResizeRightShrink),
            "pane_resize_left_grow" => Some(Action::PaneResizeLeftGrow),
            "pane_resize_left_shrink" => Some(Action::PaneResizeLeftShrink),
            "pane_reset_layout" => Some(Action::PaneResetLayout),
            // SIRK Panel and Activity Stream actions
            "sirk_panel_toggle" => Some(Action::SirkPanelToggle),
            "sirk_panel_show" => Some(Action::SirkPanelShow),
            "sirk_panel_hide" => Some(Action::SirkPanelHide),
            "activity_stream_toggle" => Some(Action::ActivityStreamToggle),
            "activity_stream_show" => Some(Action::ActivityStreamShow),
            "activity_stream_hide" => Some(Action::ActivityStreamHide),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_keybinding_from_helix_notation() {
        let binding = KeyBinding::from_helix_notation("C-c").unwrap();
        assert_eq!(binding.key, KeyCode::Char('c'));
        assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
        
        let binding = KeyBinding::from_helix_notation("A-S-x").unwrap();
        assert_eq!(binding.key, KeyCode::Char('x'));
        assert!(binding.modifiers.contains(KeyModifiers::ALT));
        assert!(binding.modifiers.contains(KeyModifiers::SHIFT));
        
        let binding = KeyBinding::from_helix_notation("tab").unwrap();
        assert_eq!(binding.key, KeyCode::Tab);
        assert!(binding.modifiers.is_empty());
        
        let binding = KeyBinding::from_helix_notation("C-esc").unwrap();
        assert_eq!(binding.key, KeyCode::Esc);
        assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
    }
    
    #[test]
    fn test_keybinding_roundtrip() {
        let original = KeyBinding::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let notation = original.to_helix_notation();
        let parsed = KeyBinding::from_helix_notation(&notation).unwrap();
        assert_eq!(original, parsed);
    }
    
    #[test]
    fn test_keybindings_config_default() {
        let config = KeybindingsConfig::default();
        assert!(config.normal.bindings.contains_key("q"));
        assert!(config.normal.bindings.contains_key("C-c"));
        assert!(config.pty_raw.bindings.contains_key("C-esc"));
    }
    
    #[test]
    fn test_keybindings_serialization() {
        let config = KeybindingsConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let _parsed: KeybindingsConfig = toml::from_str(&toml_str).unwrap();
    }
    
    #[test]
    fn test_enter_key_maps_to_enter_pty_mode() {
        use crate::input::mode::InputMode;
        
        let config = KeybindingsConfig::default();
        
        // Simulate Enter key press (no modifiers)
        let enter_key = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
        
        // In Normal mode, Enter should map to EnterPtyMode
        let action = config.get_action(&InputMode::Normal, &enter_key);
        assert!(action.is_some(), "Enter key should map to an action in Normal mode");
        
        match action {
            Some(Action::EnterPtyMode) => (),
            other => panic!("Expected EnterPtyMode, got {:?}", other),
        }
    }
}
