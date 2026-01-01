use crate::api::{ApiClient, Status, StatusResponse, TodoItem};
use anyhow::Result;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap, Clear}, // Added Clear
    Terminal,
};
use std::{io::{self, stdout}, time::Duration};
use chrono::{Local, DateTime}; // Import Local and DateTime for timestamp handling

/// Represents the different screens/views the TUI can display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Todo,
    Notes,
    Project,
    Quit,
}

/// Represents the current input mode for the Todo screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoEditMode {
    Normal,
    Adding,
    Editing,
}

/// Represents the current input mode within the floating dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Insert,
}

/// Represents which field is currently focused in the floating input form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoInputFocus {
    Title,
    Description,
    Subtasks, // NEW
    Submit,
}

/// The main application state structure for the TUI.
pub struct App {
    pub current_screen: Screen,
    pub api_client: ApiClient,
    pub status: Option<StatusResponse>,
    pub last_error: Option<String>,
    
    // Todo State
    pub todo_items: Vec<TodoItem>,
    pub todo_list_state: ListState,
    pub todo_edit_mode: TodoEditMode, // Renamed from todo_input_mode
    
    // Floating Input Form State
    pub input_mode: InputMode, // NEW: Input mode for the dialog
    pub todo_input_focus: TodoInputFocus,
    pub editing_item_id: Option<i64>, // None for adding, Some(id) for editing
    pub title_buffer: String,
    pub description_buffer: String,
    pub subtasks_buffer: String, // NEW
    pub subtasks_scroll: u16, // NEW: Vertical scroll offset for subtasks input
}

impl App {
    pub fn new(api_client: ApiClient) -> Self {
        App {
            current_screen: Screen::Dashboard,
            api_client,
            status: None,
            last_error: None,
            todo_items: Vec::new(),
            todo_list_state: ListState::default(),
            todo_edit_mode: TodoEditMode::Normal,
            
            input_mode: InputMode::Normal, // Initialize in Normal mode
            todo_input_focus: TodoInputFocus::Title,
            editing_item_id: None,
            title_buffer: String::new(),
            description_buffer: String::new(),
            subtasks_buffer: String::new(), // NEW
            subtasks_scroll: 0, // NEW
        }
    }

    /// Fetches the latest status from the main application server.
    pub async fn update_status(&mut self) {
        match self.api_client.fetch_status().await {
            Ok(status) => {
                self.status = Some(status);
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("API Error: {}", e));
            }
        }
    }
    
    /// Fetches the latest todo items.
    pub async fn fetch_todos(&mut self) {
        match self.api_client.fetch_todos().await {
            Ok(items) => {
                self.todo_items = items;
                if !self.todo_items.is_empty() {
                    // Ensure selection stays within bounds or defaults to 0
                    let current_selection = self.todo_list_state.selected().unwrap_or(0);
                    if current_selection >= self.todo_items.len() {
                        self.todo_list_state.select(Some(self.todo_items.len().saturating_sub(1)));
                    } else {
                        self.todo_list_state.select(Some(current_selection));
                    }
                } else {
                    self.todo_list_state.select(None);
                }
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("Failed to fetch todos: {}", e));
            }
        }
    }

    /// Handles input events specific to the current screen.
    pub async fn handle_input(&mut self, event: CEvent) {
        let previous_screen = self.current_screen;
        
        match self.current_screen {
            Screen::Dashboard => {
                if let CEvent::Key(key) = event {
                    self.handle_dashboard_input(key.code);
                }
            }
            Screen::Todo => self.handle_todo_input(event).await,
            _ => {} // Implement specific screen handlers later
        }
        
        // If we switched to the Todo screen, fetch data immediately
        if previous_screen != Screen::Todo && self.current_screen == Screen::Todo {
            self.fetch_todos().await;
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
    
    async fn handle_todo_input(&mut self, event: CEvent) {
        match self.todo_edit_mode {
            TodoEditMode::Normal => {
                if let CEvent::Key(key) = event {
                    match key.code {
                        KeyCode::Char('q') => self.current_screen = Screen::Dashboard,
                        KeyCode::Char('r') => self.fetch_todos().await,
                        KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1), // Added 'k'
                        KeyCode::Down | KeyCode::Char('j') => self.move_selection(1), // Added 'j'
                        KeyCode::Char('c') => self.toggle_completed().await,
                        KeyCode::Char('a') => self.start_add_mode(),
                        KeyCode::Char('e') => self.start_edit_mode(), // New: Start editing
                        KeyCode::Char('p') => self.print_selected().await, // New: Selective print
                        KeyCode::Char('x') => self.archive_selected().await, // NEW: Archive selected item
                        // KeyCode::Char('d') => self.delete_selected().await, // Future: Delete item
                        _ => {}
                    }
                }
            }
            TodoEditMode::Adding | TodoEditMode::Editing => {
                if let CEvent::Key(key) = event {
                    match self.input_mode {
                        InputMode::Normal => {
                            match key.code {
                                KeyCode::Esc => self.cancel_edit_mode(),
                                KeyCode::Up | KeyCode::Char('k') => self.move_focus(-1),
                                KeyCode::Down | KeyCode::Char('j') => self.move_focus(1),
                                KeyCode::Char('i') | KeyCode::Enter => {
                                    if self.todo_input_focus != TodoInputFocus::Submit {
                                        self.input_mode = InputMode::Insert;
                                    } else {
                                        // Enter on Submit button
                                        self.submit_item().await;
                                    }
                                }
                                _ => {}
                            }
                        }
                        InputMode::Insert => {
                            // Check for Ctrl+C (Exit Insert Mode)
                            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                self.input_mode = InputMode::Normal;
                                return;
                            }
                            
                            match key.code {
                                KeyCode::Esc => {
                                    self.input_mode = InputMode::Normal;
                                }
                                KeyCode::Enter => {
                                    if self.todo_input_focus == TodoInputFocus::Subtasks {
                                        // Enter inserts a newline in Subtasks field
                                        self.handle_text_input(KeyCode::Enter, key.modifiers);
                                    }
                                    // Otherwise, Enter does nothing in Insert mode (user must Esc then Enter on Submit)
                                }
                                KeyCode::Backspace => self.handle_text_input(key.code, key.modifiers),
                                KeyCode::Char(_) => self.handle_text_input(key.code, key.modifiers),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    
    fn move_focus(&mut self, delta: i32) {
        self.todo_input_focus = match (self.todo_input_focus, delta) {
            (TodoInputFocus::Title, 1) => TodoInputFocus::Description,
            (TodoInputFocus::Description, 1) => TodoInputFocus::Subtasks, // NEW
            (TodoInputFocus::Subtasks, 1) => TodoInputFocus::Submit, // NEW
            (TodoInputFocus::Submit, 1) => TodoInputFocus::Title,
            
            (TodoInputFocus::Title, -1) => TodoInputFocus::Submit,
            (TodoInputFocus::Description, -1) => TodoInputFocus::Title,
            (TodoInputFocus::Subtasks, -1) => TodoInputFocus::Description, // NEW
            (TodoInputFocus::Submit, -1) => TodoInputFocus::Subtasks, // NEW
            
            (f, _) => f, // Should not happen with delta 1 or -1
        };
        
        // Reset scroll when changing focus
        self.subtasks_scroll = 0;
    }

    fn handle_text_input(&mut self, key_code: KeyCode, modifiers: KeyModifiers) {
        let buffer = match self.todo_input_focus {
            TodoInputFocus::Title => &mut self.title_buffer,
            TodoInputFocus::Description => &mut self.description_buffer,
            TodoInputFocus::Subtasks => &mut self.subtasks_buffer, // NEW
            _ => return, // Not focused on a text field
        };

        match key_code {
            KeyCode::Backspace => {
                buffer.pop();
            }
            KeyCode::Char(c) => {
                // Only allow standard characters unless Ctrl/Alt/Meta modifiers are present
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT {
                    buffer.push(c);
                }
            }
            KeyCode::Enter => {
                // Handle Enter for newline insertion in Subtasks field (since we are in Insert mode)
                if self.todo_input_focus == TodoInputFocus::Subtasks {
                    buffer.push('\n');
                }
            }
            _ => {}
        }
        
        // If we are editing subtasks, we need to adjust the scroll offset if the cursor moved.
        if self.todo_input_focus == TodoInputFocus::Subtasks {
            // For simplicity in this handler, we only reset scroll if the buffer is empty.
            if buffer.is_empty() {
                self.subtasks_scroll = 0;
            }
        }
    }

    fn start_add_mode(&mut self) {
        self.todo_edit_mode = TodoEditMode::Adding;
        self.input_mode = InputMode::Normal; // Start in Normal mode
        self.editing_item_id = None;
        self.title_buffer.clear();
        self.description_buffer.clear();
        self.subtasks_buffer.clear(); // NEW
        self.subtasks_scroll = 0; // Reset scroll
        self.todo_input_focus = TodoInputFocus::Title;
        self.last_error = None;
    }
    
    fn start_edit_mode(&mut self) {
        if let Some(index) = self.todo_list_state.selected() {
            if let Some(item) = self.todo_items.get(index) {
                self.todo_edit_mode = TodoEditMode::Editing;
                self.input_mode = InputMode::Normal; // Start in Normal mode
                self.editing_item_id = item.id;
                self.title_buffer = item.title.clone();
                self.description_buffer = item.description.clone(); // Now required String
                self.subtasks_buffer = item.subtasks.clone().unwrap_or_default(); // NEW
                self.subtasks_scroll = 0; // Reset scroll on edit start
                self.todo_input_focus = TodoInputFocus::Title;
                self.last_error = None;
            }
        }
    }
    
    fn cancel_edit_mode(&mut self) {
        self.todo_edit_mode = TodoEditMode::Normal;
        self.input_mode = InputMode::Normal; // Reset mode
        self.editing_item_id = None;
        self.title_buffer.clear();
        self.description_buffer.clear();
        self.subtasks_buffer.clear(); // NEW
        self.subtasks_scroll = 0; // Reset scroll
    }
    
    async fn submit_item(&mut self) {
        let title = self.title_buffer.trim().to_string();
        let description = self.description_buffer.trim().to_string();
        let subtasks = self.subtasks_buffer.trim().to_string(); // NEW
        
        if title.is_empty() {
            self.last_error = Some("Todo title cannot be empty.".to_string());
            return;
        }
        
        // Description is now required
        if description.is_empty() {
            self.last_error = Some("Todo description cannot be empty.".to_string());
            return;
        }
        
        let subtasks_opt = if subtasks.is_empty() { None } else { Some(subtasks) }; // NEW
        
        let result = match self.todo_edit_mode {
            TodoEditMode::Adding => {
                // When adding, we rely on the API/DB to set created_at/updated_at
                let mut new_item = TodoItem::new(title, description); // Description is required
                new_item.subtasks = subtasks_opt; // NEW
                self.api_client.create_todo(new_item).await.map(|_| ()) // Map created item to ()
            }
            TodoEditMode::Editing => {
                let id = self.editing_item_id.expect("Editing mode requires an ID");
                
                // Find the existing item to preserve created_at and completed status/timestamp
                let existing_item = self.todo_items.iter().find(|i| i.id == Some(id)).cloned();
                
                if let Some(existing) = existing_item {
                    let updated_item = TodoItem {
                        id: Some(id),
                        title,
                        description, // Required
                        completed: existing.completed,
                        created_at: existing.created_at, // Preserve creation time
                        updated_at: Local::now(), // Will be overwritten by API/DB, but required for struct
                        completed_at: existing.completed_at, // Preserve completion time
                        printed_at: existing.printed_at, // Preserve printing time (FIX E0063)
                        subtasks: subtasks_opt, // NEW
                        archived: existing.archived, // Preserve archived status
                    };
                    self.api_client.update_todo(updated_item).await
                } else {
                    self.last_error = Some(format!("Cannot find item ID {} for editing.", id));
                    return;
                }
            }
            _ => return,
        };
        
        match result {
            Ok(_) => {
                self.cancel_edit_mode();
                self.fetch_todos().await; // Refresh list
            }
            Err(e) => {
                self.last_error = Some(format!("Failed to save item: {}", e));
            }
        }
    }

    fn move_selection(&mut self, delta: i32) {
        if self.todo_items.is_empty() {
            self.todo_list_state.select(None);
            return;
        }
        
        let current_index = self.todo_list_state.selected().unwrap_or(0);
        let new_index = (current_index as i32 + delta)
            .rem_euclid(self.todo_items.len() as i32) as usize;
        self.todo_list_state.select(Some(new_index));
    }
    
    async fn toggle_completed(&mut self) {
        if let Some(index) = self.todo_list_state.selected() {
            if let Some(item) = self.todo_items.get_mut(index) {
                item.completed = !item.completed;
                
                // Update timestamps locally before sending, although the API/DB will finalize updated_at
                if item.completed {
                    item.completed_at = Some(Local::now());
                } else {
                    item.completed_at = None;
                }
                item.updated_at = Local::now();
                
                let updated_item = item.clone();
                
                if let Err(e) = self.api_client.update_todo(updated_item).await {
                    self.last_error = Some(format!("Failed to toggle completion: {}", e));
                    // Revert local change if API fails
                    item.completed = !item.completed;
                    item.completed_at = if item.completed { Some(Local::now()) } else { None }; // Revert logic is complex, better to just refetch
                    self.fetch_todos().await; // Force refresh to sync state
                } else {
                    self.last_error = None;
                    // Successful update, list will be refreshed on next poll/input, but we can rely on the local change for immediate display
                }
            }
        }
    }
    
    async fn print_selected(&mut self) {
        if let Some(index) = self.todo_list_state.selected() {
            if let Some(item) = self.todo_items.get(index) {
                if let Some(id) = item.id {
                    self.last_error = Some(format!("Attempting manual print for ID {}...", id));
                    match self.api_client.print_todo(id).await {
                        Ok(_) => {
                            self.last_error = Some(format!("Print job sent for ID {}.", id));
                            self.fetch_todos().await; // Refresh to show updated printed_at timestamp
                        }
                        Err(e) => {
                            self.last_error = Some(format!("Failed to send print job for ID {}: {}", id, e));
                        }
                    }
                } else {
                    self.last_error = Some("Cannot print unsaved item.".to_string());
                }
            }
        } else {
            self.last_error = Some("No item selected to print.".to_string());
        }
    }
    
    async fn archive_selected(&mut self) {
        if let Some(index) = self.todo_list_state.selected() {
            if let Some(item) = self.todo_items.get(index) {
                if let Some(id) = item.id {
                    self.last_error = Some(format!("Attempting to archive ID {}...", id));
                    match self.api_client.archive_todo(id).await {
                        Ok(_) => {
                            self.last_error = Some(format!("Item ID {} archived successfully.", id));
                            // Refresh list to remove the archived item
                            self.fetch_todos().await; 
                        }
                        Err(e) => {
                            self.last_error = Some(format!("Failed to archive item ID {}: {}", id, e));
                        }
                    }
                } else {
                    self.last_error = Some("Cannot archive unsaved item.".to_string());
                }
            }
        } else {
            self.last_error = Some("No item selected to archive.".to_string());
        }
    }
}

// ... Tui struct and impl Tui remains the same ...

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
                Screen::Dashboard => draw_dashboard(frame, app, area),
                Screen::Todo => draw_todo_screen(frame, app, area),
                _ => {}
            }
        })?;
        Ok(())
    }
}

// --- Drawing Helper Functions ---

// ... draw_dashboard remains the same ...

fn draw_dashboard(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let overall_status = app.status.as_ref().map(|s| s.overall.gono).unwrap_or(Status::Unknown);
    let systems_status = app.status.as_ref().map(|s| s.systems);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    // Header
    let header_text = format!("Dashboard | Overall Status: {:?}", overall_status);
    let header = Paragraph::new(header_text)
        .block(Block::default().borders(Borders::ALL).title("System Status"));
    frame.render_widget(header, chunks[0]);

    // System Status List
    let status_lines: Vec<Line> = if let Some(systems) = systems_status {
        // Manually list the fields since the iterator trait is not shared across crates
        let status_to_line = |name: &'static str, status: Status| {
            let style = match status {
                Status::Go => Style::default().fg(Color::Green),
                Status::Degraded => Style::default().fg(Color::Yellow),
                Status::Nogo => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::White),
            };
            Line::from(format!("{:<10}: {:?}", name, status)).style(style)
        };

        vec![
            status_to_line("db", systems.db),
            status_to_line("log", systems.log),
            status_to_line("notes", systems.notes),
            status_to_line("project", systems.project),
            status_to_line("printer", systems.printer),
            status_to_line("todo", systems.todo),
        ]
    } else {
        vec![Line::from(app.last_error.as_deref().unwrap_or("Waiting for API status...").fg(Color::Red))]
    };

    let status_block = Block::default().borders(Borders::ALL).title("Subsystem Health");
    let status_paragraph = Paragraph::new(status_lines).block(status_block);
    frame.render_widget(status_paragraph, chunks[1]);

    // Footer/Menu
    let footer_text = "Q: Quit | 1: Todo | 2: Notes | 3: Project | R: Refresh";
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Cyan));
    frame.render_widget(footer, chunks[2]);
}


fn draw_todo_screen(frame: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    // 1. Define main layout: List (50%), Details (50%), Footer (1 line)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50), // Top half: List
            Constraint::Min(0),         // Bottom half: Details
            Constraint::Length(1),      // Footer
        ])
        .split(area);

    let (list_area, details_area, footer_area) = (main_chunks[0], main_chunks[1], main_chunks[2]);

    // --- 1. Todo List Rendering ---
    let items: Vec<ListItem> = app.todo_items.iter().map(|item| {
        let status = if item.completed { "[X]" } else { "[ ]" };
        
        let created_str = item.created_at.format("%Y-%m-%d %H:%M").to_string();
        let updated_str = item.updated_at.format("%Y-%m-%d %H:%M").to_string();
        // Fix E0282: Explicitly type the closure parameter dt
        let completed_str = item.completed_at.map(|dt: DateTime<Local>| dt.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
        
        let metadata = format!(
            " (Created: {} | Updated: {} | Completed: {})",
            created_str, updated_str, completed_str
        );
        
        let content = format!("{:<3} {:<50} {}", status, item.title, metadata);
        
        let style = if item.completed { 
            Style::default().fg(Color::DarkGray).add_modifier(ratatui::style::Modifier::CROSSED_OUT) 
        } else { 
            Style::default().fg(Color::White) 
        };
        
        ListItem::new(content).style(style)
    }).collect();

    let list_title = match app.todo_edit_mode {
        TodoEditMode::Normal => "TODO List (A: Add, E: Edit, C: Toggle Complete | P: Print | X: Archive | J/K: Navigate)",
        TodoEditMode::Adding => "TODO List (Adding Item)",
        TodoEditMode::Editing => "TODO List (Editing Item)",
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::Black));

    frame.render_stateful_widget(list, list_area, &mut app.todo_list_state);

    // --- 2. Details Panel Rendering ---
    
    let details_block = Block::default().borders(Borders::ALL).title("Item Details");
    
    if let Some(index) = app.todo_list_state.selected() {
        if let Some(item) = app.todo_items.get(index) {
            let mut text_lines = vec![
                Line::from(format!("ID: {}", item.id.unwrap_or(0))),
                Line::from(format!("Title: {}", item.title)).bold(),
                Line::from(format!("Status: {}", if item.completed { "COMPLETED" } else { "PENDING" })),
                Line::from(""),
            ];
            
            // Description (now required)
            text_lines.push(Line::from("Description:").underlined());
            text_lines.extend(Text::from(item.description.as_str()).lines.into_iter().map(|l| Line::from(format!("  {}", l))));
            text_lines.push(Line::from(""));

            // Subtasks (NEW)
            if let Some(subtasks) = &item.subtasks {
                text_lines.push(Line::from("Subtasks/Steps:").underlined());
                text_lines.extend(Text::from(subtasks.as_str()).lines.into_iter().map(|l| Line::from(format!("  {}", l))));
                text_lines.push(Line::from(""));
            }
            
            text_lines.push(Line::from(format!("Created At: {}", item.created_at.format("%Y-%m-%d %H:%M:%S"))));
            text_lines.push(Line::from(format!("Updated At: {}", item.updated_at.format("%Y-%m-%d %H:%M:%S"))));
            
            if let Some(completed_at) = item.completed_at {
                text_lines.push(Line::from(format!("Completed At: {}", completed_at.format("%Y-%m-%d %H:%M:%S"))).fg(Color::Green));
            }
            
            if let Some(printed_at) = item.printed_at {
                text_lines.push(Line::from(format!("Last Printed: {}", printed_at.format("%Y-%m-%d %H:%M:%S"))).fg(Color::Cyan));
            } else {
                text_lines.push(Line::from("Last Printed: Never").fg(Color::Yellow));
            }
            
            let details_paragraph = Paragraph::new(text_lines)
                .block(details_block)
                .wrap(Wrap { trim: true });
            
            frame.render_widget(details_paragraph, details_area);
        } else {
            // Should not happen if index is valid
            frame.render_widget(Paragraph::new("Error: Selected item not found.").block(details_block), details_area);
        }
    } else {
        frame.render_widget(Paragraph::new("Select an item to view details.").block(details_block), details_area);
    }


    // --- 3. Footer/Menu ---
    let footer_text = "Q: Back | R: Refresh | C: Toggle Complete | A: Add New | E: Edit Selected | P: Print Ticket | X: Archive";
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Cyan));
    frame.render_widget(footer, footer_area);
    
    // 4. Floating Input Form (if in Adding or Editing mode)
    if app.todo_edit_mode != TodoEditMode::Normal {
        draw_floating_input(frame, app, area);
    }
    
    // 5. Display error if present
    if let Some(err) = &app.last_error {
        let error_paragraph = Paragraph::new(err.as_str()).fg(Color::Red).block(Block::default().borders(Borders::ALL).title("Error"));
        // Render error in a small area at the bottom right
        let error_area = Rect::new(area.width.saturating_sub(40), area.height.saturating_sub(3), 40, 3);
        frame.render_widget(error_paragraph, error_area);
    }
}

fn draw_floating_input(frame: &mut ratatui::Frame, app: &mut App, parent_area: Rect) {
    // Calculate the size and position of the floating window
    let width = parent_area.width.min(80);
    let height = 14; // Increased height to accommodate Subtasks field
    let x = (parent_area.width.saturating_sub(width)) / 2;
    let y = (parent_area.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);

    let mode_indicator = match app.input_mode {
        InputMode::Normal => "NORMAL (i/Enter: Insert | j/k: Navigate)",
        InputMode::Insert => "INSERT (Esc/Ctrl+C: Normal | Enter: Newline in Subtasks)",
    };

    let title = match app.todo_edit_mode {
        TodoEditMode::Adding => format!("Add New Todo Item | {}", mode_indicator),
        TodoEditMode::Editing => format!("Edit Todo Item | {}", mode_indicator),
        _ => unreachable!(),
    };

    // 1. Clear the area behind the floating widget
    frame.render_widget(Clear, area); 

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::DarkGray)); // Background set here
    
    // Split the inner area for Title, Description, Subtasks, and Submit button
    let inner_area = block.inner(area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Title label + input
            Constraint::Length(2), // Description label + input
            Constraint::Length(4), // Subtasks label + input (multi-line area height is 3 lines + 1 line label)
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // Submit button
        ])
        .split(inner_area);

    // Helper to determine if a field is currently active (focused AND in Normal mode, or in Insert mode)
    let is_active = |focus: TodoInputFocus| {
        app.todo_input_focus == focus && (app.input_mode == InputMode::Normal || focus != TodoInputFocus::Submit)
    };
    
    // --- Title Input ---
    let title_style = if is_active(TodoInputFocus::Title) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    let title_label = Paragraph::new("Title:").style(title_style);
    frame.render_widget(title_label, chunks[0]);
    
    let title_input_area = Rect::new(chunks[0].x + 7, chunks[0].y, chunks[0].width.saturating_sub(7), 1);
    let title_input = Paragraph::new(app.title_buffer.as_str()).style(title_style);
    frame.render_widget(title_input, title_input_area);

    // --- Description Input (Now required) ---
    let desc_style = if is_active(TodoInputFocus::Description) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    let desc_label = Paragraph::new("Description:").style(desc_style);
    frame.render_widget(desc_label, chunks[1]);
    
    let desc_input_area = Rect::new(chunks[1].x + 13, chunks[1].y, chunks[1].width.saturating_sub(13), 1);
    let desc_input = Paragraph::new(app.description_buffer.as_str()).style(desc_style);
    frame.render_widget(desc_input, desc_input_area);

    // --- Subtasks Input (NEW) ---
    let subtasks_style = if is_active(TodoInputFocus::Subtasks) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    let subtasks_label = Paragraph::new("Subtasks/Steps:").style(subtasks_style);
    frame.render_widget(subtasks_label, chunks[2]);
    
    let subtasks_input_area = Rect::new(chunks[2].x, chunks[2].y + 1, chunks[2].width, 3);
    
    // Calculate cursor position and scroll offset for Subtasks
    let line_width = subtasks_input_area.width as usize;
    let cursor_pos = app.subtasks_buffer.len();
    
    // Calculate the line index of the cursor, considering wrapping
    let mut current_line_index = 0;
    let mut current_col_index = 0;
    
    for (i, c) in app.subtasks_buffer.chars().enumerate() {
        if i == cursor_pos {
            break;
        }
        if c == '\n' {
            current_line_index += 1;
            current_col_index = 0;
        } else {
            current_col_index += 1;
            if current_col_index >= line_width {
                current_line_index += 1;
                current_col_index = 0;
            }
        }
    }
    
    let cursor_line = current_line_index as u16;
    let cursor_col = current_col_index as u16;
    
    let viewport_height = subtasks_input_area.height; // 3 lines
    
    // Adjust scroll offset (app.subtasks_scroll)
    if cursor_line >= app.subtasks_scroll + viewport_height {
        // Cursor moved below the viewport
        app.subtasks_scroll = cursor_line.saturating_sub(viewport_height) + 1;
    } else if cursor_line < app.subtasks_scroll {
        // Cursor moved above the viewport
        app.subtasks_scroll = cursor_line;
    }
    
    let scroll_offset = app.subtasks_scroll;

    let subtasks_input = Paragraph::new(Text::from(app.subtasks_buffer.as_str()))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0)) // Apply vertical scroll
        .block(Block::default().borders(Borders::NONE));
        
    frame.render_widget(subtasks_input, subtasks_input_area);


    // --- Submit Button ---
    let submit_style = if is_active(TodoInputFocus::Submit) {
        // Change highlighting to blue background, black foreground
        Style::default().fg(Color::Black).bg(Color::Blue)
    } else {
        // Keep green foreground when not focused
        Style::default().fg(Color::Green)
    };
    
    let submit_text = match app.todo_edit_mode {
        TodoEditMode::Adding => " [ SUBMIT ] ",
        TodoEditMode::Editing => " [ SAVE ] ",
        _ => unreachable!(),
    };
    
    let submit_paragraph = Paragraph::new(submit_text)
        .style(submit_style)
        .alignment(ratatui::layout::Alignment::Center);
    
    frame.render_widget(submit_paragraph, chunks[4]); // Index 4 now

    // Render the main block border last to ensure it overlays everything else
    frame.render_widget(block, area);

    // Set cursor position based on focus
    if app.todo_edit_mode != TodoEditMode::Normal && app.input_mode == InputMode::Insert {
        match app.todo_input_focus {
            TodoInputFocus::Title => {
                frame.set_cursor(
                    title_input_area.x + app.title_buffer.len() as u16,
                    title_input_area.y,
                );
            }
            TodoInputFocus::Description => {
                frame.set_cursor(
                    desc_input_area.x + app.description_buffer.len() as u16,
                    desc_input_area.y,
                );
            }
            TodoInputFocus::Subtasks => { // NEW
                // Cursor position relative to the screen area
                let final_cursor_y = subtasks_input_area.y + cursor_line.saturating_sub(scroll_offset);
                let final_cursor_x = subtasks_input_area.x + cursor_col;

                frame.set_cursor(
                    final_cursor_x,
                    final_cursor_y,
                );
            }
            _ => {} // Cursor hidden for Submit focus
        }
    }
}

// Placeholder for other screens
fn draw_todo_placeholder(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let text = Paragraph::new("TODO Screen (Press Q to quit TUI)").block(Block::default().borders(Borders::ALL).title("TODO List"));
    frame.render_widget(text, area);
}

// ---------------------------------------------------------------------------------

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

/// Runs the main TUI loop.
pub async fn run_tui() -> Result<()> {
    let mut tui = Tui::new()?;
    let api_client = ApiClient::new("http://127.0.0.1:8080");
    let mut app = App::new(api_client);

    // Initial status fetch
    app.update_status().await;

    loop {
        // Draw the UI
        tui.draw(&mut app)?;

        // Handle input events
        if event::poll(Duration::from_millis(250))? {
            // Pass the raw event to handle_input
            app.handle_input(event::read()?).await;
        }
        
        // Check for quit signal
        if app.current_screen == Screen::Quit {
            break;
        }
    }

    Ok(())
}
