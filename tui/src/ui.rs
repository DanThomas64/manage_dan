use crate::api::{ApiClient, Status, StatusResponse, TodoItem, Subtask, LogEntry};
use anyhow::Result;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    text::Text,
    widgets::{Block, Borders, ListState, Paragraph, Wrap, Clear},
    Terminal,
};
use std::{io::{self, stdout}, time::Duration};
use chrono::{Local, NaiveDate, NaiveTime, Datelike, Duration as ChronoDuration, Months, Weekday}; // Expanded chrono imports for date manipulation
use ratatui::text::Line;
use ratatui::widgets::{List, ListItem};
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen}; // Re-added necessary crossterm imports

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
    Subtasks,
    DueBy,          // NEW: Toggle Due Date/Time usage
    CalendarDate,   // Focus on the calendar grid
    CalendarTime,   // Focus on the time input buffer
    Priority,
    Submit,
}

/// The main application state structure for the TUI.
pub struct App {
    pub current_screen: Screen,
    pub api_client: ApiClient,
    pub status: Option<StatusResponse>,
    pub last_error: Option<String>,
    
    // Dashboard State
    pub latest_logs: Vec<LogEntry>, // NEW: Store latest log entries
    
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
    pub subtasks_buffer: String,
    pub subtasks_scroll: u16, // Vertical scroll offset for subtasks input

    // NEW: Input buffers and toggles for new fields
    pub due_by_toggle: bool, // NEW: Whether a due date/time is set/intended
    pub calendar_date: NaiveDate, // Selected date in the picker
    pub time_buffer: String, // Time input (HH:MM)
    pub priority_buffer: String,

    // Removed: pub todo_summary: Option<TodoSummary>,
}

/// Parses the multiline subtasks input buffer into a `Vec<Subtask>`.
///
/// Each non-empty line becomes one subtask. A line starting with `[x] ` is
/// treated as done; `[ ] ` or plain text is treated as not done.
fn parse_subtasks_buffer(buffer: &str) -> Vec<Subtask> {
    buffer
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some(title) = trimmed.strip_prefix("[x] ") {
                Some(Subtask { id: None, title: title.to_string(), done: true })
            } else if let Some(title) = trimmed.strip_prefix("[ ] ") {
                Some(Subtask { id: None, title: title.to_string(), done: false })
            } else {
                Some(Subtask { id: None, title: trimmed.to_string(), done: false })
            }
        })
        .collect()
}

impl App {
    pub fn new(api_client: ApiClient) -> Self {
        let now = Local::now().date_naive();
        
        App {
            current_screen: Screen::Dashboard,
            api_client,
            status: None,
            last_error: None,
            latest_logs: Vec::new(), // Initialize logs
            todo_items: Vec::new(),
            todo_list_state: ListState::default(),
            todo_edit_mode: TodoEditMode::Normal,
            
            input_mode: InputMode::Normal, // Initialize in Normal mode
            todo_input_focus: TodoInputFocus::Title,
            editing_item_id: None,
            title_buffer: String::new(),
            description_buffer: String::new(),
            subtasks_buffer: String::new(),
            subtasks_scroll: 0,

            due_by_toggle: false, // Default to no due date
            calendar_date: now,
            time_buffer: String::from("00:00"),
            priority_buffer: String::new(),
            // Removed: todo_summary: None,
        }
    }

    /// Fetches the latest system status and log entries.
    pub async fn update_system_status_and_logs(&mut self) {
        // 1. Fetch System Status
        match self.api_client.fetch_status().await {
            Ok(status) => {
                self.status = Some(status);
                self.last_error = None;
            }
            Err(e) => {
                self.last_error = Some(format!("API Error: {}", e));
            }
        }
        
        // 2. Fetch Latest Logs (Limit to 10 for dashboard)
        match self.api_client.fetch_logs(10).await {
            Ok(logs) => {
                self.latest_logs = logs;
            }
            Err(e) => {
                // Log error but don't overwrite status error if it exists
                if self.last_error.is_none() {
                    self.last_error = Some(format!("Failed to fetch logs: {}", e));
                }
            }
        }
    }
    
    /// Fetches data specifically required for the dashboard (currently just todo items for panels).
    pub async fn update_dashboard_data(&mut self) {
        self.fetch_todos().await;
    }
    
    /// Fetches the latest todo items.
    pub async fn fetch_todos(&mut self) {
        match self.api_client.fetch_todos().await {
            Ok(items) => {
                // Filter out archived items for display purposes
                self.todo_items = items.into_iter().filter(|item| !item.archived).collect();
                
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
        let mut action_taken = false;
        
        match self.current_screen {
            Screen::Dashboard => {
                if let CEvent::Key(key) = event {
                    self.handle_dashboard_input(key.code);
                    action_taken = true; // Refresh status after any key press on dashboard
                }
            }
            Screen::Todo => {
                match self.todo_edit_mode {
                    TodoEditMode::Normal => {
                        if let CEvent::Key(key) = event {
                            match key.code {
                                KeyCode::Char('q') => self.current_screen = Screen::Dashboard,
                                KeyCode::Char('r') => { self.fetch_todos().await; action_taken = true; }
                                KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
                                KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
                                KeyCode::Char('c') => { self.toggle_completed().await; action_taken = true; }
                                KeyCode::Char('a') => self.start_add_mode(),
                                KeyCode::Char('e') => self.start_edit_mode(),
                                KeyCode::Char('p') => { self.print_selected().await; action_taken = true; }
                                KeyCode::Char('x') => { self.archive_selected().await; action_taken = true; }
                                _ => {}
                            }
                        }
                    }
                    TodoEditMode::Adding | TodoEditMode::Editing => {
                        // Handle input for floating form
                        let previous_mode = self.todo_edit_mode;
                        self.handle_todo_input_form(event).await;
                        
                        // If we exited edit mode (via submit or cancel), an action was taken
                        if self.todo_edit_mode == TodoEditMode::Normal && previous_mode != TodoEditMode::Normal {
                            action_taken = true;
                        }
                    }
                }
            }
            _ => {} // Other screens not yet implemented
        }
        
        // 1. Handle screen transition logic
        if previous_screen != self.current_screen {
            match self.current_screen {
                Screen::Dashboard => {
                    // When entering dashboard, fetch dashboard data immediately
                    self.update_dashboard_data().await;
                }
                Screen::Todo => {
                    // When entering todo screen, fetch the list immediately
                    self.fetch_todos().await;
                }
                _ => {}
            }
        }
        
        // 2. Handle periodic/action-based updates
        if self.current_screen == Screen::Dashboard {
            // Dashboard needs system status and logs frequently, and todo items (via update_dashboard_data)
            self.update_system_status_and_logs().await;
            // Note: update_dashboard_data is called on transition, and implicitly via the main loop's periodic call
        } else if action_taken && self.current_screen == Screen::Todo {
            // If an action was taken on the Todo screen, refresh the list and system status/logs
            self.fetch_todos().await;
            self.update_system_status_and_logs().await;
        } else if action_taken && self.current_screen == Screen::Dashboard {
            // If an action was taken on the Dashboard (e.g., refresh 'r'), update everything
            self.update_system_status_and_logs().await;
            self.update_dashboard_data().await;
        }
    }

    fn handle_dashboard_input(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char('q') => self.current_screen = Screen::Quit,
            KeyCode::Char('1') => self.current_screen = Screen::Todo,
            KeyCode::Char('2') => self.current_screen = Screen::Notes,
            KeyCode::Char('3') => self.current_screen = Screen::Project,
            KeyCode::Char('r') => { /* update_status is called automatically */ }
            _ => {}
        }
    }
    
    // Renamed and refactored the input handling for the floating form
    async fn handle_todo_input_form(&mut self, event: CEvent) {
        if let CEvent::Key(key) = event {
            match self.input_mode {
                InputMode::Normal => {
                    match key.code {
                        KeyCode::Esc => self.cancel_edit_mode(),
                        
                        // UP/DOWN/LEFT/RIGHT always move focus in Normal mode
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.move_focus(-1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.move_focus(1);
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            self.move_focus(-1);
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            self.move_focus(1);
                        }
                        
                        // </> only modify month if CalendarDate is focused (still allowed in Normal mode for quick month flip)
                        KeyCode::Char('<') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            // Previous month
                            self.calendar_date = self.calendar_date.with_day(1).unwrap_or(self.calendar_date).checked_sub_months(Months::new(1)).unwrap_or(self.calendar_date);
                        }
                        KeyCode::Char('>') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            // Next month
                            self.calendar_date = self.calendar_date.with_day(1).unwrap_or(self.calendar_date).checked_add_months(Months::new(1)).unwrap_or(self.calendar_date);
                        }
                        
                        KeyCode::Char('i') | KeyCode::Enter => {
                            match self.todo_input_focus {
                                TodoInputFocus::Submit => {
                                    self.submit_item().await;
                                }
                                TodoInputFocus::DueBy => {
                                    // Toggle Due By status
                                    self.due_by_toggle = !self.due_by_toggle;
                                }
                                TodoInputFocus::CalendarDate => {
                                    // ACTIVATE CALENDAR SELECTION MODE (InputMode::Insert)
                                    self.input_mode = InputMode::Insert;
                                }
                                TodoInputFocus::CalendarTime | TodoInputFocus::Priority | TodoInputFocus::Title | TodoInputFocus::Description | TodoInputFocus::Subtasks => {
                                    // Enter/i switches to Insert mode for editable fields
                                    self.input_mode = InputMode::Insert;
                                }
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
                            // If we were editing CalendarDate, move focus to the next field (Time)
                            if self.todo_input_focus == TodoInputFocus::CalendarDate {
                                self.move_focus(1);
                            }
                        }
                        
                        // Date Navigation (only active when CalendarDate is focused AND InputMode::Insert is active)
                        KeyCode::Up | KeyCode::Char('k') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            self.calendar_date = self.calendar_date.checked_sub_signed(ChronoDuration::weeks(1)).unwrap_or(self.calendar_date);
                        }
                        KeyCode::Down | KeyCode::Char('j') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            self.calendar_date = self.calendar_date.checked_add_signed(ChronoDuration::weeks(1)).unwrap_or(self.calendar_date);
                        }
                        KeyCode::Left | KeyCode::Char('h') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            self.calendar_date = self.calendar_date.checked_sub_signed(ChronoDuration::days(1)).unwrap_or(self.calendar_date);
                        }
                        KeyCode::Right | KeyCode::Char('l') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            self.calendar_date = self.calendar_date.checked_add_signed(ChronoDuration::days(1)).unwrap_or(self.calendar_date);
                        }
                        KeyCode::Char('<') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            self.calendar_date = self.calendar_date.with_day(1).unwrap_or(self.calendar_date).checked_sub_months(Months::new(1)).unwrap_or(self.calendar_date);
                        }
                        KeyCode::Char('>') if self.todo_input_focus == TodoInputFocus::CalendarDate => {
                            self.calendar_date = self.calendar_date.with_day(1).unwrap_or(self.calendar_date).checked_add_months(Months::new(1)).unwrap_or(self.calendar_date);
                        }
                        
                        KeyCode::Enter => {
                            if self.todo_input_focus == TodoInputFocus::CalendarDate {
                                // Confirm date selection, exit Insert mode, move focus to Time
                                self.input_mode = InputMode::Normal;
                                self.move_focus(1);
                            } else if self.todo_input_focus == TodoInputFocus::Subtasks || self.todo_input_focus == TodoInputFocus::Description {
                                // Enter inserts a newline in multiline fields
                                self.handle_text_input(KeyCode::Enter, key.modifiers);
                            } else {
                                // For single-line inputs, Enter exits insert mode
                                self.input_mode = InputMode::Normal;
                            }
                        }
                        
                        // Text input handling (only for non-calendar fields)
                        KeyCode::Backspace => self.handle_text_input(key.code, key.modifiers),
                        KeyCode::Char(_) => self.handle_text_input(key.code, key.modifiers),
                        _ => {}
                    }
                }
            }
        }
    }
    
    fn move_focus(&mut self, delta: i32) {
        let current_focus = self.todo_input_focus;
        
        // Define the order of fields
        let fields = [
            TodoInputFocus::Title,
            TodoInputFocus::Description,
            TodoInputFocus::Subtasks,
            TodoInputFocus::DueBy,
            TodoInputFocus::CalendarDate,
            TodoInputFocus::CalendarTime,
            TodoInputFocus::Priority,
            TodoInputFocus::Submit,
        ];
        
        let current_index = fields.iter().position(|&f| f == current_focus).unwrap_or(0);
        let num_fields = fields.len();
        
        let mut new_index = current_index;
        
        // Conditional skipping logic loop
        loop {
            new_index = (new_index as i32 + delta).rem_euclid(num_fields as i32) as usize;
            let next_focus = fields[new_index];
            
            // If DueBy is false, skip CalendarDate and CalendarTime
            if !self.due_by_toggle && (next_focus == TodoInputFocus::CalendarDate || next_focus == TodoInputFocus::CalendarTime) {
                // Continue looping to skip this field
            } else {
                break;
            }
            
            // Safety break if we somehow cycled through all fields without finding a valid one
            if new_index == current_index { break; }
        }
        
        self.todo_input_focus = fields[new_index];
        
        // Reset scroll when changing focus
        self.subtasks_scroll = 0;
    }

    fn handle_text_input(&mut self, key_code: KeyCode, modifiers: KeyModifiers) {
        let buffer = match self.todo_input_focus {
            TodoInputFocus::Title => &mut self.title_buffer,
            TodoInputFocus::Description => &mut self.description_buffer,
            TodoInputFocus::Subtasks => &mut self.subtasks_buffer,
            TodoInputFocus::CalendarTime => &mut self.time_buffer, // NEW
            TodoInputFocus::Priority => &mut self.priority_buffer,
            _ => return, // Not focused on a text field
        };

        match key_code {
            KeyCode::Backspace => {
                if self.todo_input_focus == TodoInputFocus::CalendarTime {
                    // Handle backspace for HH:MM format
                    if buffer.ends_with(':') {
                        buffer.pop();
                    }
                }
                buffer.pop();
            }
            KeyCode::Char(c) => {
                // Basic validation for Priority field
                if self.todo_input_focus == TodoInputFocus::Priority && !c.is_ascii_digit() {
                    return;
                }
                
                // Basic validation for Time field (HH:MM format)
                if self.todo_input_focus == TodoInputFocus::CalendarTime {
                    if buffer.len() >= 5 { return; }
                    if buffer.len() == 2 && c.is_ascii_digit() { buffer.push(':'); }
                    if !c.is_ascii_digit() { return; }
                }
                
                // Only allow standard characters unless Ctrl/Alt/Meta modifiers are present
                if modifiers.is_empty() || modifiers == KeyModifiers::SHIFT {
                    buffer.push(c);
                }
            }
            KeyCode::Enter => {
                // Handle Enter for newline insertion in Subtasks field (since we are in Insert mode)
                if self.todo_input_focus == TodoInputFocus::Subtasks || self.todo_input_focus == TodoInputFocus::Description {
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
        let now = Local::now().date_naive();
        self.todo_edit_mode = TodoEditMode::Adding;
        self.input_mode = InputMode::Normal; // Start in Normal mode
        self.editing_item_id = None;
        self.title_buffer.clear();
        self.description_buffer.clear();
        self.subtasks_buffer.clear();
        
        self.due_by_toggle = false;
        self.calendar_date = now; // Default to today
        self.time_buffer = String::from("00:00");
        self.priority_buffer.clear();
        
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
                self.subtasks_buffer = item.subtasks.iter()
                    .map(|s| format!("{} {}", if s.done { "[x]" } else { "[ ]" }, s.title))
                    .collect::<Vec<_>>()
                    .join("\n");
                
                // NEW: Populate date/time/toggle
                if let Some(dt) = item.due_date {
                    self.due_by_toggle = true;
                    self.calendar_date = dt.date_naive();
                    self.time_buffer = dt.format("%H:%M").to_string();
                } else {
                    self.due_by_toggle = false;
                    let now = Local::now().date_naive();
                    self.calendar_date = now;
                    self.time_buffer = String::from("00:00");
                }
                self.priority_buffer = item.priority.to_string();

                self.subtasks_scroll = 0; // Reset scroll on edit start
                self.todo_input_focus = TodoInputFocus::Title;
                self.last_error = None;
            }
        }
    }
    
    fn cancel_edit_mode(&mut self) {
        let now = Local::now().date_naive();
        self.todo_edit_mode = TodoEditMode::Normal;
        self.input_mode = InputMode::Normal; // Reset mode
        self.editing_item_id = None;
        self.title_buffer.clear();
        self.description_buffer.clear();
        self.subtasks_buffer.clear();
        
        // Reset calendar state
        self.due_by_toggle = false;
        self.calendar_date = now;
        self.time_buffer = String::from("00:00");
        self.priority_buffer.clear();
        
        self.subtasks_scroll = 0; // Reset scroll
    }
    
    async fn submit_item(&mut self) {
        let title = self.title_buffer.trim().to_string();
        let description = self.description_buffer.trim().to_string();
        let subtasks = self.subtasks_buffer.trim().to_string();
        let time_str = self.time_buffer.trim();
        let priority_str = self.priority_buffer.trim();
        
        if title.is_empty() {
            self.last_error = Some("Todo title cannot be empty.".to_string());
            return;
        }
        
        // Description is now required
        if description.is_empty() {
            self.last_error = Some("Todo description cannot be empty.".to_string());
            return;
        }
        
        // Parse Due Date and Time
        let due_date_opt = if !self.due_by_toggle {
            None
        } else if time_str.is_empty() || time_str == "00:00" {
            // If toggle is on but time is default/empty, use midnight of the selected date
            let naive_datetime = self.calendar_date.and_hms_opt(0, 0, 0).unwrap_or_else(|| self.calendar_date.and_time(NaiveTime::MIN));
            match naive_datetime.and_local_timezone(Local) {
                chrono::LocalResult::Single(dt) => Some(dt),
                _ => {
                    self.last_error = Some("Due date is invalid.".to_string());
                    return;
                }
            }
        } else {
            match NaiveTime::parse_from_str(time_str, "%H:%M") {
                Ok(naive_time) => {
                    let naive_datetime = self.calendar_date.and_time(naive_time);
                    
                    // Handle LocalResult ambiguity
                    match naive_datetime.and_local_timezone(Local) {
                        chrono::LocalResult::Single(dt) => Some(dt),
                        chrono::LocalResult::Ambiguous(dt1, _) => {
                            // If ambiguous (e.g., DST change), pick the first one
                            Some(dt1)
                        }
                        chrono::LocalResult::None => {
                            // If time doesn't exist (e.g., skipped by DST), treat as error
                            self.last_error = Some("Due time is invalid or ambiguous (e.g., during DST transition).".to_string());
                            return;
                        }
                    }
                }
                Err(_) => {
                    // This path should ideally not be hit if time_str is validated, but kept for safety
                    self.last_error = Some("Invalid Time format. Use HH:MM.".to_string());
                    return;
                }
            }
        };

        // Parse Priority
        let priority: u8 = if priority_str.is_empty() {
            0
        } else {
            match priority_str.parse::<u8>() {
                Ok(p) if p <= 10 => p,
                _ => {
                    self.last_error = Some("Priority must be an integer between 0 and 10.".to_string());
                    return;
                }
            }
        };
        
        let parsed_subtasks = parse_subtasks_buffer(&subtasks);

        let result = match self.todo_edit_mode {
            TodoEditMode::Adding => {
                let mut new_item = TodoItem::new(title, description);
                new_item.subtasks = parsed_subtasks;
                new_item.due_date = due_date_opt;
                new_item.priority = priority;
                self.api_client.create_todo(new_item).await.map(|_| ())
            }
            TodoEditMode::Editing => {
                let id = self.editing_item_id.expect("Editing mode requires an ID");

                let existing_item = self.todo_items.iter().find(|i| i.id == Some(id)).cloned();

                if let Some(existing) = existing_item {
                    let updated_item = TodoItem {
                        id: Some(id),
                        title,
                        description,
                        completed: existing.completed,
                        created_at: existing.created_at,
                        updated_at: Local::now(),
                        completed_at: existing.completed_at,
                        printed_at: existing.printed_at,
                        subtasks: parsed_subtasks,
                        archived: existing.archived,
                        due_date: due_date_opt,
                        priority,
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
                // Action taken, handle_input will refresh data
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
                    // We rely on the subsequent refresh to sync state
                } else {
                    self.last_error = None;
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
                            // Refresh is handled by action_taken logic
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
                            // Refresh is handled by action_taken logic
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

fn draw_dashboard(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let _overall_status = app.status.as_ref().map(|s| s.overall.gono).unwrap_or(Status::Unknown);
    let systems_status = app.status.as_ref().map(|s| s.systems);
    let _now = Local::now();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let (_header_area, content_area, footer_area) = (chunks[0], chunks[1], chunks[2]);

    // Content Area: Split vertically for lists and status/logs
    let main_content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Min(0)])
        .split(content_area);
        
    let list_row_area = main_content_chunks[0];
    let status_log_area = main_content_chunks[1]; // Area for status and logs

    // List Row: Split horizontally for two lists
    let list_row_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(list_row_area);
        
    let (prioritized_area, no_due_area) = (list_row_chunks[0], list_row_chunks[1]);

    // Status/Log Area: Split horizontally for System Status and Latest Logs
    let status_log_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(status_log_area);
        
    let (status_area, log_area) = (status_log_chunks[0], status_log_chunks[1]);


    // --- 1. Prioritized Todos List ---
    
    let prioritized_items: Vec<ListItem> = {
        let mut items: Vec<&TodoItem> = app.todo_items.iter()
            .filter(|item| !item.completed && item.due_date.is_some())
            .collect();

        items.sort_by(|a, b| {
            // Sort primarily by priority (desc), then by due date (asc/urgency)
            a.priority.cmp(&b.priority).reverse()
                .then_with(|| a.due_date.cmp(&b.due_date))
        });

        items.into_iter()
            .take(5)
            .map(|item| {
                let due_str = item.due_date.map(|dt| dt.format("%m-%d %H:%M").to_string()).unwrap_or_default();
                let content = format!("P{}: {} ({})", item.priority, item.title, due_str);
                let style = if item.priority >= 8 { Style::default().fg(Color::Red) } else { Style::default().fg(Color::Yellow) };
                ListItem::new(content).style(style)
            })
            .collect()
    };

    let prioritized_list = List::new(prioritized_items)
        .block(Block::default().borders(Borders::ALL).title("Prioritized Todos (Top 5)"));
    frame.render_widget(prioritized_list, prioritized_area);


    // --- 2. No Due Date Todos List ---
    
    let no_due_items: Vec<ListItem> = {
        let mut items: Vec<&TodoItem> = app.todo_items.iter()
            .filter(|item| !item.completed && item.due_date.is_none())
            .collect();

        // Order by created_at (ascending, oldest first)
        items.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        items.into_iter()
            .take(5)
            .map(|item| {
                let created_str = item.created_at.format("%m-%d").to_string();
                let content = format!("{} (Created: {})", item.title, created_str);
                ListItem::new(content).style(Style::default().fg(Color::White))
            })
            .collect()
    };

    let no_due_list = List::new(no_due_items)
        .block(Block::default().borders(Borders::ALL).title("No Due Date (Oldest 5)"));
    frame.render_widget(no_due_list, no_due_area);


    // --- 3. System Status List ---
    let status_lines: Vec<Line> = if let Some(systems) = systems_status {
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
    frame.render_widget(status_paragraph, status_area);
    
    
    // --- 4. Latest Log Entries Panel (NEW) ---
    let log_items: Vec<ListItem> = app.latest_logs.iter().map(|log| {
        let style = match log.level.as_str() {
            "ERROR" => Style::default().fg(Color::Red),
            "WARN" => Style::default().fg(Color::Yellow),
            "INFO" => Style::default().fg(Color::Green),
            _ => Style::default().fg(Color::DarkGray),
        };
        
        let content = format!(
            "{} [{}] {}: {}",
            log.timestamp.format("%H:%M:%S"),
            log.level,
            log.target,
            log.message
        );
        ListItem::new(content).style(style)
    }).collect();
    
    let log_list = List::new(log_items)
        .block(Block::default().borders(Borders::ALL).title("Latest Log Entries (DB)"));
    frame.render_widget(log_list, log_area);


    // Footer/Menu
    let footer_text = "Q: Quit | 1: Todo | 2: Notes | 3: Project | R: Refresh";
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Cyan));
    frame.render_widget(footer, footer_area);
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
        let _updated_str = item.updated_at.format("%Y-%m-%d %H:%M").to_string();
        // FIX E0107/E0038: Remove unnecessary type annotation
        let _completed_str = item.completed_at.map(|dt| dt.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default();
        
        let priority_str = if item.priority > 0 {
            format!("P:{}", item.priority)
        } else {
            "".to_string()
        };

        let due_date_str = item.due_date.map(|dt| dt.format("Due: %m-%d").to_string()).unwrap_or_default();
        
        let metadata = format!(
            " ({}, {} | Created: {})",
            priority_str, due_date_str, created_str
        );
        
        let content = format!("{:<3} {:<50} {}", status, item.title, metadata);
        
        let style = if item.completed { 
            Style::default().fg(Color::DarkGray).add_modifier(ratatui::style::Modifier::CROSSED_OUT) 
        } else if item.priority >= 8 {
            Style::default().fg(Color::Red)
        } else if item.priority >= 5 {
            Style::default().fg(Color::Yellow)
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
                Line::from(format!("Priority: {}", item.priority)).fg(if item.priority >= 8 { Color::Red } else { Color::White }),
            ];
            
            if let Some(due_date) = item.due_date {
                text_lines.push(Line::from(format!("Due Date: {}", due_date.format("%Y-%m-%d %H:%M:%S"))).fg(Color::Yellow));
            } else {
                text_lines.push(Line::from("Due Date: None"));
            }
            
            text_lines.push(Line::from(""));
            
            // Description (now required)
            text_lines.push(Line::from("Description:").underlined());
            text_lines.extend(Text::from(item.description.as_str()).lines.into_iter().map(|l| Line::from(format!("  {}", l))));
            text_lines.push(Line::from(""));

            // Subtasks
            if !item.subtasks.is_empty() {
                text_lines.push(Line::from("Subtasks:").underlined());
                for sub in &item.subtasks {
                    let marker = if sub.done { "[x]" } else { "[ ]" };
                    text_lines.push(Line::from(format!("  {} {}", marker, sub.title)));
                }
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


fn draw_calendar_picker(frame: &mut ratatui::Frame, area: Rect, date: NaiveDate, is_focused: bool) {
    
    let today = Local::now().date_naive();
    let display_month = date.with_day(1).unwrap_or(date);
    let month_name = display_month.format("%B %Y").to_string();

    let block = Block::default()
        .title(format!("Date Picker: {}", month_name))
        .borders(Borders::TOP)
        .border_style(if is_focused { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::White) });
    
    let inner_area = block.inner(area);
    frame.render_widget(block, area);

    // Calculate dimensions for the grid (7 columns, 7 rows including header)
    // inner_area.height must be >= 7 for this to work correctly.
    let day_width = inner_area.width / 7;
    let day_height = inner_area.height / 7; 

    // 1. Draw Weekday Headers
    let weekdays = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    
    let header_row_area = Rect::new(inner_area.x, inner_area.y, inner_area.width, day_height);
    let header_areas = Layout::horizontal(weekdays.iter().map(|_| Constraint::Length(day_width)))
        .split(header_row_area);

    for (i, day) in weekdays.iter().enumerate() {
        let style = Style::default().fg(Color::Cyan);
        let paragraph = Paragraph::new(*day).style(style).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, header_areas[i]);
    }

    // 2. Draw Days
    let mut current_date = display_month;
    
    // Find the starting position (Monday of the week containing the 1st of the month)
    let start_weekday = display_month.weekday();
    let offset = match start_weekday {
        Weekday::Mon => 0,
        Weekday::Tue => 1,
        Weekday::Wed => 2,
        Weekday::Thu => 3,
        Weekday::Fri => 4,
        Weekday::Sat => 5,
        Weekday::Sun => 6,
    };
    
    // Move back to the start of the week (or previous month)
    current_date = current_date.checked_sub_signed(ChronoDuration::days(offset)).unwrap_or(current_date);

    let mut row = 1;
    let mut col = 0;

    // Iterate through up to 6 weeks
    for _ in 0..42 { // Max 6 weeks * 7 days
        if row > 6 { break; }

        let day_area = Rect::new(
            inner_area.x + col * day_width,
            inner_area.y + row * day_height,
            day_width,
            day_height,
        );

        let day_num = current_date.day().to_string();
        let mut style = Style::default();

        // Style based on month
        if current_date.month() != display_month.month() {
            style = style.fg(Color::DarkGray);
        } else {
            style = style.fg(Color::White);
        }

        // Highlight today
        if current_date == today {
            // Reverting to previous color scheme for today's date
            style = style.add_modifier(ratatui::style::Modifier::BOLD).fg(Color::Green);
        }

        // Highlight selected date
        if current_date == date {
            // Reverting to previous color scheme for selected date
            style = style.bg(Color::Blue).fg(Color::Black);
        }

        let paragraph = Paragraph::new(day_num).style(style).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, day_area);

        // Move to next day
        current_date = current_date.checked_add_signed(ChronoDuration::days(1)).unwrap_or(current_date);
        col += 1;
        if col >= 7 {
            col = 0;
            row += 1;
            // Stop if we moved into the next month and finished a full week
            if current_date.month() != display_month.month() && row > 6 {
                break;
            }
        }
    }
}


fn draw_floating_input(frame: &mut ratatui::Frame, app: &mut App, parent_area: Rect) {
    // Calculate the size and position of the floating window
    let width = parent_area.width.min(80);
    let height = 29; // FIX: Increased height to 29 (27 inner lines + 2 borders)
    let x = (parent_area.width.saturating_sub(width)) / 2;
    let y = (parent_area.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);

    let mode_indicator = match app.input_mode {
        InputMode::Normal => "NORMAL (i/Enter: Insert | j/k/h/l: Navigate | </>: Month Nav)",
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
    
    // Split the inner area for fields and Submit button
    let inner_area = block.inner(area);
    
    // Determine if the calendar grid should be visible
    let grid_is_visible = app.todo_input_focus == TodoInputFocus::CalendarDate;
    
    // Determine if date navigation is active (Insert mode on CalendarDate)
    let date_is_navigable = app.todo_input_focus == TodoInputFocus::CalendarDate && app.input_mode == InputMode::Insert;

    // Adjust constraints based on whether the calendar grid is visible
    let constraints = if grid_is_visible {
        // Show calendar grid (9 lines required for 7 rows + borders/title)
        vec![
            Constraint::Length(2), // 0: Title
            Constraint::Length(4), // 1: Description (FIX: Increased to 4 lines)
            Constraint::Length(4), // 2: Subtasks
            Constraint::Length(2), // 3: Due By Toggle (NEW)
            Constraint::Length(1), // 4: Date Label
            Constraint::Length(1), // 5: Date Input (Year/Month/Day display)
            Constraint::Length(9), // 6: Calendar Grid (FIX: Increased to 9 lines)
            Constraint::Length(2), // 7: Calendar Time
            Constraint::Length(2), // 8: Priority
            Constraint::Length(1), // 9: Spacer
            Constraint::Length(1), // 10: Submit button
        ]
    } else {
        // Hide calendar grid, collapsing the 9 lines into 0
        vec![
            Constraint::Length(2), // 0: Title
            Constraint::Length(4), // 1: Description (FIX: Increased to 4 lines)
            Constraint::Length(4), // 2: Subtasks
            Constraint::Length(2), // 3: Due By Toggle (NEW)
            Constraint::Length(1), // 4: Date Label
            Constraint::Length(1), // 5: Date Input (Year/Month/Day display)
            Constraint::Length(0), // 6: Calendar Grid (Collapsed)
            Constraint::Length(2), // 7: Calendar Time
            Constraint::Length(2), // 8: Priority
            Constraint::Length(1), // 9: Spacer
            Constraint::Length(1), // 10: Submit button
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner_area);

    // Helper to determine if a field is currently active (focused AND in Insert mode, or Normal mode for non-Insert fields)
    let is_active = |focus: TodoInputFocus| {
        // CalendarDate is active if focused AND in Insert mode (for navigation)
        if focus == TodoInputFocus::CalendarDate {
            date_is_navigable
        } else {
            app.todo_input_focus == focus && (app.input_mode == InputMode::Normal || focus != TodoInputFocus::Submit)
        }
    };
    
    // --- Title Input ---
    let title_style = if is_active(TodoInputFocus::Title) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    let title_label_chunks = Layout::horizontal([
        Constraint::Length(7),
        Constraint::Min(0),
    ]).split(chunks[0]);

    let title_label = Paragraph::new("Title:").style(title_style);
    frame.render_widget(title_label, title_label_chunks[0]);
    
    let title_input = Paragraph::new(app.title_buffer.as_str()).style(title_style);
    frame.render_widget(title_input, title_label_chunks[1]);
    let title_input_area = title_label_chunks[1]; // Used for cursor positioning

    // --- Description Input (Required, Multiline) ---
    let desc_style = if is_active(TodoInputFocus::Description) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    let desc_label = Paragraph::new("Description:").style(desc_style);
    frame.render_widget(desc_label, chunks[1]);
    
    let desc_input_area = Rect::new(chunks[1].x, chunks[1].y + 1, chunks[1].width, 3);
    
    // Calculate cursor position and scroll offset for Description (similar to Subtasks)
    let desc_line_width = desc_input_area.width as usize;
    let desc_cursor_pos = app.description_buffer.len();
    
    let mut desc_current_line_index = 0;
    let mut desc_current_col_index = 0;
    
    for (i, c) in app.description_buffer.chars().enumerate() {
        if i == desc_cursor_pos {
            break;
        }
        if c == '\n' {
            desc_current_line_index += 1;
            desc_current_col_index = 0;
        } else {
            desc_current_col_index += 1;
            if desc_current_col_index >= desc_line_width {
                desc_current_line_index += 1;
                desc_current_col_index = 0;
            }
        }
    }
    
    let desc_cursor_line = desc_current_line_index as u16;
    let desc_cursor_col = desc_current_col_index as u16;
    
    let desc_viewport_height = desc_input_area.height; // 3 lines
    
    // We need a separate scroll state for description if we want to support scrolling, 
    // but for simplicity and given the small size, we'll just use a local scroll offset calculation here.
    let mut desc_scroll_offset = 0;
    if desc_cursor_line >= desc_scroll_offset + desc_viewport_height {
        desc_scroll_offset = desc_cursor_line.saturating_sub(desc_viewport_height) + 1;
    } else if desc_cursor_line < desc_scroll_offset {
        desc_scroll_offset = desc_cursor_line;
    }

    let desc_input = Paragraph::new(Text::from(app.description_buffer.as_str()))
        .wrap(Wrap { trim: false })
        .scroll((desc_scroll_offset, 0))
        .block(Block::default().borders(Borders::NONE));
        
    frame.render_widget(desc_input, desc_input_area);
    
    // --- Subtasks Input ---
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
    
    if cursor_line >= app.subtasks_scroll + viewport_height {
        app.subtasks_scroll = cursor_line.saturating_sub(viewport_height) + 1;
    } else if cursor_line < app.subtasks_scroll {
        app.subtasks_scroll = cursor_line;
    }
    
    let scroll_offset = app.subtasks_scroll;

    let subtasks_input = Paragraph::new(Text::from(app.subtasks_buffer.as_str()))
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0)) // Apply vertical scroll
        .block(Block::default().borders(Borders::NONE));
        
    frame.render_widget(subtasks_input, subtasks_input_area);

    // --- Due By Toggle (NEW) ---
    let due_by_focused = app.todo_input_focus == TodoInputFocus::DueBy;
    let due_by_style = if due_by_focused {
        Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::White)
    };
    
    let toggle_status = if app.due_by_toggle { "[X] Enabled" } else { "[ ] Disabled" };
    let toggle_text = format!("Due Date/Time: {}", toggle_status);
    
    let toggle_paragraph = Paragraph::new(toggle_text)
        .block(Block::default().borders(Borders::TOP))
        .style(due_by_style);
    frame.render_widget(toggle_paragraph, chunks[3]);


    // --- Date Label ---
    let date_label_focused_normal = app.todo_input_focus == TodoInputFocus::CalendarDate && app.input_mode == InputMode::Normal;
    
    let date_label_style = if date_label_focused_normal {
        Style::default().fg(Color::Yellow).add_modifier(ratatui::style::Modifier::UNDERLINED)
    } else if app.due_by_toggle {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    
    // --- Date Input (Year/Month/Day fields) ---
    // Combine label and Y/M/D display into chunk 4
    let date_input_area = chunks[4];
    
    let date_display_chunks = Layout::horizontal([
        Constraint::Length(6), // Label "Date:"
        Constraint::Length(6), // Year
        Constraint::Length(8), // Month
        Constraint::Length(4), // Day
        Constraint::Min(0),
    ]).split(date_input_area);

    let date_style = if date_is_navigable { // Use date_is_navigable for yellow highlight
        Style::default().fg(Color::Yellow)
    } else if app.due_by_toggle {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Render Label
    frame.render_widget(Paragraph::new("Date:").style(date_label_style), date_display_chunks[0]);

    // Display Year, Month, Day separately (read-only, selection happens in grid)
    let year_str = app.calendar_date.format("%Y").to_string();
    let month_str = app.calendar_date.format("%b").to_string();
    let day_str = app.calendar_date.format("%d").to_string();

    frame.render_widget(Paragraph::new(year_str).style(date_style), date_display_chunks[1]);
    frame.render_widget(Paragraph::new(month_str).style(date_style), date_display_chunks[2]);
    frame.render_widget(Paragraph::new(day_str).style(date_style), date_display_chunks[3]);


    // --- Calendar Grid (CONDITIONAL) ---
    if grid_is_visible {
        draw_calendar_picker(
            frame, 
            chunks[6], // Use chunk 6 for the grid
            app.calendar_date, 
            date_is_navigable // Pass true only if in Insert mode for visual feedback
        );
    }

    // --- Calendar Time Input (NEW) ---
    let time_chunk_index = 7;
    let priority_chunk_index = 8;
    let submit_chunk_index = 10;

    let time_style = if is_active(TodoInputFocus::CalendarTime) {
        Style::default().fg(Color::Yellow)
    } else if app.due_by_toggle {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    
    // Refactor Time input using horizontal layout
    let time_chunks = Layout::horizontal([
        Constraint::Length(14), // Label width
        Constraint::Length(5),  // Input width (HH:MM)
        Constraint::Min(0),
    ]).split(chunks[time_chunk_index]);

    let time_label = Paragraph::new("Time (HH:MM):").style(time_style);
    frame.render_widget(time_label, time_chunks[0]);
    
    let time_input = Paragraph::new(app.time_buffer.as_str()).style(time_style);
    frame.render_widget(time_input, time_chunks[1]);
    let time_input_area = time_chunks[1]; // Used for cursor positioning


    // --- Priority Input ---
    let priority_style = if is_active(TodoInputFocus::Priority) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    // Refactor Priority input using horizontal layout
    let priority_chunks = Layout::horizontal([
        Constraint::Length(17), // Label width
        Constraint::Length(3),  // Input width (0-10)
        Constraint::Min(0),
    ]).split(chunks[priority_chunk_index]);

    let priority_label = Paragraph::new("Priority (0-10):").style(priority_style);
    frame.render_widget(priority_label, priority_chunks[0]);
    
    let priority_input = Paragraph::new(app.priority_buffer.as_str()).style(priority_style);
    frame.render_widget(priority_input, priority_chunks[1]);
    let priority_input_area = priority_chunks[1]; // Used for cursor positioning


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
    
    frame.render_widget(Paragraph::new(submit_text)
        .style(submit_style)
        .alignment(ratatui::layout::Alignment::Center), chunks[submit_chunk_index]);

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
                // Cursor position for Description (multiline)
                let final_cursor_y = desc_input_area.y + desc_cursor_line.saturating_sub(desc_scroll_offset);
                let final_cursor_x = desc_input_area.x + desc_cursor_col;

                frame.set_cursor(
                    final_cursor_x,
                    final_cursor_y,
                );
            }
            TodoInputFocus::CalendarTime => {
                // Cursor for time input (HH:MM)
                let cursor_offset = match app.time_buffer.len() {
                    0..=1 => app.time_buffer.len(),
                    2 => 3, // Skip ':'
                    3..=4 => app.time_buffer.len() + 1,
                    _ => 5,
                } as u16;
                
                frame.set_cursor(
                    time_input_area.x + cursor_offset,
                    time_input_area.y,
                );
            }
            TodoInputFocus::Priority => {
                frame.set_cursor(
                    priority_input_area.x + app.priority_buffer.len() as u16,
                    priority_input_area.y,
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
            _ => {} // Cursor hidden for Submit/CalendarDate focus in Insert mode
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
    app.update_system_status_and_logs().await;
    app.update_dashboard_data().await;

    loop {
        // Draw the UI
        tui.draw(&mut app)?;

        // Handle input events
        if event::poll(Duration::from_millis(250))? {
            // Pass the raw event to handle_input
            app.handle_input(event::read()?).await;
        } else {
            // Periodic update for system status and logs, regardless of screen
            app.update_system_status_and_logs().await;
            
            // Only update dashboard data (which includes fetching todos) if we are on the dashboard
            if app.current_screen == Screen::Dashboard {
                app.update_dashboard_data().await;
            }
        }
        
        // Check for quit signal
        if app.current_screen == Screen::Quit {
            break;
        }
    }

    Ok(())
}
