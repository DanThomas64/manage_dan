use crate::prelude::*;
use crossterm::{
    event::{self, Event as CEvent, KeyCode},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{io::{self, stdout}, time::Duration};

/// Represents the different screens/views the TUI can display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Todo,
    Notes,
    Project,
    Quit,
}

/// The main application state structure.
pub struct App {
    pub current_screen: Screen,
    pub systems_status: SystemsStatus,
    pub go_nogo_status: SystemsGoNogo,
}

impl App {
    pub fn new(systems_status: SystemsStatus, go_nogo_status: SystemsGoNogo) -> Self {
        App {
            current_screen: Screen::Dashboard,
            systems_status,
            go_nogo_status,
        }
    }

    /// Handles input events specific to the current screen.
    pub fn handle_input(&mut self, key_code: KeyCode) {
        match self.current_screen {
            Screen::Dashboard => self.handle_dashboard_input(key_code),
            _ => {} // Implement specific screen handlers later
        }
    }

    fn handle_dashboard_input(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char('q') => self.current_screen = Screen::Quit,
            KeyCode::Char('1') => self.current_screen = Screen::Todo,
            KeyCode::Char('2') => self.current_screen = Screen::Notes,
            KeyCode::Char('3') => self.current_screen = Screen::Project,
            _ => {}
        }
    }
}

/// Helper struct to manage terminal setup and teardown.
pub struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Tui {
    pub fn new() -> io::Result<Self> {
        stdout().execute(EnterAlternateScreen)?;
        enable_raw_mode()?;
        let backend = CrosstermBackend::new(stdout());
        let terminal = Terminal::new(backend)?;
        Ok(Tui { terminal })
    }

    pub fn draw(&mut self, app: &mut App) -> io::Result<()> {
        self.terminal.draw(|frame| {
            let area = frame.size();
            match app.current_screen {
                Screen::Dashboard => self.draw_dashboard(frame, app, area),
                Screen::Todo => self.draw_todo_placeholder(frame, area),
                _ => {}
            }
        })?;
        Ok(())
    }

    fn draw_dashboard(&self, frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
            .split(area);

        // Header
        let header_text = format!("Dashboard | Overall Status: {:?}", app.go_nogo_status.gono);
        let header = Paragraph::new(header_text)
            .block(Block::default().borders(Borders::ALL).title("System Status"));
        frame.render_widget(header, chunks[0]);

        // System Status List
        let status_lines: Vec<Line> = app.systems_status.iter().map(|(name, status)| {
            let style = match status {
                Status::Go => Style::default().fg(Color::Green),
                Status::Degraded => Style::default().fg(Color::Yellow),
                Status::Nogo => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::White),
            };
            Line::from(format!("{:<10}: {:?}", name, status)).style(style)
        }).collect();

        let status_block = Block::default().borders(Borders::ALL).title("Subsystem Health");
        let status_paragraph = Paragraph::new(status_lines).block(status_block);
        frame.render_widget(status_paragraph, chunks[1]);

        // Footer/Menu
        let footer_text = "Q: Quit | 1: Todo | 2: Notes | 3: Project";
        let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Cyan));
        frame.render_widget(footer, chunks[2]);
    }

    fn draw_todo_placeholder(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let text = Paragraph::new("TODO Screen (Press Q to quit TUI)").block(Block::default().borders(Borders::ALL).title("TODO List"));
        frame.render_widget(text, area);
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Runs the main TUI loop.
pub async fn run_tui(systems_status: SystemsStatus, go_nogo_status: SystemsGoNogo) -> AppResult {
    let mut tui = Tui::new().map_err(|e| AppError::SystemStatusMonitorFail(format!("TUI initialization failed: {}", e)))?;
    let mut app = App::new(systems_status, go_nogo_status);

    loop {
        // Draw the UI
        tui.draw(&mut app).map_err(|e| AppError::SystemStatusMonitorFail(format!("TUI draw failed: {}", e)))?;

        // Handle input events
        if event::poll(Duration::from_millis(100)).map_err(|e| AppError::SystemStatusMonitorFail(format!("TUI event poll failed: {}", e)))? {
            if let CEvent::Key(key) = event::read().map_err(|e| AppError::SystemStatusMonitorFail(format!("TUI event read failed: {}", e)))? {
                app.handle_input(key.code);
            }
        }

        // Check for quit signal
        if app.current_screen == Screen::Quit {
            break;
        }
        
        // Note: Monitoring loop runs in the background via SystemsGoNogo::start_monitoring
    }

    Ok(())
}
