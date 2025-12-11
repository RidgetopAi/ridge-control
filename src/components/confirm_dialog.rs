use crossterm::event::{Event, KeyCode};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::action::Action;
use crate::config::Theme;
use crate::llm::{PendingToolUse, ToolExecutionCheck};

/// Confirmation dialog for tool execution
pub struct ConfirmDialog {
    pending_tool: Option<PendingToolUse>,
}

impl ConfirmDialog {
    pub fn new() -> Self {
        Self { pending_tool: None }
    }
    
    pub fn show(&mut self, pending: PendingToolUse) {
        self.pending_tool = Some(pending);
    }
    
    pub fn dismiss(&mut self) {
        self.pending_tool = None;
    }
    
    pub fn is_visible(&self) -> bool {
        self.pending_tool.is_some()
    }
    
    pub fn pending_tool(&self) -> Option<&PendingToolUse> {
        self.pending_tool.as_ref()
    }
    
    pub fn handle_event(&mut self, event: &Event) -> Option<Action> {
        if !self.is_visible() {
            return None;
        }
        
        if let Event::Key(key) = event {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    return Some(Action::ToolConfirm);
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    return Some(Action::ToolReject);
                }
                _ => {}
            }
        }
        
        None
    }
    
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let pending = match &self.pending_tool {
            Some(p) => p,
            None => return,
        };
        
        // Calculate dialog size (centered, 60% width, adaptive height)
        let dialog_width = (area.width * 60 / 100).max(40).min(80);
        let dialog_height = 12;
        
        let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;
        
        let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);
        
        // Clear the area behind the dialog
        frame.render_widget(Clear, dialog_area);
        
        // Determine colors based on check result
        let (border_color, title_prefix) = match pending.check {
            ToolExecutionCheck::RequiresConfirmation => (theme.colors.warning.to_color(), "⚠ CONFIRM"),
            ToolExecutionCheck::RequiresDangerousMode => (theme.colors.error.to_color(), "🚫 BLOCKED"),
            ToolExecutionCheck::PathNotAllowed => (theme.colors.error.to_color(), "🚫 PATH DENIED"),
            ToolExecutionCheck::UnknownTool => (theme.colors.error.to_color(), "❓ UNKNOWN"),
            ToolExecutionCheck::Allowed => (theme.colors.success.to_color(), "✓ ALLOWED"),
        };
        
        let title = format!("{}: {}", title_prefix, pending.tool_name());
        
        let block = Block::default()
            .title(title)
            .title_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        
        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);
        
        // Layout inner content
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // Tool name
                Constraint::Length(1),  // Spacer
                Constraint::Length(2),  // Parameters
                Constraint::Length(1),  // Spacer
                Constraint::Min(1),     // Instructions
            ])
            .split(inner);
        
        // Tool name
        let tool_line = Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(theme.colors.muted.to_color())),
            Span::styled(pending.tool_name(), Style::default().fg(theme.colors.foreground.to_color()).add_modifier(Modifier::BOLD)),
        ]);
        frame.render_widget(Paragraph::new(tool_line), chunks[0]);
        
        // Parameters summary
        let params_text = format!("Args: {}", pending.input_summary());
        let params_para = Paragraph::new(params_text)
            .style(Style::default().fg(theme.colors.primary.to_color()))
            .wrap(Wrap { trim: true });
        frame.render_widget(params_para, chunks[2]);
        
        // Instructions based on check result
        let instructions = match pending.check {
            ToolExecutionCheck::RequiresConfirmation => {
                vec![
                    Line::from(vec![
                        Span::styled("[Y]", Style::default().fg(theme.colors.success.to_color()).add_modifier(Modifier::BOLD)),
                        Span::raw(" Execute   "),
                        Span::styled("[N/Esc]", Style::default().fg(theme.colors.error.to_color()).add_modifier(Modifier::BOLD)),
                        Span::raw(" Cancel"),
                    ]),
                ]
            }
            ToolExecutionCheck::RequiresDangerousMode => {
                vec![
                    Line::from(Span::styled(
                        "This tool requires dangerous mode to be enabled.",
                        Style::default().fg(theme.colors.error.to_color())
                    )),
                    Line::from(Span::styled(
                        "Press [Esc] to dismiss",
                        Style::default().fg(theme.colors.muted.to_color())
                    )),
                ]
            }
            ToolExecutionCheck::PathNotAllowed => {
                vec![
                    Line::from(Span::styled(
                        "The requested path is not in the allowed list.",
                        Style::default().fg(theme.colors.error.to_color())
                    )),
                    Line::from(Span::styled(
                        "Press [Esc] to dismiss",
                        Style::default().fg(theme.colors.muted.to_color())
                    )),
                ]
            }
            _ => {
                vec![
                    Line::from(Span::styled(
                        "Press [Esc] to dismiss",
                        Style::default().fg(theme.colors.muted.to_color())
                    )),
                ]
            }
        };
        
        let instructions_para = Paragraph::new(instructions)
            .alignment(Alignment::Center);
        frame.render_widget(instructions_para, chunks[4]);
    }
}

impl Default for ConfirmDialog {
    fn default() -> Self {
        Self::new()
    }
}
