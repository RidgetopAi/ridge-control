use clap::Parser;

/// Ridge-Control: Terminal-based command center with PTY emulator, LLM integration, and process monitoring
#[derive(Parser, Debug, Clone)]
#[command(name = "ridge-control")]
#[command(author = "RidgetopAI")]
#[command(version)]
#[command(about = "Terminal-based command center with LLM integration", long_about = None)]
pub struct Cli {
    /// Enable dangerous mode: auto-execute all tool calls without confirmation.
    /// 
    /// WARNING: This bypasses all safety confirmations for LLM tool execution
    /// including file writes, deletes, and bash command execution.
    /// Only use in trusted environments where you accept all risks.
    #[arg(long, default_value_t = false)]
    pub dangerously_allow_all: bool,

    /// Set the working directory for tool execution
    #[arg(short = 'C', long, value_name = "DIR")]
    pub working_dir: Option<std::path::PathBuf>,

    /// API key for Anthropic (Claude). Overrides keystore/config.
    #[arg(long, env = "ANTHROPIC_API_KEY", hide_env_values = true)]
    pub anthropic_api_key: Option<String>,

    /// API key for OpenAI. Overrides keystore/config.
    #[arg(long, env = "OPENAI_API_KEY", hide_env_values = true)]
    pub openai_api_key: Option<String>,

    /// API key for Google Gemini. Overrides keystore/config.
    #[arg(long, env = "GOOGLE_API_KEY", hide_env_values = true)]
    pub gemini_api_key: Option<String>,

    /// API key for xAI (Grok). Overrides keystore/config.
    #[arg(long, env = "XAI_API_KEY", hide_env_values = true)]
    pub grok_api_key: Option<String>,

    /// API key for Groq. Overrides keystore/config.
    #[arg(long, env = "GROQ_API_KEY", hide_env_values = true)]
    pub groq_api_key: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// Restore previous session on startup
    #[arg(long, default_value_t = true)]
    pub restore_session: bool,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_defaults() {
        let cli = Cli::parse_from(["ridge-control"]);
        assert!(!cli.dangerously_allow_all);
        assert!(cli.working_dir.is_none());
        assert!(cli.anthropic_api_key.is_none());
        assert_eq!(cli.log_level, "info");
        assert!(cli.restore_session);
    }

    #[test]
    fn test_dangerously_allow_all_flag() {
        let cli = Cli::parse_from(["ridge-control", "--dangerously-allow-all"]);
        assert!(cli.dangerously_allow_all);
    }

    #[test]
    fn test_working_dir_flag() {
        let cli = Cli::parse_from(["ridge-control", "-C", "/tmp/test"]);
        assert_eq!(cli.working_dir, Some(std::path::PathBuf::from("/tmp/test")));
    }

    #[test]
    fn test_api_key_flags() {
        let cli = Cli::parse_from([
            "ridge-control",
            "--anthropic-api-key", "test-key",
        ]);
        assert_eq!(cli.anthropic_api_key, Some("test-key".to_string()));
    }
}
