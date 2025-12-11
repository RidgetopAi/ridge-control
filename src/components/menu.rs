use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::action::Action;
use crate::components::Component;
use crate::config::Theme;
use crate::streams::{ConnectionState, StreamClient};

pub struct Menu {
    selected: usize,
    stream_count: usize,
    inner_area: Rect,
}

impl Menu {
    pub fn new() -> Self {
        Self {
            selected: 0,
            stream_count: 0,
            inner_area: Rect::default(),
        }
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
    }

    pub fn set_stream_count(&mut self, count: usize) {
        self.stream_count = count;
        if self.selected >= count && count > 0 {
            self.selected = count - 1;
        }
    }

    pub fn selected_index(&self) -> usize {
        self.selected
    }

    fn select_next(&mut self) {
        if self.stream_count > 0 {
            self.selected = (self.selected + 1) % self.stream_count;
        }
    }

    fn select_prev(&mut self) {
        if self.stream_count > 0 {
            if self.selected == 0 {
                self.selected = self.stream_count - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => Some(Action::MenuSelectNext),
            KeyCode::Char('k') | KeyCode::Up => Some(Action::MenuSelectPrev),
            KeyCode::Enter => Some(Action::StreamToggle(self.selected)),
            KeyCode::Char('c') => Some(Action::StreamConnect(self.selected)),
            KeyCode::Char('d') => Some(Action::StreamDisconnect(self.selected)),
            KeyCode::Char('r') => Some(Action::StreamRefresh),
            KeyCode::Char('v') => Some(Action::StreamViewerShow(self.selected)),
            _ => None,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        if !self.inner_area.contains((mouse.column, mouse.row).into()) {
            return None;
        }

        let relative_y = mouse.row.saturating_sub(self.inner_area.y) as usize;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if relative_y < self.stream_count {
                    self.selected = relative_y;
                }
                None
            }
            MouseEventKind::ScrollUp => Some(Action::MenuSelectPrev),
            MouseEventKind::ScrollDown => Some(Action::MenuSelectNext),
            _ => None,
        }
    }

    pub fn render_with_streams(&self, frame: &mut Frame, area: Rect, focused: bool, streams: &[StreamClient], theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let title = if focused {
            " Streams [↵=toggle v=view] "
        } else {
            " Streams "
        };

        let block = Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let items: Vec<ListItem> = streams
            .iter()
            .enumerate()
            .map(|(i, client)| {
                let state_icon = client.state().to_string();
                let state_color = match client.state() {
                    ConnectionState::Connected => theme.menu.stream_connected.to_color(),
                    ConnectionState::Connecting => theme.menu.stream_connecting.to_color(),
                    ConnectionState::Reconnecting { .. } => theme.menu.stream_connecting.to_color(),
                    ConnectionState::Disconnected => theme.menu.stream_disconnected.to_color(),
                    ConnectionState::Failed => theme.menu.stream_error.to_color(),
                };

                let protocol_str = format!("[{}]", client.protocol());

                let line = Line::from(vec![
                    Span::styled(format!("{} ", state_icon), Style::default().fg(state_color)),
                    Span::styled(format!("{:5} ", protocol_str), Style::default().fg(theme.colors.accent.to_color())),
                    Span::styled(client.name(), Style::default().fg(theme.menu.item_fg.to_color())),
                ]);

                let style = if i == self.selected && focused {
                    Style::default()
                        .bg(theme.menu.selected_bg.to_color())
                        .fg(theme.menu.selected_fg.to_color())
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(line).style(style)
            })
            .collect();

        if items.is_empty() {
            let empty_msg = ListItem::new(Line::from(Span::styled(
                "No streams configured",
                Style::default().fg(theme.menu.disabled_fg.to_color()).add_modifier(Modifier::ITALIC),
            )));
            let list = List::new(vec![empty_msg]).block(block);
            frame.render_widget(list, area);
        } else {
            let mut state = ListState::default();
            if focused {
                state.select(Some(self.selected));
            }

            let list = List::new(items).block(block);
            frame.render_stateful_widget(list, area, &mut state);
        }
    }
}

impl Component for Menu {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => self.handle_key(*key),
            Event::Mouse(mouse) => self.handle_mouse(*mouse),
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::MenuSelectNext => self.select_next(),
            Action::MenuSelectPrev => self.select_prev(),
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
        let border_style = theme.border_style(focused);
        let title_style = theme.title_style(focused);

        let block = Block::default()
            .title(" Streams ")
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(border_style);

        let empty_msg = ListItem::new(Line::from(Span::styled(
            "No streams configured",
            Style::default().fg(theme.menu.disabled_fg.to_color()).add_modifier(Modifier::ITALIC),
        )));

        let list = List::new(vec![empty_msg]).block(block);
        frame.render_widget(list, area);
    }
}
