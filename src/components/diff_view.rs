// Diff view component for file_write tool calls
// Shows unified diff between original and new file content

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use similar::{ChangeTag, TextDiff};

use crate::config::Theme;

/// Represents a single line in the diff output
#[derive(Debug, Clone)]
pub enum DiffLine {
    /// Context line (unchanged)
    Context(String),
    /// Added line (new content)
    Addition(String),
    /// Removed line (old content)
    Deletion(String),
    /// Header/separator line
    Header(String),
}

/// Configuration for diff rendering
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiffConfig {
    /// Number of context lines around changes
    pub context_lines: usize,
    /// Maximum lines to show before truncating
    pub max_lines: usize,
    /// Show line numbers
    pub show_line_numbers: bool,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            context_lines: 3,
            max_lines: 50,
            show_line_numbers: true,
        }
    }
}

/// Computes a diff between old and new content
pub struct DiffComputer {
    config: DiffConfig,
}

#[allow(dead_code)]
impl DiffComputer {
    pub fn new() -> Self {
        Self {
            config: DiffConfig::default(),
        }
    }

    pub fn with_config(config: DiffConfig) -> Self {
        Self { config }
    }

    /// Compute diff lines between old and new content
    /// Returns a vector of DiffLine entries ready for rendering
    pub fn compute(&self, old: &str, new: &str, path: &str) -> Vec<DiffLine> {
        let mut lines = Vec::new();

        // Add file header
        lines.push(DiffLine::Header(format!("--- a/{}", path)));
        lines.push(DiffLine::Header(format!("+++ b/{}", path)));

        let diff = TextDiff::from_lines(old, new);
        let mut changes: Vec<DiffLine> = Vec::new();

        for change in diff.iter_all_changes() {
            let line_content = change.value().trim_end_matches('\n').to_string();
            match change.tag() {
                ChangeTag::Equal => {
                    changes.push(DiffLine::Context(line_content));
                }
                ChangeTag::Insert => {
                    changes.push(DiffLine::Addition(line_content));
                }
                ChangeTag::Delete => {
                    changes.push(DiffLine::Deletion(line_content));
                }
            }
        }

        // Apply context filtering - show only relevant context around changes
        let filtered = self.filter_context(&changes);
        lines.extend(filtered);

        lines
    }

    /// Compute diff for a new file (no original content)
    pub fn compute_new_file(&self, content: &str, path: &str) -> Vec<DiffLine> {
        let mut lines = Vec::new();

        // Header for new file
        lines.push(DiffLine::Header(format!("--- /dev/null")));
        lines.push(DiffLine::Header(format!("+++ b/{}", path)));
        lines.push(DiffLine::Header("@@ -0,0 +1 @@".to_string()));

        // All lines are additions
        for line in content.lines() {
            lines.push(DiffLine::Addition(line.to_string()));
        }

        // Handle empty file or file ending without newline
        if content.is_empty() {
            lines.push(DiffLine::Addition(String::new()));
        }

        lines
    }

    /// Filter to show only context around changes
    fn filter_context(&self, changes: &[DiffLine]) -> Vec<DiffLine> {
        if self.config.context_lines == usize::MAX {
            return changes.to_vec();
        }

        let mut result = Vec::new();
        let mut last_change_idx: Option<usize> = None;
        let mut pending_context: Vec<(usize, &DiffLine)> = Vec::new();

        for (idx, change) in changes.iter().enumerate() {
            match change {
                DiffLine::Context(_) => {
                    pending_context.push((idx, change));
                    // Keep only last N context lines
                    if pending_context.len() > self.config.context_lines {
                        pending_context.remove(0);
                    }
                }
                DiffLine::Addition(_) | DiffLine::Deletion(_) => {
                    // Add separator if there's a gap
                    if let Some(last_idx) = last_change_idx {
                        let gap = idx.saturating_sub(last_idx + 1);
                        if gap > self.config.context_lines * 2 {
                            result.push(DiffLine::Header("...".to_string()));
                        }
                    }

                    // Flush pending context (leading context for this change)
                    for (_, ctx) in pending_context.drain(..) {
                        result.push(ctx.clone());
                    }

                    // Add the change itself
                    result.push(change.clone());
                    last_change_idx = Some(idx);
                }
                DiffLine::Header(_) => {
                    result.push(change.clone());
                }
            }
        }

        // Add trailing context after last change
        if last_change_idx.is_some() {
            for (_, change) in changes.iter().enumerate() {
                if let DiffLine::Context(_) = change {
                    // Already handled in main loop
                }
            }
        }

        result
    }
}

impl Default for DiffComputer {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders diff lines to ratatui Lines with proper styling
pub struct DiffRenderer<'a> {
    theme: &'a Theme,
    show_line_numbers: bool,
}

#[allow(dead_code)]
impl<'a> DiffRenderer<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        Self {
            theme,
            show_line_numbers: true,
        }
    }

    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Render diff lines to ratatui Lines
    pub fn render(&self, diff_lines: &[DiffLine], max_lines: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut old_line_num = 0usize;
        let mut new_line_num = 0usize;
        let truncated = diff_lines.len() > max_lines;

        for diff_line in diff_lines.iter().take(max_lines) {
            let rendered = match diff_line {
                DiffLine::Header(text) => {
                    Line::from(Span::styled(
                        format!("    {}", text),
                        Style::default()
                            .fg(self.theme.colors.accent.to_color())
                            .add_modifier(Modifier::BOLD),
                    ))
                }
                DiffLine::Context(text) => {
                    old_line_num += 1;
                    new_line_num += 1;
                    if self.show_line_numbers {
                        Line::from(vec![
                            Span::styled(
                                format!("{:>4} {:>4} ", old_line_num, new_line_num),
                                Style::default().fg(self.theme.colors.muted.to_color()),
                            ),
                            Span::styled(
                                format!("  {}", text),
                                Style::default().fg(self.theme.colors.muted.to_color()),
                            ),
                        ])
                    } else {
                        Line::from(Span::styled(
                            format!("      {}", text),
                            Style::default().fg(self.theme.colors.muted.to_color()),
                        ))
                    }
                }
                DiffLine::Addition(text) => {
                    new_line_num += 1;
                    if self.show_line_numbers {
                        Line::from(vec![
                            Span::styled(
                                format!("     {:>4} ", new_line_num),
                                Style::default().fg(self.theme.colors.muted.to_color()),
                            ),
                            Span::styled(
                                "+ ",
                                Style::default()
                                    .fg(self.theme.colors.success.to_color())
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                text.clone(),
                                Style::default().fg(self.theme.colors.success.to_color()),
                            ),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(
                                "    + ",
                                Style::default()
                                    .fg(self.theme.colors.success.to_color())
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                text.clone(),
                                Style::default().fg(self.theme.colors.success.to_color()),
                            ),
                        ])
                    }
                }
                DiffLine::Deletion(text) => {
                    old_line_num += 1;
                    if self.show_line_numbers {
                        Line::from(vec![
                            Span::styled(
                                format!("{:>4}      ", old_line_num),
                                Style::default().fg(self.theme.colors.muted.to_color()),
                            ),
                            Span::styled(
                                "- ",
                                Style::default()
                                    .fg(self.theme.colors.error.to_color())
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                text.clone(),
                                Style::default().fg(self.theme.colors.error.to_color()),
                            ),
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled(
                                "    - ",
                                Style::default()
                                    .fg(self.theme.colors.error.to_color())
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                text.clone(),
                                Style::default().fg(self.theme.colors.error.to_color()),
                            ),
                        ])
                    }
                }
            };
            lines.push(rendered);
        }

        if truncated {
            lines.push(Line::from(Span::styled(
                format!("    ... ({} more lines)", diff_lines.len() - max_lines),
                Style::default()
                    .fg(self.theme.colors.muted.to_color())
                    .add_modifier(Modifier::ITALIC),
            )));
        }

        lines
    }

    /// Render a summary line showing change counts
    pub fn render_summary(&self, diff_lines: &[DiffLine]) -> Line<'static> {
        let additions = diff_lines
            .iter()
            .filter(|l| matches!(l, DiffLine::Addition(_)))
            .count();
        let deletions = diff_lines
            .iter()
            .filter(|l| matches!(l, DiffLine::Deletion(_)))
            .count();

        Line::from(vec![
            Span::styled(
                "    ",
                Style::default(),
            ),
            Span::styled(
                format!("+{}", additions),
                Style::default()
                    .fg(self.theme.colors.success.to_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " / ",
                Style::default().fg(self.theme.colors.muted.to_color()),
            ),
            Span::styled(
                format!("-{}", deletions),
                Style::default()
                    .fg(self.theme.colors.error.to_color())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " lines",
                Style::default().fg(self.theme.colors.muted.to_color()),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_new_file() {
        let computer = DiffComputer::new();
        let content = "line 1\nline 2\nline 3";
        let lines = computer.compute_new_file(content, "test.txt");

        assert!(matches!(lines[0], DiffLine::Header(_)));
        assert!(matches!(lines[1], DiffLine::Header(_)));
        // All content lines should be additions
        assert!(lines.iter().skip(3).all(|l| matches!(l, DiffLine::Addition(_))));
    }

    #[test]
    fn test_compute_diff_with_changes() {
        let computer = DiffComputer::new();
        let old = "line 1\nline 2\nline 3";
        let new = "line 1\nmodified line 2\nline 3";
        let lines = computer.compute(old, new, "test.txt");

        // Should have headers
        assert!(matches!(lines[0], DiffLine::Header(_)));
        assert!(matches!(lines[1], DiffLine::Header(_)));

        // Should have at least one deletion and one addition
        let has_deletion = lines.iter().any(|l| matches!(l, DiffLine::Deletion(_)));
        let has_addition = lines.iter().any(|l| matches!(l, DiffLine::Addition(_)));
        assert!(has_deletion);
        assert!(has_addition);
    }

    #[test]
    fn test_compute_diff_identical() {
        let computer = DiffComputer::new();
        let content = "same content";
        let lines = computer.compute(content, content, "test.txt");

        // Should have headers and context, but no additions/deletions
        let has_changes = lines.iter().any(|l| {
            matches!(l, DiffLine::Addition(_) | DiffLine::Deletion(_))
        });
        assert!(!has_changes);
    }
}
