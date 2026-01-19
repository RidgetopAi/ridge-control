use std::path::PathBuf;

use crate::config::KeyId;
use crate::input::focus::FocusArea;
use crate::llm::{LLMError, StreamChunk, PendingToolUse, ToolResult, ToolUse};

/// A question for the ask_user tool
#[derive(Debug, Clone)]
pub struct AskUserQuestion {
    /// Short header/label for the question (max 12 chars)
    pub header: String,
    /// The full question text
    pub question: String,
    /// Available options to choose from
    pub options: Vec<AskUserOption>,
    /// Allow multiple selections
    pub multi_select: bool,
}

/// An option for an ask_user question
#[derive(Debug, Clone)]
pub struct AskUserOption {
    /// Display label (1-5 words)
    pub label: String,
    /// Description of what this option means
    pub description: String,
}

/// Request to show ask_user dialog
#[derive(Debug, Clone)]
pub struct AskUserRequest {
    /// Tool use ID to respond to
    pub tool_use_id: String,
    /// Questions to ask (1-4)
    pub questions: Vec<AskUserQuestion>,
}

/// Response from ask_user dialog
#[derive(Debug, Clone)]
pub struct AskUserResponse {
    /// Tool use ID this responds to
    pub tool_use_id: String,
    /// Map of question index to selected option(s) or custom text
    pub answers: Vec<String>,
}

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

// Many Action variants have handlers but no triggers yet - this is intentional
// scaffolding per CONTRACT.md for features like LLM streaming, key management,
// session persistence, and config hot-reload.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Action {
    /// No-op action - used to consume input without triggering any behavior
    Noop,
    Quit,
    ForceQuit,
    Tick,

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
    /// Retry connection for a failed stream (resets health state)
    StreamRetry(usize),
    /// Cancel ongoing reconnection attempts
    StreamCancelReconnect(usize),
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

    // Activity Stream actions (SIRK/Forge)
    /// Show activity stream panel
    ActivityStreamShow,
    /// Hide activity stream panel
    ActivityStreamHide,
    /// Toggle activity stream visibility
    ActivityStreamToggle,
    /// Clear all activities from the stream
    ActivityStreamClear,
    /// Toggle auto-scroll mode
    ActivityStreamToggleAutoScroll,

    // SIRK Panel actions (Forge control)
    /// Show SIRK panel
    SirkPanelShow,
    /// Hide SIRK panel
    SirkPanelHide,
    /// Toggle SIRK panel visibility
    SirkPanelToggle,
    /// Start a Forge run
    SirkStart,
    /// Stop the current Forge run
    SirkStop,
    /// Resume a paused Forge run (with resume=true flag)
    SirkResume,
    /// Confirm resume when prompted by Forge (sends resume response)
    SirkResumeConfirm,
    /// Abort resume when prompted by Forge (sends abort response)
    SirkResumeAbort,
    /// Reset SIRK panel to idle state for a new run
    SirkReset,

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

    // Chat input actions
    /// Clear the chat input buffer
    ChatInputClear,
    /// Paste text into chat input
    ChatInputPaste(String),
    /// Copy selected text from chat input to clipboard
    ChatInputCopy,
    /// Scroll chat input up by n lines
    ChatInputScrollUp(u16),
    /// Scroll chat input down by n lines
    ChatInputScrollDown(u16),

    // Subagent configuration actions (T2.1b)
    /// Select model for a specific subagent type (explore, plan, review)
    SubagentSelectModel { agent_type: String, model: String },
    /// Select provider for a specific subagent type
    SubagentSelectProvider { agent_type: String, provider: String },

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

    // Thread management actions (Phase 2)
    /// Create a new conversation thread
    ThreadNew,
    /// Load an existing thread by ID
    ThreadLoad(String),
    /// List all available threads
    ThreadList,
    /// Save current thread to disk
    ThreadSave,
    /// Clear current thread (start fresh without deleting)
    ThreadClear,

    // Thread picker actions (P2-003)
    /// Show thread picker dialog for selecting a thread to resume
    ThreadPickerShow,
    /// Hide thread picker dialog
    ThreadPickerHide,

    // Thread rename actions
    /// Start rename mode for current thread
    ThreadStartRename,
    /// Cancel thread rename (revert)
    ThreadCancelRename,
    /// Input character for thread rename
    ThreadRenameInput(char),
    /// Delete character from thread rename buffer
    ThreadRenameBackspace,
    /// Confirm thread rename with new name
    ThreadRename(String),

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
    /// Copy selected text from conversation viewer
    ConversationCopy,

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
    /// TRC-029: Start inline rename mode for active tab
    TabStartRename,
    /// TRC-029: Cancel inline rename mode (revert to original name)
    TabCancelRename,
    /// TRC-029: Update rename buffer (while typing)
    TabRenameInput(char),
    /// TRC-029: Delete character from rename buffer
    TabRenameBackspace,

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
    
    // Tool result actions
    /// Toggle collapse/expand of tool results (press 'R')
    ToolResultToggleCollapse,
    /// Phase 4: Cycle through tool verbosity levels (Compact -> Normal -> Verbose)
    ToolVerbosityCycle,

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

    // Pane resize actions (TRC-024)
    /// Resize the main (left/right) split - grow left pane
    PaneResizeMainGrow,
    /// Resize the main (left/right) split - shrink left pane
    PaneResizeMainShrink,
    /// Resize the right (top/bottom) split - grow top pane
    PaneResizeRightGrow,
    /// Resize the right (top/bottom) split - shrink top pane
    PaneResizeRightShrink,
    /// Resize the left (terminal/conversation) split - grow top pane
    PaneResizeLeftGrow,
    /// Resize the left (terminal/conversation) split - shrink top pane
    PaneResizeLeftShrink,
    /// Reset all panes to default sizes
    PaneResetLayout,
    /// Start mouse drag on a border
    PaneStartDrag(PaneBorder),
    /// Continue mouse drag
    PaneDrag { x: u16, y: u16 },
    /// End mouse drag
    PaneEndDrag,

    // Settings Editor actions (TS-003+)
    /// Show settings editor
    SettingsShow,
    /// Hide settings editor
    SettingsClose,
    /// Toggle settings editor visibility
    SettingsToggle,
    /// Navigate to next section
    SettingsNextSection,
    /// Navigate to previous section
    SettingsPrevSection,
    /// Navigate to next item within section
    SettingsNextItem,
    /// Navigate to previous item within section
    SettingsPrevItem,
    /// Scroll settings up
    SettingsScrollUp(u16),
    /// Scroll settings down
    SettingsScrollDown(u16),
    /// Start editing a field (API key entry)
    SettingsStartEdit,
    /// Cancel current edit
    SettingsCancelEdit,
    /// API key entered for a provider
    SettingsKeyEntered { provider: String, key: String },
    /// Provider selection changed
    SettingsProviderChanged(String),
    /// Model selection changed
    SettingsModelChanged(String),
    /// Test current API key (TS-007)
    SettingsTestKey,
    /// Test key result received (TS-007)
    SettingsTestKeyResult { provider: String, success: bool, error: Option<String> },
    /// Temperature changed (TS-010)
    SettingsTemperatureChanged(f32),
    /// Max tokens changed (TS-010)
    SettingsMaxTokensChanged(u32),
    /// Save settings
    SettingsSave,

    // Ask User dialog actions (P2-T2.4)
    /// Show ask_user dialog with questions
    AskUserShow(AskUserRequest),
    /// Navigate to next option in ask_user dialog
    AskUserNextOption,
    /// Navigate to previous option in ask_user dialog
    AskUserPrevOption,
    /// Navigate to next question in ask_user dialog
    AskUserNextQuestion,
    /// Navigate to previous question in ask_user dialog
    AskUserPrevQuestion,
    /// Toggle option selection (for multi-select)
    AskUserToggleOption,
    /// Select current option and submit (for single-select)
    AskUserSelectOption,
    /// Start entering custom "Other" text
    AskUserStartCustom,
    /// Cancel custom text entry
    AskUserCancelCustom,
    /// Input character for custom text
    AskUserCustomInput(char),
    /// Delete character from custom text
    AskUserCustomBackspace,
    /// Submit custom text
    AskUserSubmitCustom,
    /// Submit all answers
    AskUserSubmit,
    /// Cancel ask_user dialog
    AskUserCancel,
    /// Response from ask_user dialog (internal - sends result back to tool)
    AskUserRespond(AskUserResponse),

    None,
}

/// Border types for pane resizing (TRC-024)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneBorder {
    /// Vertical border between left and right panes
    MainVertical,
    /// Horizontal border in right pane (between process monitor and menu)
    RightHorizontal,
    /// Horizontal border in left pane (between terminal and conversation)
    LeftHorizontal,
}

/// Target type for context menus (TRC-020)
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    /// Right-clicked on chat input
    ChatInput,
    /// Generic/no specific target
    Generic,
}
