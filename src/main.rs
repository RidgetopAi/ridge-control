mod action;
mod agent;
mod app;
mod cli;
mod components;
mod config;
mod error;
mod event;
mod input;
mod llm;
mod lsp;
mod pty;
mod sirk;
mod spindles;
mod streams;
mod tabs;
mod util;

use std::io::Write;
use std::panic;
use std::path::PathBuf;

use color_eyre::eyre::Result;
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

use cli::{Cli, Command, KeysAction};
use config::{KeyId, KeyStore, SecretString};

/// Get the log directory path (~/.local/share/ridge-control/logs/)
fn log_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "ridgetopai", "ridge-control")
        .map(|dirs| dirs.data_dir().join("logs"))
        .unwrap_or_else(|| PathBuf::from("/tmp/ridge-control/logs"))
}

/// Initialize the tracing/logging subsystem
/// 
/// Logs to:
/// - File: ~/.local/share/ridge-control/logs/ridge-control.YYYY-MM-DD.log (daily rotation)
/// - Stderr: Only on panic/crash (via color-eyre)
fn init_logging(log_level: &str) -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_path = log_dir();
    
    // Ensure log directory exists
    std::fs::create_dir_all(&log_path)?;
    
    // Create a daily rotating file appender
    let file_appender = RollingFileAppender::new(
        Rotation::DAILY,
        &log_path,
        "ridge-control.log",
    );
    
    // Make file appender non-blocking to avoid I/O stalls
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    
    // Build the filter from CLI log level or RUST_LOG env var
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));
    
    // Set up the subscriber with file output
    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)  // No color codes in log files
                .with_target(true)
                .with_thread_ids(true)
                .with_file(true)
                .with_line_number(true)
        )
        .init();
    
    tracing::info!("Logging initialized. Log directory: {}", log_path.display());

    Ok(guard)
}

/// Handle CLI subcommands (keys, etc.) without launching the TUI
fn handle_command(command: &Command) -> Result<()> {
    match command {
        Command::Keys { action } => handle_keys_command(action),
    }
}

/// Handle keys subcommand
fn handle_keys_command(action: &KeysAction) -> Result<()> {
    let mut keystore = KeyStore::new().map_err(|e| {
        color_eyre::eyre::eyre!("Failed to initialize keystore: {}", e)
    })?;

    match action {
        KeysAction::Set { name, value } => {
            let key_id = KeyId::Custom(name.clone());
            let secret = SecretString::new(value.clone());
            keystore.store(&key_id, &secret).map_err(|e| {
                color_eyre::eyre::eyre!("Failed to store key: {}", e)
            })?;
            println!("✅ Key '{}' stored successfully", name);
        }
        KeysAction::List => {
            let keys = keystore.list().map_err(|e| {
                color_eyre::eyre::eyre!("Failed to list keys: {}", e)
            })?;
            if keys.is_empty() {
                println!("No keys stored");
            } else {
                println!("Stored keys:");
                for key in keys {
                    println!("  • {}", key);
                }
            }
        }
        KeysAction::Delete { name } => {
            let key_id = KeyId::Custom(name.clone());
            keystore.delete(&key_id).map_err(|e| {
                color_eyre::eyre::eyre!("Failed to delete key: {}", e)
            })?;
            println!("✅ Key '{}' deleted", name);
        }
        KeysAction::Get { name, reveal } => {
            let key_id = KeyId::Custom(name.clone());
            match keystore.get(&key_id) {
                Ok(Some(secret)) => {
                    if *reveal {
                        println!("{}", secret.expose());
                    } else {
                        let value = secret.expose();
                        let masked = if value.len() > 8 {
                            format!("{}...{}", &value[..4], &value[value.len()-4..])
                        } else {
                            "****".to_string()
                        };
                        println!("{}: {}", name, masked);
                    }
                }
                Ok(None) => {
                    println!("Key '{}' not found", name);
                }
                Err(e) => {
                    return Err(color_eyre::eyre::eyre!("Failed to get key: {}", e));
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install panic hook FIRST to restore terminal state on panic.
    // This is critical because `panic = "abort"` in release mode means Drop won't run.
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // Restore terminal state before panic output
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        let _ = std::io::stdout().flush();
        // Call the original hook for proper panic reporting
        original_hook(panic_info);
    }));

    color_eyre::install()?;

    // Parse CLI arguments
    let cli = Cli::parse_args();

    // Handle subcommands (these don't need the TUI)
    if let Some(command) = &cli.command {
        return handle_command(command);
    }

    // Initialize logging FIRST (before anything else can log)
    // Keep guard alive for the entire program lifetime
    let _log_guard = init_logging(&cli.log_level)?;

    tracing::info!("Starting ridge-control v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("CLI options: {:?}", cli);

    // Create app with CLI options
    let mut app = match app::App::with_cli(&cli) {
        Ok(app) => app,
        Err(e) => {
            tracing::error!("Failed to create app: {}", e);
            return Err(e.into());
        }
    };
    
    // TRC-005: spawn_pty now spawns PTY for the main tab
    if let Err(e) = app.spawn_pty() {
        tracing::error!("Failed to spawn PTY: {}", e);
        return Err(e.into());
    }
    
    // TRC-012: Restore session if enabled
    if cli.restore_session {
        if let Err(e) = app.restore_session() {
            tracing::warn!("Failed to restore session: {}", e);
        }
    }
    
    tracing::info!("App initialized, entering main loop");
    
    match app.run().await {
        Ok(()) => {
            tracing::info!("App exited normally");
            Ok(())
        }
        Err(e) => {
            tracing::error!("App exited with error: {}", e);
            Err(e.into())
        }
    }
}
