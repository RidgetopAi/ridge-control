mod action;
mod app;
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

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut app = app::App::new()?;
    // TRC-005: spawn_pty now spawns PTY for the main tab
    app.spawn_pty()?;
    app.run()?;

    Ok(())
}
