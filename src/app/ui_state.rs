// UiState - Extracted UI-related state from App struct (Order 8.2)
// Contains focus, modes, dialogs, layout areas, and UI chrome components

use std::time::Instant;

use arboard::Clipboard;
use ratatui::layout::Rect;

use crate::components::ask_user_dialog::AskUserDialog;
use crate::components::command_palette::CommandPalette;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::ContextMenu;
use crate::components::menu::Menu;
use crate::components::notification::NotificationManager;
use crate::components::pane_layout::{DragState, PaneLayout};
use crate::components::spinner_manager::SpinnerManager;
use crate::input::focus::FocusManager;
use crate::input::mode::InputMode;

pub struct UiState {
    // Core UI mode/state
    pub input_mode: InputMode,
    pub focus: FocusManager,
    pub needs_redraw: bool,
    pub last_activity: Instant,
    pub last_esc_press: Option<Instant>,

    // UI chrome/components
    pub menu: Menu,
    pub command_palette: CommandPalette,
    pub confirm_dialog: ConfirmDialog,
    pub context_menu: ContextMenu,
    pub notification_manager: NotificationManager,
    pub spinner_manager: SpinnerManager,
    pub ask_user_dialog: AskUserDialog,
    pub clipboard: Option<Clipboard>,

    // Layout / hit testing areas
    pub tab_bar_area: Rect,
    pub terminal_area: Rect,
    pub conversation_area: Rect,
    pub chat_input_area: Rect,
    pub content_area: Rect,
    pub pane_layout: PaneLayout,
    pub drag_state: DragState,

    // Activity Stream visibility (SIRK/Forge)
    pub activity_stream_visible: bool,
    // SIRK Panel visibility (Forge control)
    pub sirk_panel_visible: bool,
}

impl UiState {
    pub fn new(menu: Menu, clipboard: Option<Clipboard>) -> Self {
        let now = Instant::now();
        Self {
            input_mode: InputMode::Normal,
            focus: FocusManager::new(),
            needs_redraw: true,
            last_activity: now,
            last_esc_press: None,
            menu,
            command_palette: CommandPalette::new(),
            confirm_dialog: ConfirmDialog::new(),
            context_menu: ContextMenu::new(),
            notification_manager: NotificationManager::new(),
            spinner_manager: SpinnerManager::new(),
            ask_user_dialog: AskUserDialog::new(),
            clipboard,
            tab_bar_area: Rect::default(),
            terminal_area: Rect::default(),
            conversation_area: Rect::default(),
            chat_input_area: Rect::default(),
            content_area: Rect::default(),
            pane_layout: PaneLayout::new(),
            drag_state: DragState::default(),
            activity_stream_visible: false,
            sirk_panel_visible: false,
        }
    }

    /// Mark the UI as needing a redraw and record activity for adaptive polling
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.needs_redraw = true;
        self.last_activity = Instant::now();
    }
}
