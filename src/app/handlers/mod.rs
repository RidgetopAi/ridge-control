// Domain-specific action dispatch handlers
// Refactored from monolithic handlers.rs into logical domain groupings (Hardening Order 7)

mod core;
mod terminal_tabs;
mod streams_process;
mod chat_llm;
mod config_settings;
mod ui_chrome;
mod input;

use crate::action::Action;
use crate::error::Result;
use super::App;

impl App {
    /// Main dispatch router - delegates to domain-specific handlers
    /// This is the single exhaustive match over Action for compile-time safety
    pub(super) fn dispatch(&mut self, action: Action) -> Result<()> {
        use Action::*;

        match action {
            // 1. Core app lifecycle, modes, and focus
            Noop | Quit | ForceQuit | Tick
            | EnterPtyMode | EnterNormalMode
            | OpenCommandPalette | CloseCommandPalette
            | FocusNext | FocusPrev | FocusArea(_)
                => self.dispatch_core(action),

            // 2. Terminal, PTY, tabs, and pane layout
            PtyInput(_) | PtyOutput(_)
            | PtyResize { .. }
            | ScrollUp(_) | ScrollDown(_)
            | ScrollPageUp | ScrollPageDown
            | ScrollToTop | ScrollToBottom
            | Copy | Paste
            | TabCreate | TabClose | TabCloseIndex(_)
            | TabNext | TabPrev | TabSelect(_)
            | TabRename(_) | TabMove { .. }
            | TabStartRename | TabCancelRename
            | TabRenameInput(_) | TabRenameBackspace
            | SessionSave | SessionLoad | SessionClear
            | PaneResizeMainGrow | PaneResizeMainShrink
            | PaneResizeRightGrow | PaneResizeRightShrink
            | PaneResizeLeftGrow | PaneResizeLeftShrink
            | PaneResetLayout | PaneStartDrag(_) | PaneDrag { .. } | PaneEndDrag
                => self.dispatch_terminal_tabs(action),

            // 3. Streams, process monitor, menu, log viewer
            MenuSelectNext | MenuSelectPrev | MenuSelected(_)
            | StreamConnect(_) | StreamDisconnect(_) | StreamToggle(_)
            | StreamRefresh | StreamData(_, _)
            | StreamRetry(_) | StreamCancelReconnect(_)
            | StreamViewerShow(_) | StreamViewerHide | StreamViewerToggle
            | StreamViewerScrollUp(_) | StreamViewerScrollDown(_)
            | StreamViewerScrollToTop | StreamViewerScrollToBottom
            | StreamViewerSearchStart | StreamViewerSearchClose
            | StreamViewerSearchNext | StreamViewerSearchPrev
            | StreamViewerSearchQuery(_) | StreamViewerSearchToggleCase
            | StreamViewerFilterStart | StreamViewerFilterClose
            | StreamViewerFilterApply | StreamViewerFilterPattern(_)
            | StreamViewerFilterToggleCase | StreamViewerFilterToggleRegex
            | StreamViewerFilterToggleInvert | StreamViewerFilterClear
            | ProcessRefresh | ProcessSelectNext | ProcessSelectPrev
            | ProcessKillRequest(_) | ProcessKillConfirm(_) | ProcessKillCancel
            | ProcessSetFilter(_) | ProcessClearFilter
            | ProcessSetSort(_) | ProcessToggleSortOrder
            | LogViewerShow | LogViewerHide | LogViewerToggle
            | LogViewerScrollUp(_) | LogViewerScrollDown(_)
            | LogViewerScrollToTop | LogViewerScrollToBottom
            | LogViewerScrollPageUp | LogViewerScrollPageDown
            | LogViewerToggleAutoScroll | LogViewerClear | LogViewerPush(_, _)
            | LogViewerSearchStart | LogViewerSearchClose
            | LogViewerSearchNext | LogViewerSearchPrev
            | LogViewerSearchQuery(_) | LogViewerSearchToggleCase
            | LogViewerFilterStart | LogViewerFilterClose
            | LogViewerFilterApply | LogViewerFilterPattern(_)
            | LogViewerFilterToggleCase | LogViewerFilterToggleRegex
            | LogViewerFilterToggleInvert | LogViewerFilterClear
            | ActivityStreamShow | ActivityStreamHide | ActivityStreamToggle
            | ActivityStreamClear | ActivityStreamToggleAutoScroll
            | SirkPanelShow | SirkPanelHide | SirkPanelToggle
            | SirkStart | SirkStop | SirkResume
            | SirkResumeConfirm | SirkResumeAbort | SirkReset
                => self.dispatch_streams_process(action),

            // 4. Chat, LLM, threads, tools, conversation
            LlmSendMessage(_) | LlmStreamChunk(_)
            | LlmStreamComplete | LlmStreamError(_)
            | LlmCancel
            | LlmSelectModel(_) | LlmSelectProvider(_)
            | LlmClearConversation
            | SubagentSelectModel { .. } | SubagentSelectProvider { .. }
            | ChatInputClear | ChatInputPaste(_) | ChatInputCopy
            | ChatInputScrollUp(_) | ChatInputScrollDown(_)
            | ConversationToggle
            | ConversationScrollUp(_) | ConversationScrollDown(_)
            | ConversationScrollToTop | ConversationScrollToBottom
            | ConversationCopy
            | ConversationSearchStart | ConversationSearchClose
            | ConversationSearchNext | ConversationSearchPrev
            | ConversationSearchQuery(_) | ConversationSearchToggleCase
            | ToolUseReceived(_)
            | ToolConfirm | ToolReject
            | ToolResult(_)
            | ToolToggleDangerousMode | ToolSetDangerousMode(_)
            | ToolCallNextTool | ToolCallPrevTool
            | ToolCallToggleExpand | ToolCallExpandAll | ToolCallCollapseAll
            | ToolCallStartExecution(_) | ToolCallRegister(_)
            | ThinkingToggleCollapse
            | ToolResultToggleCollapse | ToolVerbosityCycle
            | ThreadNew | ThreadLoad(_) | ThreadList
            | ThreadSave | ThreadClear
            | ThreadPickerShow | ThreadPickerHide
            | ThreadStartRename | ThreadCancelRename
            | ThreadRenameInput(_) | ThreadRenameBackspace | ThreadRename(_)
                => self.dispatch_chat_llm(action),

            // 5. Configuration, settings editor, key storage, config panel
            ConfigChanged(_) | ConfigReload | ConfigApplyTheme
            | ConfigPanelShow | ConfigPanelHide | ConfigPanelToggle
            | ConfigPanelScrollUp(_) | ConfigPanelScrollDown(_)
            | ConfigPanelScrollToTop | ConfigPanelScrollToBottom
            | ConfigPanelScrollPageUp | ConfigPanelScrollPageDown
            | ConfigPanelNextSection | ConfigPanelPrevSection | ConfigPanelToggleSection
            | SettingsShow | SettingsClose | SettingsToggle
            | SettingsNextSection | SettingsPrevSection
            | SettingsNextItem | SettingsPrevItem
            | SettingsScrollUp(_) | SettingsScrollDown(_)
            | SettingsStartEdit | SettingsCancelEdit
            | SettingsKeyEntered { .. }
            | SettingsProviderChanged(_) | SettingsModelChanged(_)
            | SettingsTestKey | SettingsTestKeyResult { .. }
            | SettingsTemperatureChanged(_) | SettingsMaxTokensChanged(_)
            | SettingsSave
            | KeyStore(_, _) | KeyGet(_) | KeyDelete(_) | KeyList
            | KeyUnlock(_) | KeyInit(_)
                => self.dispatch_config_settings(action),

            // 6. UI chrome: notifications, context menu, spinners, ask_user dialog
            NotifyInfo(_) | NotifyInfoMessage(_, _)
            | NotifySuccess(_) | NotifySuccessMessage(_, _)
            | NotifyWarning(_) | NotifyWarningMessage(_, _)
            | NotifyError(_) | NotifyErrorMessage(_, _)
            | NotifyDismiss | NotifyDismissAll
            | ContextMenuShow { .. }
            | ContextMenuClose | ContextMenuNext | ContextMenuPrev | ContextMenuSelect
            | SpinnerTick | SpinnerStart(_, _) | SpinnerStop(_) | SpinnerSetLabel(_, _)
            | AskUserShow(_)
            | AskUserNextOption | AskUserPrevOption
            | AskUserNextQuestion | AskUserPrevQuestion
            | AskUserToggleOption | AskUserSelectOption
            | AskUserStartCustom | AskUserCancelCustom
            | AskUserCustomInput(_) | AskUserCustomBackspace
            | AskUserSubmitCustom | AskUserSubmit
            | AskUserCancel | AskUserRespond(_)
                => self.dispatch_ui_chrome(action),

            // Catch-all for Action::None
            None => Ok(()),
        }
    }
}
