use std::collections::HashMap;
use std::time::Instant;

use crossterm::event::{Event, KeyCode, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, Borders, Row, Table, TableState},
    Frame,
};

use crate::action::{Action, SortColumn, SortOrder};
use crate::components::Component;

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: i32,
    pub name: String,
    pub state: char,
    pub cpu_percent: f64,
    pub memory_kb: u64,
    pub prev_cpu_ticks: u64,
}

impl ProcessInfo {
    pub fn state_display(&self) -> &'static str {
        match self.state {
            'R' => "Running",
            'S' => "Sleeping",
            'D' => "Disk Wait",
            'Z' => "Zombie",
            'T' => "Stopped",
            't' => "Tracing",
            'X' => "Dead",
            'I' => "Idle",
            _ => "Unknown",
        }
    }

    pub fn state_color(&self) -> Color {
        match self.state {
            'R' => Color::Green,
            'S' => Color::Blue,
            'D' => Color::Yellow,
            'Z' => Color::Red,
            'T' => Color::Magenta,
            't' => Color::Magenta,
            'X' => Color::DarkGray,
            'I' => Color::DarkGray,
            _ => Color::Gray,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmState {
    None,
    AwaitingKillConfirm(i32),
}

pub struct ProcessMonitor {
    processes: Vec<ProcessInfo>,
    prev_stats: HashMap<i32, u64>,
    table_state: TableState,
    sort_column: SortColumn,
    sort_order: SortOrder,
    filter: String,
    confirm_state: ConfirmState,
    last_refresh: Instant,
    ticks_per_second: u64,
    page_size: u64,
    inner_area: Rect,
}

impl ProcessMonitor {
    pub fn new() -> Self {
        let ticks = procfs::ticks_per_second();
        let page = procfs::page_size();
        
        let mut monitor = Self {
            processes: Vec::new(),
            prev_stats: HashMap::new(),
            table_state: TableState::default(),
            sort_column: SortColumn::Cpu,
            sort_order: SortOrder::Descending,
            filter: String::new(),
            confirm_state: ConfirmState::None,
            last_refresh: Instant::now(),
            ticks_per_second: ticks,
            page_size: page,
            inner_area: Rect::default(),
        };
        
        monitor.refresh_processes();
        monitor
    }

    pub fn refresh_processes(&mut self) {
        let elapsed_secs = self.last_refresh.elapsed().as_secs_f64().max(0.1);
        self.last_refresh = Instant::now();

        let mut new_processes = Vec::new();

        if let Ok(all_procs) = procfs::process::all_processes() {
            for process in all_procs.flatten() {
                if let Ok(stat) = process.stat() {
                    let pid = process.pid;
                    let current_ticks = stat.utime + stat.stime;

                    let cpu_percent = if let Some(&prev_ticks) = self.prev_stats.get(&pid) {
                        let delta = current_ticks.saturating_sub(prev_ticks);
                        let cpu_secs = delta as f64 / self.ticks_per_second as f64;
                        (cpu_secs / elapsed_secs) * 100.0
                    } else {
                        0.0
                    };

                    let memory_kb = (stat.rss * self.page_size) / 1024;

                    new_processes.push(ProcessInfo {
                        pid,
                        name: stat.comm.clone(),
                        state: stat.state,
                        cpu_percent,
                        memory_kb,
                        prev_cpu_ticks: current_ticks,
                    });
                }
            }
        }

        self.prev_stats = new_processes
            .iter()
            .map(|p| (p.pid, p.prev_cpu_ticks))
            .collect();

        self.processes = new_processes;
        self.sort_processes();
    }

    fn sort_processes(&mut self) {
        let order = self.sort_order;
        
        self.processes.sort_by(|a, b| {
            let cmp = match self.sort_column {
                SortColumn::Pid => a.pid.cmp(&b.pid),
                SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortColumn::Cpu => a.cpu_percent.partial_cmp(&b.cpu_percent).unwrap_or(std::cmp::Ordering::Equal),
                SortColumn::Memory => a.memory_kb.cmp(&b.memory_kb),
                SortColumn::State => a.state.cmp(&b.state),
            };
            
            match order {
                SortOrder::Ascending => cmp,
                SortOrder::Descending => cmp.reverse(),
            }
        });
    }

    fn filtered_processes(&self) -> Vec<&ProcessInfo> {
        if self.filter.is_empty() {
            self.processes.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.processes
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&filter_lower)
                        || p.pid.to_string().contains(&filter_lower)
                })
                .collect()
        }
    }

    fn selected_pid(&self) -> Option<i32> {
        let filtered = self.filtered_processes();
        self.table_state
            .selected()
            .and_then(|idx| filtered.get(idx))
            .map(|p| p.pid)
    }

    fn select_next(&mut self) {
        let count = self.filtered_processes().len();
        if count == 0 {
            self.table_state.select(None);
            return;
        }
        
        let new_idx = match self.table_state.selected() {
            Some(idx) => (idx + 1).min(count - 1),
            None => 0,
        };
        self.table_state.select(Some(new_idx));
    }

    fn select_prev(&mut self) {
        let count = self.filtered_processes().len();
        if count == 0 {
            self.table_state.select(None);
            return;
        }
        
        let new_idx = match self.table_state.selected() {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };
        self.table_state.select(Some(new_idx));
    }

    fn kill_process(&self, pid: i32) -> bool {
        use libc::{kill, SIGTERM};
        
        let result = unsafe { kill(pid, SIGTERM) };
        result == 0
    }

    fn format_memory(kb: u64) -> String {
        if kb >= 1_048_576 {
            format!("{:.1}G", kb as f64 / 1_048_576.0)
        } else if kb >= 1024 {
            format!("{:.1}M", kb as f64 / 1024.0)
        } else {
            format!("{}K", kb)
        }
    }

    fn header_spans(&self) -> Vec<Span<'static>> {
        let cols = [
            (SortColumn::Pid, "PID"),
            (SortColumn::Name, "NAME"),
            (SortColumn::Cpu, "CPU%"),
            (SortColumn::Memory, "MEM"),
            (SortColumn::State, "STATE"),
        ];

        cols.iter()
            .map(|(col, name)| {
                let indicator = if *col == self.sort_column {
                    match self.sort_order {
                        SortOrder::Ascending => " ▲",
                        SortOrder::Descending => " ▼",
                    }
                } else {
                    ""
                };

                let style = if *col == self.sort_column {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                Span::styled(format!("{}{}", name, indicator), style)
            })
            .collect()
    }

    pub fn set_inner_area(&mut self, area: Rect) {
        self.inner_area = area;
    }
}

impl Component for ProcessMonitor {
    fn handle_event(&mut self, event: &Event) -> Option<Action> {
        match event {
            Event::Key(key) => {
                if let ConfirmState::AwaitingKillConfirm(pid) = self.confirm_state {
                    return match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            Some(Action::ProcessKillConfirm(pid))
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            Some(Action::ProcessKillCancel)
                        }
                        _ => None,
                    };
                }

                match key.code {
                    KeyCode::Char('j') | KeyCode::Down => Some(Action::ProcessSelectNext),
                    KeyCode::Char('k') | KeyCode::Up => Some(Action::ProcessSelectPrev),
                    KeyCode::Char('r') => Some(Action::ProcessRefresh),
                    KeyCode::Char('x') | KeyCode::Delete => {
                        self.selected_pid().map(Action::ProcessKillRequest)
                    }
                    KeyCode::Char('1') => Some(Action::ProcessSetSort(SortColumn::Pid)),
                    KeyCode::Char('2') => Some(Action::ProcessSetSort(SortColumn::Name)),
                    KeyCode::Char('3') => Some(Action::ProcessSetSort(SortColumn::Cpu)),
                    KeyCode::Char('4') => Some(Action::ProcessSetSort(SortColumn::Memory)),
                    KeyCode::Char('5') => Some(Action::ProcessSetSort(SortColumn::State)),
                    KeyCode::Char('o') => Some(Action::ProcessToggleSortOrder),
                    KeyCode::Char('/') if key.modifiers.is_empty() => {
                        Some(Action::ProcessSetFilter(String::new()))
                    }
                    KeyCode::Esc => {
                        if !self.filter.is_empty() {
                            Some(Action::ProcessClearFilter)
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            }
            Event::Mouse(mouse) => {
                let x = mouse.column;
                let y = mouse.row;

                if !self.inner_area.contains((x, y).into()) {
                    return None;
                }

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        let row_in_table = y.saturating_sub(self.inner_area.y + 1);
                        let filtered_count = self.filtered_processes().len();
                        
                        if (row_in_table as usize) < filtered_count {
                            self.table_state.select(Some(row_in_table as usize));
                        }
                        None
                    }
                    MouseEventKind::ScrollUp => Some(Action::ProcessSelectPrev),
                    MouseEventKind::ScrollDown => Some(Action::ProcessSelectNext),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn update(&mut self, action: &Action) {
        match action {
            Action::ProcessRefresh => {
                self.refresh_processes();
            }
            Action::ProcessSelectNext => {
                self.select_next();
            }
            Action::ProcessSelectPrev => {
                self.select_prev();
            }
            Action::ProcessKillRequest(pid) => {
                self.confirm_state = ConfirmState::AwaitingKillConfirm(*pid);
            }
            Action::ProcessKillConfirm(pid) => {
                self.kill_process(*pid);
                self.confirm_state = ConfirmState::None;
                self.refresh_processes();
            }
            Action::ProcessKillCancel => {
                self.confirm_state = ConfirmState::None;
            }
            Action::ProcessSetFilter(f) => {
                self.filter = f.clone();
                self.table_state.select(if self.filtered_processes().is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Action::ProcessClearFilter => {
                self.filter.clear();
                self.table_state.select(if self.processes.is_empty() {
                    None
                } else {
                    Some(0)
                });
            }
            Action::ProcessSetSort(col) => {
                if self.sort_column == *col {
                    self.sort_order = match self.sort_order {
                        SortOrder::Ascending => SortOrder::Descending,
                        SortOrder::Descending => SortOrder::Ascending,
                    };
                } else {
                    self.sort_column = *col;
                    self.sort_order = SortOrder::Descending;
                }
                self.sort_processes();
            }
            Action::ProcessToggleSortOrder => {
                self.sort_order = match self.sort_order {
                    SortOrder::Ascending => SortOrder::Descending,
                    SortOrder::Descending => SortOrder::Ascending,
                };
                self.sort_processes();
            }
            Action::Tick => {
                if self.last_refresh.elapsed().as_secs() >= 2 {
                    self.refresh_processes();
                }
            }
            _ => {}
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        let border_color = if focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let title = if let ConfirmState::AwaitingKillConfirm(pid) = self.confirm_state {
            format!(" Kill PID {}? [y/n] ", pid)
        } else if !self.filter.is_empty() {
            format!(" Processes [filter: {}] ", self.filter)
        } else {
            format!(" Processes ({}) ", self.processes.len())
        };

        let title_style = if matches!(self.confirm_state, ConfirmState::AwaitingKillConfirm(_)) {
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(border_color)
        };

        let block = Block::default()
            .title(Span::styled(title, title_style))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let filtered = self.filtered_processes();
        
        let rows: Vec<Row> = filtered
            .iter()
            .enumerate()
            .map(|(idx, proc)| {
                let is_selected = self.table_state.selected() == Some(idx);
                
                let cells = vec![
                    Span::styled(
                        format!("{:>6}", proc.pid),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("{:<15}", truncate_string(&proc.name, 15)),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{:>5.1}", proc.cpu_percent),
                        Style::default().fg(if proc.cpu_percent > 50.0 {
                            Color::Red
                        } else if proc.cpu_percent > 10.0 {
                            Color::Yellow
                        } else {
                            Color::Green
                        }),
                    ),
                    Span::styled(
                        format!("{:>6}", Self::format_memory(proc.memory_kb)),
                        Style::default().fg(Color::Magenta),
                    ),
                    Span::styled(
                        format!("{:<8}", proc.state_display()),
                        Style::default().fg(proc.state_color()),
                    ),
                ];

                let row = Row::new(cells);
                
                if is_selected && focused {
                    row.style(
                        Style::default()
                            .bg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    row
                }
            })
            .collect();

        let header = Row::new(self.header_spans())
            .style(Style::default().add_modifier(Modifier::BOLD))
            .height(1);

        let widths = [
            Constraint::Length(7),  // PID
            Constraint::Min(15),    // NAME
            Constraint::Length(6),  // CPU%
            Constraint::Length(7),  // MEM
            Constraint::Length(10), // STATE
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(if focused {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            });

        frame.render_stateful_widget(table, area, &mut self.table_state.clone());
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_memory() {
        assert_eq!(ProcessMonitor::format_memory(512), "512K");
        assert_eq!(ProcessMonitor::format_memory(2048), "2.0M");
        assert_eq!(ProcessMonitor::format_memory(1_572_864), "1.5G");
    }

    #[test]
    fn test_process_info_state_display() {
        let proc = ProcessInfo {
            pid: 1,
            name: "test".to_string(),
            state: 'R',
            cpu_percent: 0.0,
            memory_kb: 0,
            prev_cpu_ticks: 0,
        };
        assert_eq!(proc.state_display(), "Running");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(truncate_string("verylongprocessname", 10), "verylongp…");
    }
}
