use std::path::PathBuf;

use crate::config::KeyId;
use crate::input::focus::FocusArea;
use crate::llm::{LLMError, StreamChunk, PendingToolUse, ToolResult, ToolUse};

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
    OpenCommandPalette,
    CloseCommandPalette,

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
    /// Notify that menu selection changed to specific index
    MenuSelected(usize),

    // Stream actions
    StreamConnect(usize),
    StreamDisconnect(usize),
    StreamToggle(usize),
    StreamRefresh,
    StreamData(String, String),
    /// Show stream viewer with selected stream index
    StreamViewerShow(usize),
    /// Hide stream viewer
    StreamViewerHide,
    /// Toggle stream viewer visibility
    StreamViewerToggle,
    /// Scroll stream viewer
    StreamViewerScrollUp(u16),
    StreamViewerScrollDown(u16),
    StreamViewerScrollToTop,
    StreamViewerScrollToBottom,

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
    
    // Tool execution actions
    /// LLM requested a tool use, needs confirmation
    ToolUseReceived(PendingToolUse),
    /// User confirmed tool execution
    ToolConfirm,
    /// User rejected tool execution  
    ToolReject,
    /// Tool execution completed
    ToolResult(ToolResult),
    /// Toggle dangerous mode for tool execution
    ToolToggleDangerousMode,
    /// Set dangerous mode explicitly (used by CLI flag --dangerously-allow-all)
    ToolSetDangerousMode(bool),

    // Config actions
    /// Configuration file changed (hot-reload trigger)
    ConfigChanged(PathBuf),
    /// Reload all configuration files
    ConfigReload,
    /// Apply theme changes
    ConfigApplyTheme,

    // Key storage actions
    /// Store an API key securely
    KeyStore(KeyId, String),
    /// Request to retrieve an API key
    KeyGet(KeyId),
    /// Delete an API key
    KeyDelete(KeyId),
    /// List all stored keys
    KeyList,
    /// Unlock encrypted keystore with master password
    KeyUnlock(String),
    /// Initialize encrypted keystore with master password  
    KeyInit(String),

    // Conversation viewer actions
    /// Toggle conversation viewer visibility
    ConversationToggle,
    /// Scroll conversation viewer
    ConversationScrollUp(u16),
    ConversationScrollDown(u16),
    ConversationScrollToTop,
    ConversationScrollToBottom,

    // Tab actions
    /// Create a new tab
    TabCreate,
    /// Close the active tab (if not main)
    TabClose,
    /// Close a specific tab by index
    TabCloseIndex(usize),
    /// Switch to next tab
    TabNext,
    /// Switch to previous tab
    TabPrev,
    /// Switch to tab by index (0-based)
    TabSelect(usize),
    /// Rename the active tab
    TabRename(String),
    /// Move tab from one position to another
    TabMove { from: usize, to: usize },

    // Session persistence actions (TRC-012)
    /// Save current session (tabs, layout) to disk
    SessionSave,
    /// Load session from disk and restore tabs
    SessionLoad,
    /// Clear saved session file
    SessionClear,

    // Log viewer actions (TRC-013)
    /// Show log viewer
    LogViewerShow,
    /// Hide log viewer
    LogViewerHide,
    /// Toggle log viewer visibility
    LogViewerToggle,
    /// Scroll log viewer up
    LogViewerScrollUp(u16),
    /// Scroll log viewer down
    LogViewerScrollDown(u16),
    /// Scroll log viewer to top
    LogViewerScrollToTop,
    /// Scroll log viewer to bottom
    LogViewerScrollToBottom,
    /// Scroll log viewer page up
    LogViewerScrollPageUp,
    /// Scroll log viewer page down
    LogViewerScrollPageDown,
    /// Toggle auto-scroll for log viewer
    LogViewerToggleAutoScroll,
    /// Clear all log entries
    LogViewerClear,
    /// Add log entry (target, message)
    LogViewerPush(String, String),

    // Config panel actions (TRC-014)
    /// Show config panel
    ConfigPanelShow,
    /// Hide config panel
    ConfigPanelHide,
    /// Toggle config panel visibility
    ConfigPanelToggle,
    /// Scroll config panel up
    ConfigPanelScrollUp(u16),
    /// Scroll config panel down
    ConfigPanelScrollDown(u16),
    /// Scroll config panel to top
    ConfigPanelScrollToTop,
    /// Scroll config panel to bottom
    ConfigPanelScrollToBottom,
    /// Scroll config panel page up
    ConfigPanelScrollPageUp,
    /// Scroll config panel page down
    ConfigPanelScrollPageDown,
    /// Navigate to next section in config panel
    ConfigPanelNextSection,
    /// Navigate to previous section in config panel
    ConfigPanelPrevSection,
    /// Toggle section expand/collapse
    ConfigPanelToggleSection,

    // Spinner/Animation actions (TRC-015)
    /// Advance all active spinners one frame (called on tick)
    SpinnerTick,
    /// Start a named spinner with optional label
    SpinnerStart(String, Option<String>),
    /// Stop a named spinner
    SpinnerStop(String),
    /// Set spinner label
    SpinnerSetLabel(String, Option<String>),

    // Tool Call UI actions (TRC-016)
    /// Navigate to next tool call in conversation
    ToolCallNextTool,
    /// Navigate to previous tool call in conversation
    ToolCallPrevTool,
    /// Toggle expand/collapse of selected tool call
    ToolCallToggleExpand,
    /// Expand all tool calls in conversation
    ToolCallExpandAll,
    /// Collapse all tool calls in conversation
    ToolCallCollapseAll,
    /// Start execution of a tool by ID
    ToolCallStartExecution(String),
    /// Register a tool use from LLM in the conversation viewer
    ToolCallRegister(ToolUse),

    // Thinking block actions (TRC-017)
    /// Toggle collapse/expand of all thinking blocks
    ThinkingToggleCollapse,

    // Context menu actions (TRC-020)
    /// Show context menu at position for target
    ContextMenuShow { x: u16, y: u16, target: ContextMenuTarget },
    /// Close context menu
    ContextMenuClose,
    /// Navigate to next context menu item
    ContextMenuNext,
    /// Navigate to previous context menu item
    ContextMenuPrev,
    /// Select/activate current context menu item
    ContextMenuSelect,

    // Search actions (TRC-021)
    /// Start search in log viewer
    LogViewerSearchStart,
    /// Close search in log viewer
    LogViewerSearchClose,
    /// Navigate to next search match
    LogViewerSearchNext,
    /// Navigate to previous search match
    LogViewerSearchPrev,
    /// Update search query in log viewer
    LogViewerSearchQuery(String),
    /// Toggle case sensitivity in log viewer search
    LogViewerSearchToggleCase,

    /// Start search in stream viewer
    StreamViewerSearchStart,
    /// Close search in stream viewer
    StreamViewerSearchClose,
    /// Navigate to next search match
    StreamViewerSearchNext,
    /// Navigate to previous search match
    StreamViewerSearchPrev,
    /// Update search query in stream viewer
    StreamViewerSearchQuery(String),
    /// Toggle case sensitivity in stream viewer search
    StreamViewerSearchToggleCase,

    /// Start search in conversation viewer
    ConversationSearchStart,
    /// Close search in conversation viewer
    ConversationSearchClose,
    /// Navigate to next search match
    ConversationSearchNext,
    /// Navigate to previous search match
    ConversationSearchPrev,
    /// Update search query in conversation viewer
    ConversationSearchQuery(String),
    /// Toggle case sensitivity in conversation viewer search
    ConversationSearchToggleCase,

    // Filter/Grep actions (TRC-022)
    /// Start filter mode in log viewer (shows filter bar)
    LogViewerFilterStart,
    /// Close filter mode in log viewer (clears filter)
    LogViewerFilterClose,
    /// Apply current filter (hide filter bar but keep filter active)
    LogViewerFilterApply,
    /// Update filter pattern in log viewer
    LogViewerFilterPattern(String),
    /// Toggle case sensitivity in log viewer filter
    LogViewerFilterToggleCase,
    /// Toggle regex mode in log viewer filter
    LogViewerFilterToggleRegex,
    /// Toggle inverted (exclude) mode in log viewer filter
    LogViewerFilterToggleInvert,
    /// Clear filter in log viewer
    LogViewerFilterClear,

    /// Start filter mode in stream viewer
    StreamViewerFilterStart,
    /// Close filter mode in stream viewer
    StreamViewerFilterClose,
    /// Apply current filter in stream viewer
    StreamViewerFilterApply,
    /// Update filter pattern in stream viewer
    StreamViewerFilterPattern(String),
    /// Toggle case sensitivity in stream viewer filter
    StreamViewerFilterToggleCase,
    /// Toggle regex mode in stream viewer filter
    StreamViewerFilterToggleRegex,
    /// Toggle inverted mode in stream viewer filter
    StreamViewerFilterToggleInvert,
    /// Clear filter in stream viewer
    StreamViewerFilterClear,

    // Notification actions (TRC-023)
    /// Show an info notification
    NotifyInfo(String),
    /// Show an info notification with message body
    NotifyInfoMessage(String, String),
    /// Show a success notification
    NotifySuccess(String),
    /// Show a success notification with message body
    NotifySuccessMessage(String, String),
    /// Show a warning notification
    NotifyWarning(String),
    /// Show a warning notification with message body
    NotifyWarningMessage(String, String),
    /// Show an error notification
    NotifyError(String),
    /// Show an error notification with message body
    NotifyErrorMessage(String, String),
    /// Dismiss the first (oldest) notification
    NotifyDismiss,
    /// Dismiss all notifications
    NotifyDismissAll,

    None,
}

/// Target type for context menus (TRC-020)
#[derive(Debug, Clone)]
pub enum ContextMenuTarget {
    /// Right-clicked on a tab (includes tab index)
    Tab(usize),
    /// Right-clicked on a process (includes PID)
    Process(i32),
    /// Right-clicked on a stream (includes stream index)
    Stream(usize),
    /// Right-clicked on terminal area
    Terminal,
    /// Right-clicked on log viewer
    LogViewer,
    /// Right-clicked on conversation viewer
    Conversation,
    /// Generic/no specific target
    Generic,
}
