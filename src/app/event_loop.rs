// Event loop - main run() method and PTY polling

use std::time::{Duration, Instant};

use crossterm::event;

use super::{App, TICK_INTERVAL_MS};
use crate::action::Action;
use crate::config::ConfigEvent;
use crate::error::{Result, RidgeError};
use crate::event::PtyEvent;
use crate::input::mode::InputMode;
use crate::tabs::TabId;

impl App {
    /// Poll PTY events from all tabs. Returns true if any events were processed.
    pub(super) fn poll_pty_events(&mut self) -> bool {
        // Collect events first to avoid borrow issues
        let mut events: Vec<(TabId, PtyEvent)> = Vec::new();
        
        for rx in &mut self.pty_receivers {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        
        if events.is_empty() {
            return false;
        }
        
        // Process collected events
        for (tab_id, event) in events {
            match event {
                PtyEvent::Output(data) => {
                    self.tab_manager.process_pty_output(tab_id, &data);
                    // Mark tab as having activity if not active
                    if tab_id != self.tab_manager.active_tab().id() {
                        self.tab_manager.set_tab_activity(tab_id, true);
                    }
                }
                PtyEvent::Exited(code) => {
                    self.tab_manager.mark_pty_dead(tab_id);
                    // If main tab (id 0) dies, quit the app
                    if tab_id == 0 {
                        self.should_quit = true;
                    } else {
                        // TRC-023: Notify on background tab shell exit
                        self.notification_manager.info_with_message(
                            "Shell Exited",
                            format!("Tab {} exited with code {}", tab_id, code)
                        );
                    }
                }
                PtyEvent::Error(err) => {
                    self.tab_manager.mark_pty_dead(tab_id);
                    if tab_id == 0 {
                        self.should_quit = true;
                    } else {
                        // TRC-023: Notify on background tab shell error
                        self.notification_manager.error_with_message(
                            "Shell Error",
                            format!("Tab {}: {}", tab_id, err)
                        );
                    }
                }
            }
        }
        
        true
    }

    pub fn run(&mut self) -> Result<()> {
        let mut stream_rx = self.stream_manager.take_event_rx();

        loop {
            // ---- 1. Poll non-input sources ----

            // Poll PTY events from all tabs (TRC-005)
            if self.poll_pty_events() {
                self.mark_dirty();
            }

            // Poll stream events
            if let Some(ref mut rx) = stream_rx {
                let mut had_stream_events = false;
                while let Ok(stream_event) = rx.try_recv() {
                    had_stream_events = true;
                    self.handle_stream_event(stream_event);
                }
                if had_stream_events {
                    self.mark_dirty();
                }
            }
            
            // TP2-002-FIX-01: Poll AgentEngine's internal LLM events and forward to engine
            let agent_llm_events: Vec<_> = if let Some(ref mut rx) = self.agent_llm_event_rx {
                let mut events = Vec::new();
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
                events
            } else {
                Vec::new()
            };
            
            if !agent_llm_events.is_empty() {
                for ev in agent_llm_events {
                    self.agent_engine.handle_llm_event(ev);
                }
                self.mark_dirty();
            }
            
            // Poll AgentEngine events (TP2-002-05)
            let agent_events: Vec<_> = if let Some(ref mut rx) = self.agent_event_rx {
                let mut events = Vec::new();
                while let Ok(event) = rx.try_recv() {
                    events.push(event);
                }
                events
            } else {
                Vec::new()
            };
            
            if !agent_events.is_empty() {
                for agent_event in agent_events {
                    self.handle_agent_event(agent_event);
                }
                self.mark_dirty();
            }
            
            // Poll tool execution results from all receivers
            let tool_results: Vec<(String, std::result::Result<crate::llm::ToolResult, crate::llm::ToolError>)> = {
                let mut results = Vec::new();
                for (tool_id, rx) in self.tool_result_rxs.iter_mut() {
                    while let Ok(result) = rx.try_recv() {
                        results.push((tool_id.clone(), result));
                    }
                }
                results
            };
            
            if !tool_results.is_empty() {
                for (tool_id, result) in tool_results {
                    match result {
                        Ok(tool_result) => {
                            self.dispatch(Action::ToolResult(tool_result))?;
                        }
                        Err(crate::llm::ToolError::WaitingForUserInput { tool_use_id, questions }) => {
                            // T2.4: Show ask_user dialog instead of sending error
                            self.ask_user_dialog.show(tool_use_id, questions);
                        }
                        Err(e) => {
                            // Create an error result using the tool_id we tracked
                            let error_result = crate::llm::ToolResult {
                                tool_use_id: tool_id,
                                content: crate::llm::ToolResultContent::Text(e.to_string()),
                                is_error: true,
                            };
                            self.dispatch(Action::ToolResult(error_result))?;
                        }
                    }
                }
                self.mark_dirty();
            }
            
            // Clean up completed receivers (those with no pending tools)
            self.tool_result_rxs.retain(|id, _| self.pending_tools.contains_key(id));

            // Tick (drives animations/timeouts)
            if self.last_tick.elapsed() >= Duration::from_millis(TICK_INTERVAL_MS) {
                self.dispatch(Action::Tick)?;
                self.last_tick = Instant::now();
                self.mark_dirty();
            }
            
            // Poll config watcher for file changes
            let config_events: Vec<_> = if let Some(ref mut watcher) = self.config_watcher {
                watcher.poll_events()
            } else {
                Vec::new()
            };
            
            for event in config_events {
                match event {
                    ConfigEvent::Changed(path) => {
                        self.dispatch(Action::ConfigChanged(path))?;
                        self.mark_dirty();
                    }
                    ConfigEvent::Error(msg) => {
                        tracing::warn!("Config watcher error: {}", msg);
                    }
                }
            }

            if self.should_quit {
                break;
            }

            // ---- 2. Poll user input (keys/mouse/resize) ----

            if event::poll(Duration::from_millis(16)).map_err(|e| RidgeError::Terminal(e.to_string()))? {
                let event = event::read().map_err(|e| RidgeError::Terminal(e.to_string()))?;

                // Any user input implies we want to give UI feedback
                self.mark_dirty();

                if let Some(action) = self.handle_event(event) {
                    // Track if this action came from command palette
                    let was_in_palette = matches!(self.input_mode, InputMode::CommandPalette);

                    // Dispatch the action
                    self.dispatch(action)?;

                    // POST-DISPATCH HOOK: Reset mode after palette actions complete
                    if was_in_palette && matches!(self.input_mode, InputMode::CommandPalette) {
                        self.input_mode = InputMode::Normal;
                    }
                }
            }

            if self.should_quit {
                break;
            }

            // ---- 3. Draw once if anything changed ----

            if self.needs_redraw {
                self.draw()?;
                self.needs_redraw = false;
            }
        }

        Ok(())
    }
}
