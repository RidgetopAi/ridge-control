// Streams, process monitor, menu, and log viewer dispatch
// Domain: Stream connections, menu navigation, process monitor, log viewer with search/filter

use crate::action::Action;
use crate::components::Component;
use crate::error::Result;
use crate::input::focus::FocusArea;
use crate::streams::ConnectionState;

use super::super::App;

impl App {
    pub(super) fn dispatch_streams_process(&mut self, action: Action) -> Result<()> {
        match action {
            // Menu actions
            Action::MenuSelectNext => {
                self.ui.menu.update(&Action::MenuSelectNext);
                // Sync selected_stream_index with menu selection
                let idx = self.ui.menu.selected_index();
                self.selected_stream_index = Some(idx);
            }
            Action::MenuSelectPrev => {
                self.ui.menu.update(&Action::MenuSelectPrev);
                // Sync selected_stream_index with menu selection
                let idx = self.ui.menu.selected_index();
                self.selected_stream_index = Some(idx);
            }
            Action::MenuSelected(idx) => {
                // Direct selection update (e.g., from mouse click)
                self.selected_stream_index = Some(idx);
            }

            // Stream connection actions
            Action::StreamConnect(idx) => {
                if let Some(client) = self.stream_manager.clients().get(idx) {
                    let id = client.id().to_string();
                    self.stream_manager.connect(&id);
                }
            }
            Action::StreamDisconnect(idx) => {
                if let Some(client) = self.stream_manager.clients().get(idx) {
                    let id = client.id().to_string();
                    self.stream_manager.disconnect(&id);
                }
            }
            Action::StreamToggle(idx) => {
                let id_and_state = self.stream_manager.clients().get(idx).map(|c| {
                    (c.id().to_string(), c.state())
                });
                if let Some((id, state)) = id_and_state {
                    match state {
                        ConnectionState::Connected => self.stream_manager.disconnect(&id),
                        _ => self.stream_manager.connect(&id),
                    }
                }
            }
            Action::StreamRefresh => {
                // TRC-028: Use centralized reload method for consistency
                self.reload_streams_from_config();
            }
            Action::StreamData(_, _) => {
                // Stream data is handled elsewhere
            }
            Action::StreamRetry(idx) => {
                // TRC-025: Retry connection for failed stream, resetting health
                let info = self.stream_manager.clients().get(idx)
                    .map(|c| (c.id().to_string(), c.name().to_string()));
                if let Some((id, name)) = info {
                    self.stream_manager.retry(&id);
                    self.ui.notification_manager.info(format!("Retrying {}...", name));
                }
            }
            Action::StreamCancelReconnect(idx) => {
                // TRC-025: Cancel ongoing reconnection
                if let Some(client) = self.stream_manager.clients().get(idx) {
                    let id = client.id().to_string();
                    self.stream_manager.cancel_reconnect(&id);
                }
            }

            // Stream viewer actions
            Action::StreamViewerShow(idx) => {
                self.selected_stream_index = Some(idx);
                self.show_stream_viewer = true;
                self.ui.focus.focus(FocusArea::StreamViewer);
            }
            Action::StreamViewerHide => {
                self.show_stream_viewer = false;
                self.ui.focus.focus(FocusArea::Menu);
            }
            Action::StreamViewerToggle => {
                if self.show_stream_viewer {
                    self.show_stream_viewer = false;
                    self.ui.focus.focus(FocusArea::Menu);
                } else if self.selected_stream_index.is_some() {
                    self.show_stream_viewer = true;
                    self.ui.focus.focus(FocusArea::StreamViewer);
                }
            }
            Action::StreamViewerScrollUp(n) => {
                self.stream_viewer.scroll_up(n);
            }
            Action::StreamViewerScrollDown(n) => {
                self.stream_viewer.scroll_down(n);
            }
            Action::StreamViewerScrollToTop => {
                self.stream_viewer.scroll_to_top();
            }
            Action::StreamViewerScrollToBottom => {
                self.stream_viewer.scroll_to_bottom();
            }

            // Stream viewer search actions (TRC-021) - placeholder, methods not yet implemented
            Action::StreamViewerSearchStart
            | Action::StreamViewerSearchClose
            | Action::StreamViewerSearchNext
            | Action::StreamViewerSearchPrev
            | Action::StreamViewerSearchQuery(_)
            | Action::StreamViewerSearchToggleCase => {
                // TODO: Implement search in StreamViewer
            }

            // Stream viewer filter actions (TRC-022) - placeholder, methods not yet implemented
            Action::StreamViewerFilterStart
            | Action::StreamViewerFilterClose
            | Action::StreamViewerFilterApply
            | Action::StreamViewerFilterPattern(_)
            | Action::StreamViewerFilterToggleCase
            | Action::StreamViewerFilterToggleRegex
            | Action::StreamViewerFilterToggleInvert
            | Action::StreamViewerFilterClear => {
                // TODO: Implement filter in StreamViewer
            }

            // Process monitor actions
            Action::ProcessRefresh
            | Action::ProcessSelectNext
            | Action::ProcessSelectPrev
            | Action::ProcessKillRequest(_)
            | Action::ProcessKillConfirm(_)
            | Action::ProcessKillCancel
            | Action::ProcessSetFilter(_)
            | Action::ProcessClearFilter
            | Action::ProcessSetSort(_)
            | Action::ProcessToggleSortOrder => {
                self.process_monitor.update(&action);
            }

            // Log viewer actions (TRC-013)
            Action::LogViewerShow => {
                self.show_log_viewer = true;
                self.ui.focus.focus(FocusArea::LogViewer);
            }
            Action::LogViewerHide => {
                self.show_log_viewer = false;
                self.ui.focus.focus(FocusArea::Menu);
            }
            Action::LogViewerToggle => {
                if self.show_log_viewer {
                    self.show_log_viewer = false;
                    self.ui.focus.focus(FocusArea::Menu);
                } else {
                    self.show_log_viewer = true;
                    self.ui.focus.focus(FocusArea::LogViewer);
                }
            }
            Action::LogViewerScrollUp(n) => {
                self.log_viewer.scroll_up(n);
            }
            Action::LogViewerScrollDown(n) => {
                self.log_viewer.scroll_down(n);
            }
            Action::LogViewerScrollToTop => {
                self.log_viewer.scroll_to_top();
            }
            Action::LogViewerScrollToBottom => {
                self.log_viewer.scroll_to_bottom();
            }
            Action::LogViewerScrollPageUp => {
                self.log_viewer.scroll_page_up();
            }
            Action::LogViewerScrollPageDown => {
                self.log_viewer.scroll_page_down();
            }
            Action::LogViewerToggleAutoScroll => {
                self.log_viewer.toggle_auto_scroll();
            }
            Action::LogViewerClear => {
                self.log_viewer.clear();
            }
            Action::LogViewerPush(target, message) => {
                self.log_viewer.push_info(target, message);
            }

            // Log viewer search actions (TRC-021) - placeholder, methods not yet implemented
            Action::LogViewerSearchStart
            | Action::LogViewerSearchClose
            | Action::LogViewerSearchNext
            | Action::LogViewerSearchPrev
            | Action::LogViewerSearchQuery(_)
            | Action::LogViewerSearchToggleCase => {
                // TODO: Implement search in LogViewer
            }

            // Log viewer filter actions (TRC-022) - placeholder, methods not yet implemented
            Action::LogViewerFilterStart
            | Action::LogViewerFilterClose
            | Action::LogViewerFilterApply
            | Action::LogViewerFilterPattern(_)
            | Action::LogViewerFilterToggleCase
            | Action::LogViewerFilterToggleRegex
            | Action::LogViewerFilterToggleInvert
            | Action::LogViewerFilterClear => {
                // TODO: Implement filter in LogViewer
            }

            // Activity Stream actions (SIRK/Forge)
            Action::ActivityStreamShow => {
                self.ui.activity_stream_visible = true;
            }
            Action::ActivityStreamHide => {
                self.ui.activity_stream_visible = false;
            }
            Action::ActivityStreamToggle => {
                self.ui.activity_stream_visible = !self.ui.activity_stream_visible;
            }
            Action::ActivityStreamClear => {
                if let Some(ref mut stream) = self.activity_stream {
                    stream.clear();
                }
            }
            Action::ActivityStreamToggleAutoScroll => {
                if let Some(ref mut stream) = self.activity_stream {
                    stream.toggle_auto_scroll();
                }
            }

            // SIRK Panel actions (Forge control)
            Action::SirkPanelShow => {
                self.ui.sirk_panel_visible = true;
            }
            Action::SirkPanelHide => {
                self.ui.sirk_panel_visible = false;
            }
            Action::SirkPanelToggle => {
                self.ui.sirk_panel_visible = !self.ui.sirk_panel_visible;
            }
            Action::SirkStart => {
                // Validate config before starting
                if let Some(ref panel) = self.sirk_panel {
                    match panel.validate_config() {
                        Ok(()) => {
                            let _config = panel.build_config();
                            // TODO: FORGE-027 will implement ForgeController.spawn(config)
                            self.ui.notification_manager.info("Forge run would start here (FORGE-027)");
                        }
                        Err(e) => {
                            self.ui.notification_manager.error(format!("Invalid config: {}", e));
                        }
                    }
                }
            }
            Action::SirkStop => {
                // TODO: FORGE-027 will implement ForgeController.stop()
                self.ui.notification_manager.info("Forge stop would happen here (FORGE-027)");
            }
            Action::SirkResume => {
                // TODO: FORGE-030 will implement resume flow
                self.ui.notification_manager.info("Forge resume would happen here (FORGE-030)");
            }

            _ => unreachable!("non-streams/process action passed to dispatch_streams_process: {:?}", action),
        }
        Ok(())
    }
}
