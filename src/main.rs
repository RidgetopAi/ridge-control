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
mod pty;
mod streams;
mod tabs;

use std::path::PathBuf;

use color_eyre::eyre::Result;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

use cli::Cli;

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

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    // Parse CLI arguments
    let cli = Cli::parse_args();
    
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
    
    match app.run() {
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
