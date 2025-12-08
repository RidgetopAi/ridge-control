use crate::input::focus::FocusArea;
use crate::llm::{LLMError, StreamChunk};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Pid,
    Name,
    Cpu,
    Memory,
    State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

#[derive(Debug, Clone)]
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

    Copy,
    Paste,

    // Menu actions
    MenuSelectNext,
    MenuSelectPrev,

    // Stream actions
    StreamConnect(usize),
    StreamDisconnect(usize),
    StreamToggle(usize),
    StreamRefresh,
    StreamData(String, String),

    // Process Monitor actions
    ProcessRefresh,
    ProcessSelectNext,
    ProcessSelectPrev,
    ProcessKillRequest(i32),
    ProcessKillConfirm(i32),
    ProcessKillCancel,
    ProcessSetFilter(String),
    ProcessClearFilter,
    ProcessSetSort(SortColumn),
    ProcessToggleSortOrder,

    // LLM actions
    LlmSendMessage(String),
    LlmStreamChunk(StreamChunk),
    LlmStreamComplete,
    LlmStreamError(LLMError),
    LlmCancel,
    LlmSelectModel(String),
    LlmSelectProvider(String),
    LlmClearConversation,

    None,
}
