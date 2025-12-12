mod action;
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

use color_eyre::eyre::Result;
use cli::Cli;

fn main() -> Result<()> {
    color_eyre::install()?;

    // Parse CLI arguments
    let cli = Cli::parse_args();

    // Create app with CLI options
    let mut app = app::App::with_cli(&cli)?;
    
    // TRC-005: spawn_pty now spawns PTY for the main tab
    app.spawn_pty()?;
    
    // TRC-012: Restore session if enabled
    if cli.restore_session {
        let _ = app.restore_session();
    }
    
    app.run()?;

    Ok(())
}
