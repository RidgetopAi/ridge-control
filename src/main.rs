mod action;
mod app;
mod components;
mod error;
mod event;
mod input;
mod pty;
mod streams;

use color_eyre::eyre::Result;

fn main() -> Result<()> {
    color_eyre::install()?;

    let mut app = app::App::new()?;
    let pty_rx = app.spawn_pty()?;
    app.run(pty_rx)?;

    Ok(())
}
