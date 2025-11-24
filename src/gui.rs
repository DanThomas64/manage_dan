use crate::{datatypes::Task, escpos};
use color_eyre::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    ExecutableCommand,
};
use itertools::Itertools;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    DefaultTerminal, Frame,
};
use std::io::stdout;

pub fn tui(tasks: Vec<Task>, printer_device: String, web_url: String) -> Result<()> {
    color_eyre::install()?;
    stdout().execute(EnableMouseCapture)?;
    let terminal = ratatui::init();
    let app_result = App::new(tasks, printer_device, web_url).run(terminal);
    ratatui::restore();
    stdout().execute(DisableMouseCapture)?;
    app_result
}

struct App {
    exit: bool,
    tasks: Vec<Task>,
    state: ListState,
    printer_device: String,
    web_url: String,
}

impl App {
    fn new(tasks: Vec<Task>, printer_device: String, web_url: String) -> Self {
        let mut state = ListState::default();
        if !tasks.is_empty() {
            state.select(Some(0));
        }
        Self {
            exit: false,
            tasks,
            state,
            printer_device,
            web_url,
        }
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render(frame))?;
            if let Event::Key(key) = event::read()? {
                self.handle_key_event(key);
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
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            KeyCode::Char('p') | KeyCode::Enter => self.print_selected_task(),
            _ => {}
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.tasks.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tasks.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn print_selected_task(&self) {
        if let Some(selected) = self.state.selected() {
            if let Some(task) = self.tasks.get(selected) {
                if let Err(e) = escpos::print_task(task, &self.printer_device, &self.web_url) {
                    // In a real app, you might want to display this error in the TUI
                    eprintln!("Failed to print task: {}", e);
                }
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let main_layout =
            Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(frame.area());

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .map(|task| ListItem::new(task.title.clone()))
            .collect();

        let tasks_list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Tasks"))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .bg(Color::Gray),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(tasks_list, main_layout[0], &mut self.state);

        let task_detail_text = if let Some(selected) = self.state.selected() {
            if let Some(task) = self.tasks.get(selected) {
                let labels = task
                    .labels
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|l| l.title.clone())
                    .join(", ");
                vec![
                    Line::from(vec!["Title: ".bold(), task.title.clone().into()]),
                    Line::from(""),
                    Line::from(vec![
                        "Project ID: ".bold(),
                        task.project_id.to_string().into(),
                    ]),
                    Line::from(""),
                    Line::from("Description:".bold()),
                    Line::from(task.description.clone()),
                    Line::from(""),
                    Line::from(vec!["Labels: ".bold(), labels.into()]),
                ]
            } else {
                vec![Line::from("No task selected")]
            }
        } else {
            vec![Line::from("No tasks found")]
        };

        let task_detail = Paragraph::new(task_detail_text)
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .wrap(Wrap { trim: true });

        frame.render_widget(task_detail, main_layout[1]);
    }
}
