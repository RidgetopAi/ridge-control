// Event loop - main run() method using tokio::select! for event-driven processing
// Order 9: Converted from polling-based to event-driven architecture

use std::time::{Duration, Instant};

use crossterm::event::{self, Event as CrosstermEvent, MouseEventKind};
use tokio::sync::mpsc;

use super::{App, TICK_INTERVAL_MS};
use crate::action::Action;
use crate::config::ConfigEvent;
use crate::error::Result;
use crate::event::PtyEvent;
use crate::input::mode::InputMode;
use crate::llm::{ToolError, ToolResult, ToolResultContent};
use crate::sirk::ForgeEvent;
use crate::tabs::TabId;

impl App {
    /// Handle a single PTY event
    fn handle_pty_event(&mut self, tab_id: TabId, event: PtyEvent) {
        match event {
            PtyEvent::Output(data) => {
                self.pty.tab_manager.process_pty_output(tab_id, &data);
                if tab_id != self.pty.tab_manager.active_tab().id() {
                    self.pty.tab_manager.set_tab_activity(tab_id, true);
                }
            }
            PtyEvent::Exited(code) => {
                self.pty.tab_manager.mark_pty_dead(tab_id);
                if tab_id == 0 {
                    self.should_quit = true;
                } else {
                    self.ui.notification_manager.info_with_message(
                        "Shell Exited",
                        format!("Tab {} exited with code {}", tab_id, code)
                    );
                }
            }
            PtyEvent::Error(err) => {
                self.pty.tab_manager.mark_pty_dead(tab_id);
                if tab_id == 0 {
                    self.should_quit = true;
                } else {
                    self.ui.notification_manager.error_with_message(
                        "Shell Error",
                        format!("Tab {}: {}", tab_id, err)
                    );
                }
            }
        }
        self.mark_dirty();
    }

    /// Handle a tool execution result
    fn handle_tool_result(&mut self, tool_id: String, result: std::result::Result<ToolResult, ToolError>) -> Result<()> {
        match result {
            Ok(tool_result) => {
                self.dispatch(Action::ToolResult(tool_result))?;
            }
            Err(ToolError::WaitingForUserInput { tool_use_id, questions }) => {
                self.ui.ask_user_dialog.show(tool_use_id, questions);
            }
            Err(e) => {
                let error_result = ToolResult {
                    tool_use_id: tool_id,
                    content: ToolResultContent::Text(e.to_string()),
                    is_error: true,
                };
                self.dispatch(Action::ToolResult(error_result))?;
            }
        }
        self.mark_dirty();
        Ok(())
    }

    /// Handle user input event.
    /// Only marks dirty when the event actually produces an action,
    /// avoiding unnecessary redraws for unhandled mouse events.
    /// Overlays (command palette, confirm dialog, etc.) consume input
    /// without returning an Action, so they always need a redraw.
    fn handle_input_event(&mut self, event: crossterm::event::Event) -> Result<()> {
        let overlay_active = matches!(self.ui.input_mode, InputMode::CommandPalette | InputMode::Confirm { .. })
            || self.ui.ask_user_dialog.is_visible();

        if let Some(action) = self.handle_event(event) {
            // PtyInput just writes bytes to the PTY — no visual change until the
            // shell echoes back (handled by handle_pty_event). Skipping mark_dirty
            // here avoids a premature render that would push the echo render into
            // the throttle window, adding up to MIN_RENDER_INTERVAL_MS latency.
            if !matches!(action, Action::PtyInput(_)) {
                self.mark_dirty();
            }
            let was_in_palette = matches!(self.ui.input_mode, InputMode::CommandPalette);
            self.dispatch(action)?;
            if was_in_palette && matches!(self.ui.input_mode, InputMode::CommandPalette) {
                self.ui.input_mode = InputMode::Normal;
            }
        } else if overlay_active {
            self.mark_dirty();
        }
        Ok(())
    }

    /// Handle a ForgeEvent from the Forge subprocess
    fn handle_forge_event(&mut self, event: ForgeEvent) {
        if let Some(ref mut panel) = self.sirk_panel {
            match event {
                ForgeEvent::RunStarted(e) => {
                    panel.run_started(e.total_instances);
                    self.ui.notification_manager.info(format!(
                        "Forge run '{}' started ({} instances)",
                        e.run_name, e.total_instances
                    ));
                }
                ForgeEvent::InstanceStarted(e) => {
                    panel.instance_started(e.instance_number);
                }
                ForgeEvent::InstanceCompleted(e) => {
                    panel.instance_completed(e.success);
                    if !e.success {
                        self.ui.notification_manager.warning(format!(
                            "Instance {} completed with failure",
                            e.instance_number
                        ));
                    }
                }
                ForgeEvent::InstanceFailed(e) => {
                    panel.instance_completed(false);
                    self.ui.notification_manager.error(format!(
                        "Instance {} failed: {}",
                        e.instance_number, e.error
                    ));
                }
                ForgeEvent::RunCompleted(e) => {
                    panel.run_completed();
                    self.ui.notification_manager.success(format!(
                        "Forge run '{}' completed: {} succeeded, {} failed",
                        e.run_name, e.success_count, e.fail_count
                    ));
                }
                ForgeEvent::Error(e) => {
                    if e.fatal {
                        panel.run_failed(e.message.clone());
                        self.ui.notification_manager.error(format!("Forge fatal error: {}", e.message));
                    } else {
                        self.ui.notification_manager.warning(format!("Forge warning: {}", e.message));
                    }
                }
                ForgeEvent::ResumePrompt(e) => {
                    // Store the pending resume prompt for user decision
                    self.forge_resume_pending = Some(e.clone());
                    // Show notification to user
                    self.ui.notification_manager.info(format!(
                        "Resume available: {} ({}/{} completed) - Press 'r' to resume or 'a' to abort",
                        e.run_name, e.last_instance_completed, e.total_instances
                    ));
                }
                ForgeEvent::StderrLine(e) => {
                    // Push stderr output to activity stream for display
                    if let Some(ref mut activity_stream) = self.activity_stream {
                        activity_stream.push_text(e.line, e.timestamp);
                    }
                }
            }
        }
        self.mark_dirty();
    }

    /// Process pending Forge spawn request (async)
    async fn process_forge_spawn(&mut self) {
        if let Some(config) = self.forge_spawn_pending.take() {
            match self.forge_controller.spawn(config).await {
                Ok(rx) => {
                    self.forge_event_rx = Some(rx);
                    // Panel state is updated via events from Forge
                }
                Err(e) => {
                    self.ui.notification_manager.error(format!("Failed to start Forge: {}", e));
                    if let Some(ref mut panel) = self.sirk_panel {
                        panel.run_failed(e.to_string());
                    }
                }
            }
            self.mark_dirty();
        }
    }

    /// Process pending Forge stop request (async)
    async fn process_forge_stop(&mut self) {
        if self.forge_stop_pending {
            self.forge_stop_pending = false;
            if let Err(e) = self.forge_controller.stop().await {
                self.ui.notification_manager.error(format!("Failed to stop Forge: {}", e));
            } else {
                self.forge_event_rx = None;
                if let Some(ref mut panel) = self.sirk_panel {
                    panel.run_paused();
                }
                self.ui.notification_manager.info("Forge run stopped");
            }
            self.mark_dirty();
        }
    }

    /// Process pending Forge reset request (async)
    async fn process_forge_reset(&mut self) {
        if self.forge_reset_pending {
            self.forge_reset_pending = false;
            self.forge_controller.reset().await;
            self.forge_event_rx = None;
            // Delete state file so next Start is truly fresh
            if let Some(ref panel) = self.sirk_panel {
                let run_name = panel.run_name();
                if !run_name.is_empty() {
                    let state_path = dirs::home_dir()
                        .unwrap_or_default()
                        .join(".forge")
                        .join("runs")
                        .join(run_name)
                        .join("state.json");
                    if let Err(e) = tokio::fs::remove_file(&state_path).await {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            tracing::warn!("Failed to delete state file: {}", e);
                        }
                    }
                }
            }
            self.mark_dirty();
        }
    }

    /// Process pending Forge resume response (async)
    async fn process_forge_resume_response(&mut self) {
        if let Some(should_resume) = self.forge_resume_response_pending.take() {
            // Clear the pending prompt since we're responding
            self.forge_resume_pending = None;

            let response = if should_resume {
                crate::sirk::ForgeResumeResponse::resume()
            } else {
                crate::sirk::ForgeResumeResponse::abort()
            };

            match self.forge_controller.send_resume_response(response).await {
                Ok(()) => {
                    if should_resume {
                        self.ui.notification_manager.success("Forge run resumed");
                    } else {
                        self.ui.notification_manager.info("Forge run aborted");
                        // Stop the subprocess since user aborted
                        if let Err(e) = self.forge_controller.stop().await {
                            self.ui.notification_manager.error(format!("Failed to stop Forge: {}", e));
                        }
                        self.forge_event_rx = None;
                        if let Some(ref mut panel) = self.sirk_panel {
                            panel.run_paused();
                        }
                    }
                }
                Err(e) => {
                    self.ui.notification_manager.error(format!("Failed to send resume response: {}", e));
                }
            }
            self.mark_dirty();
        }
    }

    /// Handle config change event
    fn handle_config_event(&mut self, event: ConfigEvent) -> Result<()> {
        match event {
            ConfigEvent::Changed(path) => {
                self.dispatch(Action::ConfigChanged(path))?;
                self.mark_dirty();
            }
            ConfigEvent::Error(msg) => {
                tracing::warn!("Config watcher error: {}", msg);
            }
        }
        Ok(())
    }

    /// Spawn the input reader thread that forwards crossterm events to a tokio channel.
    /// Uses adaptive poll rate: 16ms when active, 33ms when idle (after 2s).
    /// Filters out high-frequency mouse motion events that flood TMUX multiplexers.
    fn spawn_input_reader(&self) -> mpsc::UnboundedReceiver<crossterm::event::Event> {
        let (tx, rx) = mpsc::unbounded_channel();
        let in_tmux = std::env::var("TMUX").is_ok();

        std::thread::spawn(move || {
            let mut last_activity = Instant::now();

            loop {
                // Adaptive poll rate: 16ms active, 33ms idle.
                // Idle threshold at 2s so normal typing pauses stay in active mode.
                let idle = last_activity.elapsed() > Duration::from_millis(2000);
                let poll_timeout = if idle {
                    Duration::from_millis(33)
                } else {
                    Duration::from_millis(16)
                };

                match event::poll(poll_timeout) {
                    Ok(true) => {
                        match event::read() {
                            Ok(ev) => {
                                // In TMUX, drop pure mouse-move events (no button held).
                                // These are the highest-volume events and flood the
                                // multiplexer's escape sequence pipeline, causing typing lag.
                                // Clicks, scrolls, and button-held drags still pass through.
                                if in_tmux {
                                    if let CrosstermEvent::Mouse(ref mouse) = ev {
                                        if matches!(mouse.kind, MouseEventKind::Moved) {
                                            continue;
                                        }
                                    }
                                }

                                last_activity = Instant::now();
                                if tx.send(ev).is_err() {
                                    break; // App dropped, exit thread
                                }
                            }
                            Err(e) => {
                                tracing::error!("crossterm read error: {}", e);
                                break;
                            }
                        }
                    }
                    Ok(false) => {
                        // Timeout, continue polling
                    }
                    Err(e) => {
                        tracing::error!("crossterm poll error: {}", e);
                        break;
                    }
                }
            }
        });

        rx
    }

    /// Spawn a thread that forwards config watcher events to a tokio channel.
    fn spawn_config_watcher_adapter(&mut self) -> Option<mpsc::UnboundedReceiver<ConfigEvent>> {
        let watcher = self.config_watcher.take()?;
        let (tx, rx) = mpsc::unbounded_channel();

        std::thread::spawn(move || {
            let mut watcher = watcher;
            loop {
                // Poll at a reasonable interval
                std::thread::sleep(Duration::from_millis(100));
                
                for event in watcher.poll_events() {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            }
        });

        Some(rx)
    }

    /// Main event loop using tokio::select! for true event-driven processing.
    /// Order 9: Replaces polling-based loop with async event handling.
    pub async fn run(&mut self) -> Result<()> {
        let throttle_ms = crate::app::ui_state::MIN_RENDER_INTERVAL_MS;

        // Take ownership of event receivers
        let mut stream_rx = self.stream_manager.take_event_rx();
        
        // Spawn adapters for blocking sources
        let mut input_rx = self.spawn_input_reader();
        let mut config_rx = self.spawn_config_watcher_adapter();

        // Create a unified PTY event channel and spawn forwarders
        // Keep pty_tx alive so new tabs can add their receivers dynamically
        let (pty_tx, mut pty_rx) = mpsc::unbounded_channel::<(TabId, PtyEvent)>();
        for rx in self.pty.pty_receivers.drain(..) {
            let tx = pty_tx.clone();
            tokio::spawn(async move {
                let mut rx = rx;
                while let Some(event) = rx.recv().await {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
            });
        }
        // NOTE: Don't drop pty_tx - we need it to forward new tab PTY receivers

        // Create unified tool result channel
        let (tool_tx, mut tool_rx) = mpsc::unbounded_channel::<(String, std::result::Result<ToolResult, ToolError>)>();
        
        // Track active tool forwarder count
        let mut tool_forwarder_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

        loop {
            // Calculate next tick deadline
            let tick_deadline = self.last_tick + Duration::from_millis(TICK_INTERVAL_MS);
            let now = Instant::now();
            let tick_remaining = if tick_deadline > now {
                tick_deadline - now
            } else {
                Duration::ZERO
            };

            // When a render is pending but throttled, wake up after the throttle
            // expires instead of waiting up to 500ms for the next tick.
            let timer_duration = if self.ui.needs_redraw {
                let render_deadline = self.ui.last_render + Duration::from_millis(throttle_ms);
                let render_remaining = if render_deadline > now {
                    render_deadline - now
                } else {
                    Duration::ZERO
                };
                render_remaining.min(tick_remaining)
            } else {
                tick_remaining
            };

            // Spawn forwarders for any new PTY receivers (from new tabs)
            for rx in self.pty.pty_receivers.drain(..) {
                let tx = pty_tx.clone();
                tokio::spawn(async move {
                    let mut rx = rx;
                    while let Some(event) = rx.recv().await {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                });
            }

            // Spawn forwarders for any new tool result receivers
            for (tool_id, rx) in self.agent.tool_result_rxs.drain() {
                let tx = tool_tx.clone();
                let id = tool_id.clone();
                let handle = tokio::spawn(async move {
                    let mut rx = rx;
                    while let Some(result) = rx.recv().await {
                        if tx.send((id.clone(), result)).is_err() {
                            break;
                        }
                    }
                });
                tool_forwarder_handles.push(handle);
            }

            // Process pending Forge spawn/stop/reset/resume requests
            self.process_forge_spawn().await;
            self.process_forge_stop().await;
            self.process_forge_reset().await;
            self.process_forge_resume_response().await;

            tokio::select! {
                biased;  // Prioritize in order listed

                // 1. User input (highest priority for responsiveness)
                // Drain all buffered input events to batch rapid typing
                Some(event) = input_rx.recv() => {
                    self.handle_input_event(event)?;
                    // Drain all buffered input events to batch rapid typing
                    while let Ok(ev) = input_rx.try_recv() {
                        self.handle_input_event(ev)?;
                    }
                }

                // 2. PTY events
                Some((tab_id, event)) = pty_rx.recv() => {
                    self.handle_pty_event(tab_id, event);
                    // Drain any buffered PTY events for efficiency
                    while let Ok((tid, ev)) = pty_rx.try_recv() {
                        self.handle_pty_event(tid, ev);
                    }
                }

                // 3. Agent events
                Some(agent_event) = async {
                    if let Some(ref mut rx) = self.agent.agent_event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    // Collect all buffered events first
                    let mut events = vec![agent_event];
                    if let Some(ref mut rx) = self.agent.agent_event_rx {
                        while let Ok(ev) = rx.try_recv() {
                            events.push(ev);
                        }
                    }
                    // Then process them
                    for ev in events {
                        self.handle_agent_event(ev);
                    }
                    self.mark_dirty();
                }

                // 4. Agent LLM events (forwarded to engine)
                Some(llm_event) = async {
                    if let Some(ref mut rx) = self.agent.agent_llm_event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    // Collect all buffered events first
                    let mut events = vec![llm_event];
                    if let Some(ref mut rx) = self.agent.agent_llm_event_rx {
                        while let Ok(ev) = rx.try_recv() {
                            events.push(ev);
                        }
                    }
                    // Then process them
                    for ev in events {
                        self.agent.agent_engine.handle_llm_event(ev);
                    }
                    self.mark_dirty();
                }

                // 5. Tool results
                Some((tool_id, result)) = tool_rx.recv() => {
                    self.handle_tool_result(tool_id, result)?;
                    // Drain buffered tool results
                    while let Ok((tid, res)) = tool_rx.try_recv() {
                        self.handle_tool_result(tid, res)?;
                    }
                    // Clean up pending tools
                    self.agent.pending_tools.retain(|id, _| {
                        // Keep if still has active receiver or in collected results
                        self.agent.collected_results.contains_key(id)
                    });
                }

                // 6. Stream events
                Some(stream_event) = async {
                    if let Some(ref mut rx) = stream_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    self.handle_stream_event(stream_event);
                    // Drain buffered stream events
                    if let Some(ref mut rx) = stream_rx {
                        while let Ok(ev) = rx.try_recv() {
                            self.handle_stream_event(ev);
                        }
                    }
                    self.mark_dirty();
                }

                // 7. Config events
                Some(config_event) = async {
                    if let Some(ref mut rx) = config_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    self.handle_config_event(config_event)?;
                }

                // 8. Forge events (SIRK subprocess)
                Some(forge_event) = async {
                    if let Some(ref mut rx) = self.forge_event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    // Collect all buffered events first
                    let mut events = vec![forge_event];
                    if let Some(ref mut rx) = self.forge_event_rx {
                        while let Ok(ev) = rx.try_recv() {
                            events.push(ev);
                        }
                    }
                    // Then process them
                    for ev in events {
                        self.handle_forge_event(ev);
                    }
                }

                // 9. Spindles activity events (WebSocket from spindles-proxy)
                Some(spindles_event) = async {
                    if let Some(ref mut rx) = self.spindles_event_rx {
                        rx.recv().await
                    } else {
                        std::future::pending().await
                    }
                } => {
                    // Drain all buffered spindles events
                    if let Some(ref mut rx) = self.spindles_event_rx {
                        while let Ok(_ev) = rx.try_recv() {
                            // Events already stored in activity_store by the WebSocket handler
                        }
                    }
                    // Update connection state from events
                    use crate::spindles::SpindlesEvent;
                    match spindles_event {
                        SpindlesEvent::Connected => {
                            self.spindles_stream.set_state(crate::spindles::SpindlesConnectionState::Connected);
                        }
                        SpindlesEvent::Disconnected(_) => {
                            self.spindles_stream.set_state(crate::spindles::SpindlesConnectionState::Disconnected);
                        }
                        SpindlesEvent::StateChanged(state) => {
                            self.spindles_stream.set_state(state);
                        }
                        _ => {}
                    }
                    // Auto-scroll activity stream on new data
                    if let Some(ref mut activity_stream) = self.activity_stream {
                        activity_stream.scroll_to_bottom();
                    }
                    self.mark_dirty();
                }

                // 10. Timer: fires for pending render deadline or tick, whichever is sooner
                _ = tokio::time::sleep(timer_duration) => {
                    // Only dispatch tick when actually due
                    if self.last_tick.elapsed() >= Duration::from_millis(TICK_INTERVAL_MS) {
                        self.dispatch(Action::Tick)?;
                        self.last_tick = Instant::now();
                        if self.ui.spinner_manager.active_count() > 0 {
                            self.mark_dirty();
                        }
                    }
                    // Pending render will be handled by the render check below
                }
            }

            // Check quit condition
            if self.should_quit {
                break;
            }

            // Draw once if anything changed, throttled to prevent
            // escape sequence floods that cause TMUX lag.
            // Draw once if anything changed, throttled to ~30 FPS to prevent
            // escape sequence floods that cause TMUX lag.
            if self.ui.needs_redraw {
                let since_last_render = Instant::now().duration_since(self.ui.last_render);
                if since_last_render >= Duration::from_millis(throttle_ms) {
                    self.draw()?;
                    self.ui.needs_redraw = false;
                    self.ui.last_render = Instant::now();
                }
            }
        }

        Ok(())
    }
}
