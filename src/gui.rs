/// A Ratatui example that demonstrates how to draw on a canvas.
///
/// This example demonstrates how to draw various shapes such as rectangles, circles, and lines
/// on a canvas. It also demonstrates how to draw a map.
///
/// This example runs with the Ratatui library code in the branch that you are currently
/// reading. See the [`latest`] branch for the code which works with the most recent Ratatui
/// release.
///
/// [`latest`]: https://github.com/ratatui/ratatui/tree/latest
use std::{
    io::stdout,
    time::{Duration, Instant},
};

use color_eyre::Result;
use crossterm::ExecutableCommand;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseEventKind,
};
use itertools::Itertools;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::Text;
use ratatui::widgets::canvas::{Canvas, Circle, Map, MapResolution, Points, Rectangle};
use ratatui::widgets::{Block, Widget};
use ratatui::{DefaultTerminal, Frame};

pub fn tui() -> Result<()> {
    color_eyre::install()?;
    stdout().execute(EnableMouseCapture)?;
    let terminal = ratatui::init();
    let app_result = App::new().run(terminal);
    ratatui::restore();
    stdout().execute(DisableMouseCapture)?;
    app_result
}

struct App {
    exit: bool,
    x: f64,
    y: f64,
    ball: Circle,
    playground: Rect,
    vx: f64,
    vy: f64,
    marker: Marker,
    points: Vec<Position>,
    is_drawing: bool,
}

impl App {
    const fn new() -> Self {
        Self {
            exit: false,
            x: 0.0,
            y: 0.0,
            ball: Circle {
                x: 20.0,
                y: 40.0,
                radius: 10.0,
                color: Color::Yellow,
            },
            playground: Rect::new(10, 10, 200, 100),
            vx: 1.0,
            vy: 1.0,
            marker: Marker::Dot,
            points: vec![],
            is_drawing: false,
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let tick_rate = Duration::from_millis(16);
        let mut last_tick = Instant::now();
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if !event::poll(timeout)? {
                self.on_tick();
                last_tick = Instant::now();
                continue;
            }
            match event::read()? {
                Event::Key(key) => self.handle_key_event(key),
                Event::Mouse(event) => self.handle_mouse_event(event),
                _ => (),
            }
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) {
        if !key.is_press() {
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.exit = true,
            KeyCode::Char('j') | KeyCode::Down => self.y += 1.0,
            KeyCode::Char('k') | KeyCode::Up => self.y -= 1.0,
            KeyCode::Char('l') | KeyCode::Right => self.x += 1.0,
            KeyCode::Char('h') | KeyCode::Left => self.x -= 1.0,
            KeyCode::Enter => self.cycle_marker(),
            _ => {}
        }
    }

    fn handle_mouse_event(&mut self, event: event::MouseEvent) {
        match event.kind {
            MouseEventKind::Down(_) => self.is_drawing = true,
            MouseEventKind::Up(_) => self.is_drawing = false,
            MouseEventKind::Drag(_) => {
                self.points.push(Position::new(event.column, event.row));
            }
            _ => {}
        }
    }

    fn on_tick(&mut self) {
    }

    fn render(&self, frame: &mut Frame) {
    }
}
