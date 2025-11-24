use std::{
    collections::VecDeque,
    io::{self, stdout},
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
    use ratatui::{
    backend::CrosstermBackend,
    prelude::*,
    widgets::{block::Title, Block, Borders, List, ListItem, ListState, Paragraph, Table, Row, Cell},
    style::{Color, Stylize}, // Use Ratatui's Color and Stylize
    text::Line, // Import Line for explicit conversion
};use tokio::sync::{mpsc, broadcast}; // Add broadcast
use dircrab::{ScanEvent, ControlEvent}; // Import ControlEvent

pub type Tui = Terminal<CrosstermBackend<io::Stdout>>;



/// The application state for the TUI.
#[derive(Debug)]
pub struct App {
    pub found_urls: VecDeque<String>,
    pub requests_completed: usize,
    pub errors_occurred: usize,
    pub total_words: usize,
    pub current_word_index: usize,
    pub start_time: Instant,
    pub scan_finished: bool,
    pub scan_stopped: bool, // New field for user-initiated stop
}

impl Default for App {
    fn default() -> Self {
        Self {
            found_urls: VecDeque::with_capacity(100), // Keep last 100 found URLs
            requests_completed: 0,
            errors_occurred: 0,
            total_words: 0,
            current_word_index: 0,
            start_time: Instant::now(),
            scan_finished: false,
            scan_stopped: false,
        }
    }
}

impl App {
    /// Adds a found URL to the list, maintaining a maximum capacity.
    pub fn add_found_url(&mut self, url: String) {
        if self.found_urls.len() == self.found_urls.capacity() {
            self.found_urls.pop_front();
        }
        self.found_urls.push_back(url);
    }

    /// Calculates requests per second.
    pub fn rps(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if self.scan_stopped || self.scan_finished || elapsed == 0.0 {
            0.0
        } else {
            self.requests_completed as f64 / elapsed
        }
    }

    /// Calculates scan progress as a percentage.
    pub fn progress(&self) -> f64 {
        if self.total_words > 0 {
            (self.current_word_index as f64 / self.total_words as f64) * 100.0
        } else {
            0.0
        }
    }
}

pub fn init() -> io::Result<Tui> {
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

pub async fn run_tui(terminal: &mut Tui, mut rx_events: mpsc::Receiver<ScanEvent>, _tx_control: broadcast::Sender<ControlEvent>) -> io::Result<()> {
    let mut app = App::default();
    let mut last_tick = Instant::now();
    let tick_rate = Duration::from_millis(250);

    let (tx_key_events, mut rx_key_events) = mpsc::channel(100);

    // Spawn a blocking task to read crossterm events
    tokio::spawn(async move {
        loop {
            if let Ok(event) = event::read() {
                if tx_key_events.send(event).await.is_err() {
                    // Receiver dropped, exit task
                    break;
                }
            }
        }
    });

    loop {
        terminal.draw(|frame| {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(frame.size());

            // Top section: Statistics
            let stats_block_area = layout[0];
            let stats_block = Block::default()
                .title(Title::from(Line::from(" DirCrab TUI Dashboard ".bold())))
                .borders(Borders::ALL);
            
            // Render the stats_block itself into stats_block_area
            frame.render_widget(stats_block.clone(), stats_block_area);

            // Now, split the *inner area* of the rendered stats_block
            let inner_stats_area = stats_block.inner(stats_block_area);
            
            let stats_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(inner_stats_area);

            let stats_table_rows = vec![
                Row::new(vec![
                    Cell::from("Progress:").bold(),
                    Cell::from(format!("{:.2}%", if app.scan_stopped || app.scan_finished { 100.0 } else { app.progress() })).fg(Color::Green),
                ]),
                Row::new(vec![
                    Cell::from("Words Processed:").bold(),
                    Cell::from(format!("{}/{}", app.current_word_index, app.total_words)),
                ]),
                Row::new(vec![
                    Cell::from("RPS:").bold(),
                    Cell::from(format!("{:.2}", if app.scan_stopped || app.scan_finished { 0.0 } else { app.rps() })).fg(Color::Blue),
                ]),
                Row::new(vec![
                    Cell::from("Errors:").bold(),
                    Cell::from(format!("{}", app.errors_occurred)).fg(Color::Red),
                ]),
            ];

            let stats_table = Table::new(
                stats_table_rows,
                [Constraint::Length(18), Constraint::Min(10)],
            )
                .column_spacing(1);
            
            frame.render_widget(stats_table, stats_layout[0]);

            let status_text = if app.scan_finished {
                Line::from("Scan Finished!".green().bold())
            } else if app.scan_stopped {
                Line::from("Scan Stopped!".red().bold())
            } else {
                Line::from("Scanning...".yellow().bold())
            };
            let status_widget = Paragraph::new(status_text);
            frame.render_widget(status_widget, stats_layout[1]);


            // Bottom section: Found URLs
            let found_urls_block = Block::default()
                .title(Title::from(Line::from(" Found URLs ".bold())))
                .borders(Borders::ALL);

            let items: Vec<ListItem> = app.found_urls
                .iter()
                .rev()
                .map(|u| {
                    let mut item = ListItem::new(u.clone());
                    if u.starts_with("Error:") {
                        item = item.style(Style::default().fg(Color::Red));
                    } else if u.starts_with("Warning:") {
                        item = item.style(Style::default().fg(Color::Yellow));
                    } else {
                        item = item.style(Style::default().fg(Color::Green));
                    }
                    item
                })
                .collect();
            let found_urls_list = List::new(items)
                .block(found_urls_block)
                .highlight_style(Style::default().fg(Color::LightBlue).bold())
                .highlight_symbol(">> ")
                .repeat_highlight_symbol(true)
                .direction(ratatui::widgets::ListDirection::TopToBottom);

            let mut list_state = ListState::default(); // Not using selection, but needed for List widget
            frame.render_stateful_widget(found_urls_list, layout[1], &mut list_state);
        })?;

        tokio::select! {
            _ = tokio::time::sleep(tick_rate.checked_sub(last_tick.elapsed()).unwrap_or_else(|| Duration::from_secs(0))) => {
                last_tick = Instant::now();
            }
            Some(event) = rx_events.recv() => {
                match event {
                    ScanEvent::FoundUrl(url) => app.add_found_url(url),
                    ScanEvent::RequestCompleted => app.requests_completed += 1,
                    ScanEvent::ErrorOccurred(msg) => {
                        app.errors_occurred += 1;
                        app.add_found_url(format!("Error: {}", msg));
                    },
                    ScanEvent::Warning(msg) => {
                        app.add_found_url(format!("Warning: {}", msg));
                    },
                    ScanEvent::ScanStarted { total_words } => {
                        app.total_words = total_words;
                        app.start_time = Instant::now();
                        app.scan_finished = false;
                        app.scan_stopped = false;
                    },
                    ScanEvent::ScanFinished => app.scan_finished = true,
                    ScanEvent::ScanStopped => app.scan_stopped = true, // Handle the new event
                }
            }
            Some(key_event) = rx_key_events.recv() => {
                if let Event::Key(key) = key_event {
                    if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                        break; // Exit the TUI loop
                    }
                }
            }
        }
    }
    Ok(())
}