// Command palette - some methods reserved for dynamic command registration

use crossterm::event::{Event, KeyCode, KeyModifiers};
use nucleo::{Config, Matcher, Utf32String};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::action::Action;
use crate::config::{SubagentsConfig, Theme};

/// A command that can be executed from the command palette
#[derive(Debug, Clone)]
pub struct Command {
    /// Unique identifier for the command
    pub id: String,
    /// Display name shown in palette
    pub name: String,
    /// Brief description
    pub description: String,
    /// The action to dispatch when selected
    pub action: Action,
}

impl Command {
    pub fn new(id: impl Into<String>, name: impl Into<String>, description: impl Into<String>, action: Action) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            action,
        }
    }
}

/// Registry of all available commands
pub struct CommandRegistry {
    commands: Vec<Command>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Self::default_commands(),
        }
    }

    fn default_commands() -> Vec<Command> {
        vec![
            Command::new("quit", "Quit", "Exit Ridge-Control", Action::Quit),
            Command::new("force_quit", "Force Quit", "Exit immediately without cleanup", Action::ForceQuit),
            Command::new("focus_terminal", "Focus Terminal", "Switch focus to terminal pane", Action::FocusArea(crate::input::focus::FocusArea::Terminal)),
            Command::new("focus_process_monitor", "Focus Process Monitor", "Switch focus to process monitor", Action::FocusArea(crate::input::focus::FocusArea::ProcessMonitor)),
            Command::new("focus_menu", "Focus Menu", "Switch focus to menu pane", Action::FocusArea(crate::input::focus::FocusArea::Menu)),
            Command::new("focus_next", "Focus Next", "Cycle to next pane", Action::FocusNext),
            Command::new("focus_prev", "Focus Previous", "Cycle to previous pane", Action::FocusPrev),
            Command::new("enter_pty_mode", "Enter PTY Mode", "Switch to PTY raw input mode", Action::EnterPtyMode),
            Command::new("enter_normal_mode", "Enter Normal Mode", "Switch to normal navigation mode", Action::EnterNormalMode),
            Command::new("scroll_up", "Scroll Up", "Scroll up one line", Action::ScrollUp(1)),
            Command::new("scroll_down", "Scroll Down", "Scroll down one line", Action::ScrollDown(1)),
            Command::new("scroll_page_up", "Scroll Page Up", "Scroll up one page", Action::ScrollPageUp),
            Command::new("scroll_page_down", "Scroll Page Down", "Scroll down one page", Action::ScrollPageDown),
            Command::new("scroll_top", "Scroll to Top", "Scroll to beginning", Action::ScrollToTop),
            Command::new("scroll_bottom", "Scroll to Bottom", "Scroll to end", Action::ScrollToBottom),
            Command::new("copy", "Copy", "Copy selected text to clipboard", Action::Copy),
            Command::new("paste", "Paste", "Paste from clipboard", Action::Paste),
            Command::new("process_refresh", "Refresh Processes", "Update process list", Action::ProcessRefresh),
            Command::new("process_next", "Process Next", "Select next process", Action::ProcessSelectNext),
            Command::new("process_prev", "Process Previous", "Select previous process", Action::ProcessSelectPrev),
            Command::new("stream_refresh", "Refresh Streams", "Reload stream configuration from streams.toml", Action::StreamRefresh),
            Command::new("stream_viewer_toggle", "Toggle Stream Viewer", "Show/hide stream viewer panel", Action::StreamViewerToggle),
            Command::new("stream_viewer_hide", "Hide Stream Viewer", "Close stream viewer panel (Esc)", Action::StreamViewerHide),
            // TRC-028: Config panel command (always accessible per CONTRACT requirement)
            Command::new("config_panel_toggle", "Settings", "Open settings panel (view config, theme, providers)", Action::ConfigPanelToggle),
            Command::new("config_panel_show", "Show Settings", "Open settings panel", Action::ConfigPanelShow),
            Command::new("config_panel_hide", "Hide Settings", "Close settings panel", Action::ConfigPanelHide),
            Command::new("llm_cancel", "Cancel LLM", "Cancel current LLM request", Action::LlmCancel),
            Command::new("llm_clear", "Clear Conversation", "Clear LLM conversation history", Action::LlmClearConversation),
            Command::new("conversation_toggle", "Toggle Conversation View", "Show/hide LLM conversation panel (Ctrl+L)", Action::ConversationToggle),
            Command::new("toggle_dangerous_mode", "Toggle Dangerous Mode", "Enable/disable dangerous tool execution", Action::ToolToggleDangerousMode),
            // Settings Editor commands (TS-014)
            Command::new("settings_editor_toggle", "Edit Settings", "Open settings editor (API keys, provider, model)", Action::SettingsToggle),
            Command::new("settings_editor_show", "Open Settings Editor", "Open the full settings editor panel", Action::SettingsShow),
            Command::new("settings_editor_close", "Close Settings Editor", "Close the settings editor panel", Action::SettingsClose),
            Command::new("settings_save", "Save Settings", "Save current settings to disk (Ctrl+S in editor)", Action::SettingsSave),
            // Thread management commands (Phase 2 - AgentEngine)
            Command::new("thread_new", "New Thread", "Start a new conversation thread", Action::ThreadNew),
            Command::new("thread_save", "Save Thread", "Save current thread to disk", Action::ThreadSave),
            Command::new("thread_clear", "Clear Thread", "Clear current thread (start fresh)", Action::ThreadClear),
            Command::new("thread_continue", "Continue Thread", "Resume a saved conversation thread", Action::ThreadPickerShow),
            Command::new("thread_rename", "Rename Thread", "Rename the current conversation thread", Action::ThreadStartRename),
            // Tab commands
            Command::new("tab_new", "New Tab", "Create a new tab (Ctrl+T)", Action::TabCreate),
            Command::new("tab_close", "Close Tab", "Close current tab (Ctrl+W)", Action::TabClose),
            Command::new("tab_next", "Next Tab", "Switch to next tab (])", Action::TabNext),
            Command::new("tab_prev", "Previous Tab", "Switch to previous tab ([)", Action::TabPrev),
            Command::new("tab_1", "Tab 1", "Switch to tab 1 (F1)", Action::TabSelect(0)),
            Command::new("tab_2", "Tab 2", "Switch to tab 2 (F2)", Action::TabSelect(1)),
            Command::new("tab_3", "Tab 3", "Switch to tab 3 (F3)", Action::TabSelect(2)),
            Command::new("tab_4", "Tab 4", "Switch to tab 4 (F4)", Action::TabSelect(3)),
            Command::new("tab_5", "Tab 5", "Switch to tab 5 (F5)", Action::TabSelect(4)),
        ]
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    #[allow(dead_code)]
    pub fn add_command(&mut self, cmd: Command) {
        self.commands.push(cmd);
    }

    /// Remove all commands with IDs starting with the given prefix
    pub fn remove_commands_with_prefix(&mut self, prefix: &str) {
        self.commands.retain(|c| !c.id.starts_with(prefix));
    }

    /// Set available providers (removes old provider commands and adds new ones)
    pub fn set_providers(&mut self, providers: &[&str], current_provider: &str) {
        self.remove_commands_with_prefix("provider:");
        
        for provider in providers {
            let is_current = *provider == current_provider;
            let name = if is_current {
                format!("Provider: {} ✓", provider)
            } else {
                format!("Provider: {}", provider)
            };
            let description = format!("Switch to {} provider", provider);
            
            self.commands.push(Command::new(
                format!("provider:{}", provider),
                name,
                description,
                Action::LlmSelectProvider(provider.to_string()),
            ));
        }
    }

    /// Set available models for current provider (removes old model commands and adds new ones)
    pub fn set_models(&mut self, models: &[&str], current_model: &str) {
        self.remove_commands_with_prefix("model:");

        for model in models {
            let is_current = *model == current_model;
            let name = if is_current {
                format!("Model: {} ✓", model)
            } else {
                format!("Model: {}", model)
            };
            let description = format!("Switch to {} model", model);

            self.commands.push(Command::new(
                format!("model:{}", model),
                name,
                description,
                Action::LlmSelectModel(model.to_string()),
            ));
        }
    }

    /// Set available models for each subagent type (T2.1b)
    ///
    /// # Arguments
    /// * `subagent_config` - Current subagent configuration
    /// * `available_models` - Map of provider name to available model names
    pub fn set_subagent_models(
        &mut self,
        subagent_config: &SubagentsConfig,
        available_models: &std::collections::HashMap<String, Vec<String>>,
    ) {
        self.remove_commands_with_prefix("subagent:");

        for (agent_type, config) in subagent_config.iter() {
            // Get models available for this agent's provider
            if let Some(models) = available_models.get(&config.provider) {
                for model in models {
                    let is_current = *model == config.model;
                    let name = if is_current {
                        format!("Subagent {}: {} ✓", agent_type, model)
                    } else {
                        format!("Subagent {}: {}", agent_type, model)
                    };

                    self.commands.push(Command::new(
                        format!("subagent:{}:{}", agent_type, model),
                        name,
                        format!("Use {} for {} subagent", model, agent_type),
                        Action::SubagentSelectModel {
                            agent_type: agent_type.to_string(),
                            model: model.to_string(),
                        },
                    ));
                }
            }
        }
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Fuzzy matcher result with score and indices
struct MatchResult {
    command_idx: usize,
    score: u32,
    indices: Vec<u32>,
}

/// Command Palette component with nucleo fuzzy matching
pub struct CommandPalette {
    visible: bool,
    query: String,
    registry: CommandRegistry,
    matcher: Matcher,
    filtered_results: Vec<MatchResult>,
    list_state: ListState,
}

impl CommandPalette {
    pub fn new() -> Self {
        let config = Config::DEFAULT;
        Self {
            visible: false,
            query: String::new(),
            registry: CommandRegistry::new(),
            matcher: Matcher::new(config),
            filtered_results: Vec::new(),
            list_state: ListState::default(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.query.clear();
        self.update_filtered_results();
        // Select first item
        if !self.filtered_results.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    /// Set available providers in the command palette
    pub fn set_providers(&mut self, providers: &[&str], current_provider: &str) {
        self.registry.set_providers(providers, current_provider);
    }

    /// Set available models in the command palette
    pub fn set_models(&mut self, models: &[&str], current_model: &str) {
        self.registry.set_models(models, current_model);
    }

    /// Set available models for subagents in the command palette
    pub fn set_subagent_models(
        &mut self,
        subagent_config: &SubagentsConfig,
        available_models: &std::collections::HashMap<String, Vec<String>>,
    ) {
        self.registry.set_subagent_models(subagent_config, available_models);
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.query.clear();
        self.filtered_results.clear();
        self.list_state.select(None);
    }

    #[allow(dead_code)]
    pub fn query(&self) -> &str {
        &self.query
    }

    fn update_filtered_results(&mut self) {
        self.filtered_results.clear();

        if self.query.is_empty() {
            // Show all commands when no query
            for (idx, _) in self.registry.commands().iter().enumerate() {
                self.filtered_results.push(MatchResult {
                    command_idx: idx,
                    score: 0,
                    indices: Vec::new(),
                });
            }
        } else {
            // Fuzzy match against query
            let pattern = nucleo::pattern::Pattern::parse(
                &self.query,
                nucleo::pattern::CaseMatching::Smart,
                nucleo::pattern::Normalization::Smart,
            );

            for (idx, cmd) in self.registry.commands().iter().enumerate() {
                // Match against both name and description
                let name_utf32: Utf32String = cmd.name.as_str().into();
                let desc_utf32: Utf32String = cmd.description.as_str().into();

                let mut indices = Vec::new();
                let name_score = pattern.indices(
                    name_utf32.slice(..),
                    &mut self.matcher,
                    &mut indices,
                );

                // Also check description if name didn't match well
                let desc_score = if name_score.is_none() {
                    let mut desc_indices = Vec::new();
                    pattern.indices(
                        desc_utf32.slice(..),
                        &mut self.matcher,
                        &mut desc_indices,
                    )
                } else {
                    None
                };

                // Use the best score
                let final_score = name_score.or(desc_score);
                if let Some(score) = final_score {
                    self.filtered_results.push(MatchResult {
                        command_idx: idx,
                        score,
                        indices,
                    });
                }
            }

            // Sort by score (higher is better)
            self.filtered_results.sort_by(|a, b| b.score.cmp(&a.score));
        }

        // Reset selection to first item
        if !self.filtered_results.is_empty() {
            self.list_state.select(Some(0));
        } else {
            self.list_state.select(None);
        }
    }

    fn select_next(&mut self) {
        if self.filtered_results.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let next = (current + 1) % self.filtered_results.len();
        self.list_state.select(Some(next));
    }

    fn select_prev(&mut self) {
        if self.filtered_results.is_empty() {
            return;
        }
        let current = self.list_state.selected().unwrap_or(0);
        let prev = if current == 0 {
            self.filtered_results.len() - 1
        } else {
            current - 1
        };
        self.list_state.select(Some(prev));
    }

    fn execute_selected(&mut self) -> Option<Action> {
        let selected_idx = self.list_state.selected()?;
        let result = self.filtered_results.get(selected_idx)?;
        let cmd = self.registry.commands().get(result.command_idx)?;
        let action = cmd.action.clone();
        self.hide();
        Some(action)
    }

    pub fn handle_event(&mut self, event: &Event) -> Option<Action> {
        if !self.visible {
            return None;
        }

        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Esc => {
                    self.hide();
                    return Some(Action::EnterNormalMode);
                }
                KeyCode::Enter => {
                    return self.execute_selected();
                }
                KeyCode::Up | KeyCode::BackTab => {
                    self.select_prev();
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.select_next();
                }
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.select_next();
                }
                KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.select_prev();
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.update_filtered_results();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.update_filtered_results();
                }
                _ => {}
            }
        }

        None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        if !self.visible {
            return;
        }

        // Calculate dialog size (centered, 60% width, 50% height)
        let dialog_width = (area.width * 60 / 100).clamp(50, 100);
        let dialog_height = (area.height * 50 / 100).clamp(10, 30);

        let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

        // Clear the area behind
        frame.render_widget(Clear, dialog_area);

        // Main block
        let block = Block::default()
            .title(" Command Palette ")
            .title_style(Style::default().fg(theme.command_palette.border.to_color()).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.command_palette.border.to_color()));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        // Split inner area: input line at top, results below
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Input
                Constraint::Length(1), // Separator/info
                Constraint::Min(1),    // Results
            ])
            .split(inner);

        // Input line with prompt
        let input_line = Line::from(vec![
            Span::styled(": ", Style::default().fg(theme.colors.primary.to_color()).add_modifier(Modifier::BOLD)),
            Span::styled(&self.query, Style::default().fg(theme.command_palette.input_fg.to_color())),
            Span::styled("▎", Style::default().fg(theme.colors.primary.to_color())), // Cursor
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[0]);

        // Info line
        let count = self.filtered_results.len();
        let total = self.registry.commands().len();
        let info = if self.query.is_empty() {
            format!("{} commands", total)
        } else {
            format!("{}/{} matching", count, total)
        };
        let info_line = Paragraph::new(info)
            .style(Style::default().fg(theme.command_palette.description_fg.to_color()))
            .alignment(Alignment::Right);
        frame.render_widget(info_line, chunks[1]);

        // Results list
        let items: Vec<ListItem> = self
            .filtered_results
            .iter()
            .map(|result| {
                let cmd = &self.registry.commands()[result.command_idx];
                self.render_command_item(cmd, &result.indices, theme)
            })
            .collect();

        let list = List::new(items)
            .highlight_style(
                Style::default()
                    .bg(theme.command_palette.selected_bg.to_color())
                    .fg(theme.command_palette.selected_fg.to_color())
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        // Clone list_state for rendering (ratatui requires &mut for StatefulWidget)
        let mut list_state = self.list_state.clone();
        frame.render_stateful_widget(list, chunks[2], &mut list_state);
    }

    fn render_command_item(&self, cmd: &Command, indices: &[u32], theme: &Theme) -> ListItem<'_> {
        let mut name_spans = Vec::new();

        // Highlight matched characters in name
        if indices.is_empty() {
            name_spans.push(Span::styled(
                cmd.name.clone(),
                Style::default().fg(theme.command_palette.item_fg.to_color()),
            ));
        } else {
            let chars: Vec<char> = cmd.name.chars().collect();
            let indices_set: std::collections::HashSet<u32> = indices.iter().copied().collect();

            for (i, ch) in chars.iter().enumerate() {
                let style = if indices_set.contains(&(i as u32)) {
                    Style::default().fg(theme.command_palette.match_highlight.to_color()).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.command_palette.item_fg.to_color())
                };
                name_spans.push(Span::styled(ch.to_string(), style));
            }
        }

        // Add description
        name_spans.push(Span::styled(
            format!("  {}", cmd.description),
            Style::default().fg(theme.command_palette.description_fg.to_color()),
        ));

        ListItem::new(Line::from(name_spans))
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_registry_has_default_commands() {
        let registry = CommandRegistry::new();
        assert!(!registry.commands().is_empty());
        
        // Check that quit command exists
        let quit = registry.commands().iter().find(|c| c.id == "quit");
        assert!(quit.is_some());
    }

    #[test]
    fn test_command_palette_visibility() {
        let mut palette = CommandPalette::new();
        assert!(!palette.is_visible());
        
        palette.show();
        assert!(palette.is_visible());
        
        palette.hide();
        assert!(!palette.is_visible());
    }

    #[test]
    fn test_fuzzy_filtering() {
        let mut palette = CommandPalette::new();
        palette.show();
        
        // Initially shows all commands
        let initial_count = palette.filtered_results.len();
        assert!(initial_count > 0);
        
        // Filter with "quit" should narrow results
        palette.query = "quit".to_string();
        palette.update_filtered_results();
        
        // Should have fewer or equal results
        assert!(palette.filtered_results.len() <= initial_count);
        // Should still have quit-related commands
        assert!(!palette.filtered_results.is_empty());
    }

    #[test]
    fn test_selection_navigation() {
        let mut palette = CommandPalette::new();
        palette.show();
        
        assert_eq!(palette.list_state.selected(), Some(0));
        
        palette.select_next();
        assert_eq!(palette.list_state.selected(), Some(1));
        
        palette.select_prev();
        assert_eq!(palette.list_state.selected(), Some(0));
        
        // Test wrap around
        palette.select_prev();
        assert_eq!(palette.list_state.selected(), Some(palette.filtered_results.len() - 1));
    }
}
