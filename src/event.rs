use crossterm::event::Event as CrosstermEvent;

#[derive(Debug)]
pub enum Event {
    Input(CrosstermEvent),
    Pty(PtyEvent),
    Tick,
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug)]
pub enum PtyEvent {
    Output(Vec<u8>),
    Exited(i32),
    Error(std::io::Error),
}
