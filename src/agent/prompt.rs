//! System prompt builder with platform and repository awareness

use std::process::Command;

/// Platform information for system prompt
#[derive(Debug, Clone, Default)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub shell: String,
    pub cwd: String,
    pub home: String,
    pub date: String,
}

impl PlatformInfo {
    /// Gather current platform information
    pub fn gather() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
            cwd: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".to_string()),
            home: dirs::home_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            date: chrono::Local::now().format("%Y-%m-%d").to_string(),
        }
    }
}

/// Repository context information
#[derive(Debug, Clone, Default)]
pub struct RepoContextInfo {
    pub root: Option<String>,
    pub branch: Option<String>,
    pub has_uncommitted: bool,
    pub language: Option<String>,
}

impl RepoContextInfo {
    /// Gather git repository information from current directory
    pub fn gather() -> Self {
        let mut info = Self::default();

        // Get git root
        if let Ok(output) = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
        {
            if output.status.success() {
                info.root = Some(
                    String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .to_string(),
                );
            }
        }

        // Get current branch
        if let Ok(output) = Command::new("git")
            .args(["branch", "--show-current"])
            .output()
        {
            if output.status.success() {
                let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !branch.is_empty() {
                    info.branch = Some(branch);
                }
            }
        }

        // Check for uncommitted changes
        if let Ok(output) = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
        {
            if output.status.success() {
                info.has_uncommitted = !output.stdout.is_empty();
            }
        }

        // Detect primary language from common files
        info.language = Self::detect_language();

        info
    }

    fn detect_language() -> Option<String> {
        let indicators = [
            ("Cargo.toml", "Rust"),
            ("package.json", "JavaScript/TypeScript"),
            ("go.mod", "Go"),
            ("pyproject.toml", "Python"),
            ("setup.py", "Python"),
            ("Gemfile", "Ruby"),
            ("pom.xml", "Java"),
            ("build.gradle", "Java/Kotlin"),
            ("mix.exs", "Elixir"),
        ];

        for (file, lang) in indicators {
            if std::path::Path::new(file).exists() {
                return Some(lang.to_string());
            }
        }

        None
    }
}

/// Builder for constructing system prompts
#[derive(Debug, Clone)]
pub struct SystemPromptBuilder {
    role: String,
    platform: Option<PlatformInfo>,
    repo: Option<RepoContextInfo>,
    tool_instructions: Vec<String>,
    user_preferences: Vec<String>,
    coding_guidelines: Vec<String>,
}

impl SystemPromptBuilder {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            platform: None,
            repo: None,
            tool_instructions: Vec::new(),
            user_preferences: Vec::new(),
            coding_guidelines: Vec::new(),
        }
    }

    /// Default role for ridge-control agent
    pub fn ridge_control() -> Self {
        Self::new(
            "You are Ridge Control, an expert AI coding assistant running in a terminal interface. \
             You help users with software development tasks including writing code, debugging, \
             explaining concepts, and using system tools. You are concise, accurate, and proactive."
        )
    }

    pub fn with_platform(mut self, platform: PlatformInfo) -> Self {
        self.platform = Some(platform);
        self
    }

    pub fn with_repo(mut self, repo: RepoContextInfo) -> Self {
        self.repo = Some(repo);
        self
    }

    pub fn add_tool_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.tool_instructions.push(instruction.into());
        self
    }

    pub fn add_preference(mut self, pref: impl Into<String>) -> Self {
        self.user_preferences.push(pref.into());
        self
    }

    pub fn add_coding_guideline(mut self, guideline: impl Into<String>) -> Self {
        self.coding_guidelines.push(guideline.into());
        self
    }

    /// Build the full system prompt
    pub fn build(&self) -> String {
        let mut parts = Vec::new();

        // Role
        parts.push(self.role.clone());

        // Platform context
        if let Some(ref platform) = self.platform {
            parts.push(format!(
                "\n## Environment\n- OS: {} ({})\n- Shell: {}\n- Working directory: {}\n- Date: {}",
                platform.os, platform.arch, platform.shell, platform.cwd, platform.date
            ));
        }

        // Repository context
        if let Some(ref repo) = self.repo {
            let mut repo_lines = Vec::new();
            if let Some(ref root) = repo.root {
                repo_lines.push(format!("- Repository root: {}", root));
            }
            if let Some(ref branch) = repo.branch {
                repo_lines.push(format!("- Branch: {}", branch));
            }
            if let Some(ref lang) = repo.language {
                repo_lines.push(format!("- Primary language: {}", lang));
            }
            if repo.has_uncommitted {
                repo_lines.push("- Has uncommitted changes".to_string());
            }
            if !repo_lines.is_empty() {
                parts.push(format!("\n## Repository\n{}", repo_lines.join("\n")));
            }
        }

        // Tool instructions
        if !self.tool_instructions.is_empty() {
            parts.push(format!(
                "\n## Tool Usage\n{}",
                self.tool_instructions
                    .iter()
                    .map(|t| format!("- {}", t))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        // Coding guidelines
        if !self.coding_guidelines.is_empty() {
            parts.push(format!(
                "\n## Coding Guidelines\n{}",
                self.coding_guidelines
                    .iter()
                    .map(|g| format!("- {}", g))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        // User preferences
        if !self.user_preferences.is_empty() {
            parts.push(format!(
                "\n## Preferences\n{}",
                self.user_preferences
                    .iter()
                    .map(|p| format!("- {}", p))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }

        parts.join("\n")
    }

    /// Build a short version for when context budget is tight
    pub fn build_short(&self) -> String {
        let mut parts = Vec::new();
        parts.push(self.role.clone());

        if let Some(ref platform) = self.platform {
            parts.push(format!(
                "Environment: {} {}, cwd: {}",
                platform.os, platform.arch, platform.cwd
            ));
        }

        parts.join(" ")
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::ridge_control()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_info() {
        let info = PlatformInfo::gather();
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
    }

    #[test]
    fn test_prompt_builder() {
        let prompt = SystemPromptBuilder::ridge_control()
            .with_platform(PlatformInfo {
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                shell: "/bin/bash".to_string(),
                cwd: "/home/user/project".to_string(),
                home: "/home/user".to_string(),
                date: "2025-01-01".to_string(),
            })
            .add_tool_instruction("Use bash_run for shell commands")
            .add_coding_guideline("Follow Rust conventions")
            .build();

        assert!(prompt.contains("Ridge Control"));
        assert!(prompt.contains("linux"));
        assert!(prompt.contains("bash_run"));
        assert!(prompt.contains("Rust conventions"));
    }

    #[test]
    fn test_short_prompt() {
        let prompt = SystemPromptBuilder::ridge_control()
            .with_platform(PlatformInfo::gather())
            .build_short();

        // Short prompt should be much smaller
        assert!(prompt.len() < 500);
    }
}
