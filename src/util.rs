//! Utility functions for text processing

use regex::Regex;
use std::sync::LazyLock;

/// ANSI escape sequence regex pattern
/// Matches CSI sequences (ESC[...m), OSC sequences (ESC]...BEL), and other control codes
static ANSI_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\x1b\[[0-9;?]*[A-Za-z]",     // CSI sequences (colors, cursor movement, etc.)
        r"|\x1b\][^\x07]*\x07",         // OSC sequences (title, etc.)
        r"|\x1b\][^\x1b]*\x1b\\",       // OSC with ST terminator
        r"|\x1b[PX^_][^\x1b]*\x1b\\",   // DCS, SOS, PM, APC sequences
        r"|\x1b.",                       // Other two-byte escape sequences
        r"|[\x00-\x08\x0b\x0c\x0e-\x1f]" // Other control characters (except \n, \r, \t)
    )).unwrap()
});

/// Strip ANSI escape sequences and control characters from text.
/// 
/// This is essential for displaying LLM responses that may contain raw terminal
/// escape codes (e.g., from Grok's ASCII art) in a Ratatui-managed TUI.
pub fn strip_ansi(text: &str) -> String {
    ANSI_REGEX.replace_all(text, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_basic_colors() {
        let input = "\x1b[31mRed Text\x1b[0m";
        assert_eq!(strip_ansi(input), "Red Text");
    }

    #[test]
    fn test_strip_256_colors() {
        let input = "\x1b[38;5;196mBright Red\x1b[0m";
        assert_eq!(strip_ansi(input), "Bright Red");
    }

    #[test]
    fn test_strip_true_color() {
        let input = "\x1b[38;2;255;0;0mTrue Red\x1b[0m";
        assert_eq!(strip_ansi(input), "True Red");
    }

    #[test]
    fn test_strip_cursor_movement() {
        let input = "\x1b[5;10HMoved Cursor";
        assert_eq!(strip_ansi(input), "Moved Cursor");
    }

    #[test]
    fn test_strip_clear_screen() {
        let input = "\x1b[2JCleared Screen";
        assert_eq!(strip_ansi(input), "Cleared Screen");
    }

    #[test]
    fn test_preserves_newlines_and_tabs() {
        let input = "Line1\nLine2\tTabbed";
        assert_eq!(strip_ansi(input), "Line1\nLine2\tTabbed");
    }

    #[test]
    fn test_complex_mixed_content() {
        let input = "\x1b[1;32m╔══════════╗\x1b[0m\n\x1b[34m║  Hello   ║\x1b[0m";
        assert_eq!(strip_ansi(input), "╔══════════╗\n║  Hello   ║");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_no_escapes() {
        let input = "Just plain text";
        assert_eq!(strip_ansi(input), "Just plain text");
    }
}
