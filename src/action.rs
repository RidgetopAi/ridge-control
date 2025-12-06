use crate::input::focus::FocusArea;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    ForceQuit,
    Tick,
    Render,

    EnterPtyMode,
    EnterNormalMode,

    FocusNext,
    FocusPrev,
    FocusArea(FocusArea),

    PtyInput(Vec<u8>),
    PtyOutput(Vec<u8>),
    PtyResize { cols: u16, rows: u16 },
    PtyExited(i32),

    ScrollUp(u16),
    ScrollDown(u16),
    ScrollPageUp,
    ScrollPageDown,
    ScrollToTop,
    ScrollToBottom,

    None,
}
