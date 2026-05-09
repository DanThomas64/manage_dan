use crate::api::{ApiClient, Status, StatusResponse, TodoItem, Subtask, LogEntry, ListGroup, ListCategory, ListItem as ApiListItem, CommonItem, Note, NoteStatus};
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
    Lists,
    Quit,
}

/// Represents the current input mode for the Todo screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoEditMode {
    Normal,
    Adding,
}

/// Represents the current input mode within the floating dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Insert,
}

/// Which panel is focused on the Lists screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListsFocus {
    Groups,
    Categories,
    Items,
}

/// Input mode for the Lists screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListsInputMode {
    Normal,
    AddingGroup,
    AddingCategory,
    AddingItem,
    /// Browsing the common items overlay to quick-add to the list.
    QuickAdd,
}

/// Which sub-mode the Notes screen is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesMode {
    List,
    View,
    Search,
    Create,
    ConfirmDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesCreateFocus {
    Title,
    Tags,
    Folder,
    Content,
}

/// Status filter on the Notes list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesFilterStatus {
    All,
    Raw,
    NoteStatus,
    Article,
}

impl NotesFilterStatus {
    fn as_api_str(&self) -> Option<&'static str> {
        match self {
            NotesFilterStatus::All => None,
            NotesFilterStatus::Raw => Some("raw"),
            NotesFilterStatus::NoteStatus => Some("note"),
            NotesFilterStatus::Article => Some("article"),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            NotesFilterStatus::All => "All",
            NotesFilterStatus::Raw => "Raw",
            NotesFilterStatus::NoteStatus => "Note",
            NotesFilterStatus::Article => "Article",
        }
    }

    fn next(&self) -> Self {
        match self {
            NotesFilterStatus::All => NotesFilterStatus::Raw,
            NotesFilterStatus::Raw => NotesFilterStatus::NoteStatus,
            NotesFilterStatus::NoteStatus => NotesFilterStatus::Article,
            NotesFilterStatus::Article => NotesFilterStatus::All,
        }
    }
}

/// Represents which field is currently focused in the floating input form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoInputFocus {
    Title,
    Description,
    Subtasks,
    DueBy,
    CalendarDate,
    CalendarTime,
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
    pub todo_hide_completed: bool,
    
    // Add Form State
    pub input_mode: InputMode,
    pub todo_input_focus: TodoInputFocus,
    pub title_buffer: String,
    pub description_buffer: String,
    pub subtasks_buffer: String,
    pub subtasks_scroll: u16,
    pub due_by_toggle: bool,
    pub calendar_date: NaiveDate,
    pub time_buffer: String,
    pub priority_buffer: String,

    // Notes state
    pub notes: Vec<Note>,
    pub notes_list_state: ListState,
    pub notes_view_note: Option<Note>,
    pub notes_mode: NotesMode,
    pub notes_filter: NotesFilterStatus,
    pub notes_search_buf: String,
    pub notes_scroll: u16,
    pub notes_folders: Vec<String>,
    pub notes_create_title: String,
    pub notes_create_tags: String,
    pub notes_create_folder: String,
    pub notes_create_content: String,
    pub notes_create_focus: NotesCreateFocus,

    // Lists State
    pub list_groups: Vec<ListGroup>,
    pub list_categories: Vec<ListCategory>,
    pub list_items: Vec<ApiListItem>,
    pub list_group_state: ListState,
    pub list_category_state: ListState,
    pub list_item_state: ListState,
    pub lists_focus: ListsFocus,
    pub lists_input_mode: ListsInputMode,
    pub lists_input_buffer: String,
    pub common_items: Vec<CommonItem>,
    pub common_item_state: ListState,
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
            todo_hide_completed: false,
            
            input_mode: InputMode::Normal,
            todo_input_focus: TodoInputFocus::Title,
            title_buffer: String::new(),
            description_buffer: String::new(),
            subtasks_buffer: String::new(),
            subtasks_scroll: 0,
            due_by_toggle: false,
            calendar_date: now,
            time_buffer: String::from("00:00"),
            priority_buffer: String::new(),

            notes: Vec::new(),
            notes_list_state: ListState::default(),
            notes_view_note: None,
            notes_mode: NotesMode::List,
            notes_filter: NotesFilterStatus::All,
            notes_search_buf: String::new(),
            notes_scroll: 0,
            notes_folders: Vec::new(),
            notes_create_title: String::new(),
            notes_create_tags: String::new(),
            notes_create_folder: String::new(),
            notes_create_content: String::new(),
            notes_create_focus: NotesCreateFocus::Content,

            list_groups: Vec::new(),
            list_categories: Vec::new(),
            list_items: Vec::new(),
            list_group_state: ListState::default(),
            list_category_state: ListState::default(),
            list_item_state: ListState::default(),
            lists_focus: ListsFocus::Groups,
            lists_input_mode: ListsInputMode::Normal,
            lists_input_buffer: String::new(),
            common_items: Vec::new(),
            common_item_state: ListState::default(),
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

    // --- Lists helpers ---

    pub async fn fetch_list_groups(&mut self) {
        match self.api_client.fetch_list_groups().await {
            Ok(groups) => {
                self.list_groups = groups;
                self.list_categories.clear();
                self.list_category_state.select(None);
                self.list_items.clear();
                self.list_item_state.select(None);
                if self.list_groups.is_empty() {
                    self.list_group_state.select(None);
                } else {
                    let sel = self.list_group_state.selected().unwrap_or(0)
                        .min(self.list_groups.len().saturating_sub(1));
                    self.list_group_state.select(Some(sel));
                    self.fetch_list_categories_for_selected_group().await;
                }
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("Lists API error: {}", e)),
        }
    }

    pub async fn fetch_list_categories_for_selected_group(&mut self) {
        let group_id = match self.list_group_state.selected()
            .and_then(|i| self.list_groups.get(i))
        {
            Some(g) => g.id,
            None => return,
        };
        match self.api_client.fetch_list_categories(group_id).await {
            Ok(cats) => {
                self.list_categories = cats;
                self.list_items.clear();
                self.list_item_state.select(None);
                if self.list_categories.is_empty() {
                    self.list_category_state.select(None);
                } else {
                    let sel = self.list_category_state.selected().unwrap_or(0)
                        .min(self.list_categories.len().saturating_sub(1));
                    self.list_category_state.select(Some(sel));
                    self.fetch_list_items_for_selected().await;
                }
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("Lists API error: {}", e)),
        }
    }

    pub async fn fetch_list_items_for_selected(&mut self) {
        if let Some(idx) = self.list_category_state.selected() {
            if let Some(cat) = self.list_categories.get(idx) {
                let id = cat.id;
                match self.api_client.fetch_list_items(id).await {
                    Ok(items) => {
                        self.list_items = items;
                        if self.list_items.is_empty() {
                            self.list_item_state.select(None);
                        } else {
                            let sel = self.list_item_state.selected().unwrap_or(0)
                                .min(self.list_items.len().saturating_sub(1));
                            self.list_item_state.select(Some(sel));
                        }
                    }
                    Err(e) => self.last_error = Some(format!("Lists API error: {}", e)),
                }
            }
        }
    }

    fn lists_move_group(&mut self, delta: i32) {
        let len = self.list_groups.len();
        if len == 0 { return; }
        let cur = self.list_group_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.list_group_state.select(Some(next));
        self.list_categories.clear();
        self.list_category_state.select(None);
        self.list_items.clear();
        self.list_item_state.select(None);
    }

    fn lists_move_category(&mut self, delta: i32) {
        let len = self.list_categories.len();
        if len == 0 { return; }
        let cur = self.list_category_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.list_category_state.select(Some(next));
        self.list_item_state.select(None);
        self.list_items.clear();
    }

    fn lists_move_item(&mut self, delta: i32) {
        let len = self.list_items.len();
        if len == 0 { return; }
        let cur = self.list_item_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.list_item_state.select(Some(next));
    }

    pub async fn lists_toggle_check(&mut self) {
        if let Some(idx) = self.list_item_state.selected() {
            if let Some(item) = self.list_items.get(idx) {
                let id = item.id;
                let new_checked = !item.checked;
                if let Err(e) = self.api_client.check_list_item(id, new_checked).await {
                    self.last_error = Some(format!("Check failed: {}", e));
                } else {
                    self.fetch_list_items_for_selected().await;
                }
            }
        }
    }

    pub async fn lists_delete_item(&mut self) {
        if let Some(idx) = self.list_item_state.selected() {
            if let Some(item) = self.list_items.get(idx) {
                let id = item.id;
                if let Err(e) = self.api_client.delete_list_item(id).await {
                    self.last_error = Some(format!("Delete failed: {}", e));
                } else {
                    self.fetch_list_items_for_selected().await;
                }
            }
        }
    }

    pub async fn lists_delete_category(&mut self) {
        if let Some(idx) = self.list_category_state.selected() {
            if let Some(cat) = self.list_categories.get(idx) {
                let id = cat.id;
                if let Err(e) = self.api_client.delete_list_category(id).await {
                    self.last_error = Some(format!("Delete failed: {}", e));
                } else {
                    self.fetch_list_categories_for_selected_group().await;
                }
            }
        }
    }

    pub async fn lists_delete_group(&mut self) {
        if let Some(idx) = self.list_group_state.selected() {
            if let Some(group) = self.list_groups.get(idx) {
                let id = group.id;
                if let Err(e) = self.api_client.delete_list_group(id).await {
                    self.last_error = Some(format!("Delete failed: {}", e));
                } else {
                    self.fetch_list_groups().await;
                }
            }
        }
    }

    pub async fn lists_clear_checked(&mut self) {
        if let Some(idx) = self.list_category_state.selected() {
            if let Some(cat) = self.list_categories.get(idx) {
                let id = cat.id;
                if let Err(e) = self.api_client.clear_list_checked(id).await {
                    self.last_error = Some(format!("Clear failed: {}", e));
                } else {
                    self.fetch_list_items_for_selected().await;
                }
            }
        }
    }

    pub async fn lists_print_list(&mut self) {
        if let Some(idx) = self.list_category_state.selected() {
            if let Some(cat) = self.list_categories.get(idx) {
                let id = cat.id;
                if let Err(e) = self.api_client.print_list(id).await {
                    self.last_error = Some(format!("Print failed: {}", e));
                }
            }
        }
    }

    pub async fn lists_submit_input(&mut self) {
        let input = self.lists_input_buffer.trim().to_string();
        if input.is_empty() {
            self.lists_input_mode = ListsInputMode::Normal;
            self.lists_input_buffer.clear();
            return;
        }
        match self.lists_input_mode {
            ListsInputMode::AddingGroup => {
                if let Err(e) = self.api_client.add_list_group(&input).await {
                    self.last_error = Some(format!("Add group failed: {}", e));
                } else {
                    self.fetch_list_groups().await;
                }
            }
            ListsInputMode::AddingCategory => {
                if let Some(group) = self.list_group_state.selected()
                    .and_then(|i| self.list_groups.get(i))
                {
                    let group_id = group.id;
                    if let Err(e) = self.api_client.add_list_category(group_id, &input).await {
                        self.last_error = Some(format!("Add category failed: {}", e));
                    } else {
                        self.fetch_list_categories_for_selected_group().await;
                    }
                }
            }
            ListsInputMode::AddingItem => {
                if let Some(idx) = self.list_category_state.selected() {
                    if let Some(cat) = self.list_categories.get(idx) {
                        let cat_id = cat.id;
                        if let Err(e) = self.api_client.add_list_item(cat_id, &input, None).await {
                            self.last_error = Some(format!("Add item failed: {}", e));
                        } else {
                            self.fetch_list_items_for_selected().await;
                        }
                    }
                }
            }
            ListsInputMode::Normal | ListsInputMode::QuickAdd => {}
        }
        self.lists_input_mode = ListsInputMode::Normal;
        self.lists_input_buffer.clear();
    }

    /// Opens the QuickAdd overlay: fetches common items for the selected category.
    pub async fn lists_open_quick_add(&mut self) {
        if let Some(idx) = self.list_category_state.selected() {
            if let Some(cat) = self.list_categories.get(idx) {
                let id = cat.id;
                match self.api_client.fetch_common_items(id).await {
                    Ok(items) => {
                        self.common_items = items;
                        if self.common_items.is_empty() {
                            self.common_item_state.select(None);
                        } else {
                            self.common_item_state.select(Some(0));
                        }
                        self.lists_input_mode = ListsInputMode::QuickAdd;
                        self.last_error = None;
                    }
                    Err(e) => self.last_error = Some(format!("Failed to fetch common items: {}", e)),
                }
            }
        }
    }

    /// Adds the currently highlighted common item to the active list.
    pub async fn lists_quick_add_selected(&mut self) {
        if let Some(idx) = self.common_item_state.selected() {
            if let Some(common) = self.common_items.get(idx) {
                let id = common.id;
                if let Err(e) = self.api_client.add_item_from_common(id).await {
                    self.last_error = Some(format!("Quick add failed: {}", e));
                } else {
                    self.fetch_list_items_for_selected().await;
                }
            }
        }
    }

    /// Saves the currently selected list item as a common item for its category.
    pub async fn lists_save_as_common(&mut self) {
        if let Some(idx) = self.list_item_state.selected() {
            if let Some(item) = self.list_items.get(idx) {
                let cat_id = item.category_id;
                let name = item.name.clone();
                let qty = item.quantity.clone();
                if let Err(e) = self.api_client.add_common_item(cat_id, &name, qty.as_deref()).await {
                    self.last_error = Some(format!("Save as common failed: {}", e));
                } else {
                    self.last_error = None;
                }
            }
        }
    }

    /// Deletes the highlighted common item from the saved templates.
    pub async fn lists_delete_common_item(&mut self) {
        if let Some(idx) = self.common_item_state.selected() {
            if let Some(common) = self.common_items.get(idx) {
                let id = common.id;
                if let Err(e) = self.api_client.delete_common_item(id).await {
                    self.last_error = Some(format!("Delete common failed: {}", e));
                } else {
                    self.common_items.remove(idx);
                    let new_sel = if self.common_items.is_empty() {
                        None
                    } else {
                        Some(idx.saturating_sub(1))
                    };
                    self.common_item_state.select(new_sel);
                }
            }
        }
    }

    // --- Notes helpers ---

    pub async fn fetch_notes_filtered(&mut self) {
        let status = self.notes_filter.as_api_str();
        match self.api_client.fetch_notes(status, None).await {
            Ok(notes) => {
                self.notes = notes;
                let sel = self.notes_list_state.selected().unwrap_or(0)
                    .min(self.notes.len().saturating_sub(1));
                if self.notes.is_empty() {
                    self.notes_list_state.select(None);
                } else {
                    self.notes_list_state.select(Some(sel));
                }
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("Notes error: {}", e)),
        }
        match self.api_client.fetch_note_folders().await {
            Ok(folders) => self.notes_folders = folders,
            Err(_) => {}
        }
    }

    fn notes_move(&mut self, delta: i32) {
        let len = self.notes.len();
        if len == 0 { return; }
        let cur = self.notes_list_state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        self.notes_list_state.select(Some(next));
    }

    pub async fn notes_open_selected(&mut self) {
        if let Some(idx) = self.notes_list_state.selected() {
            if let Some(note) = self.notes.get(idx) {
                let uuid = note.uuid.clone();
                match self.api_client.fetch_note(&uuid).await {
                    Ok(full_note) => {
                        self.notes_view_note = Some(full_note);
                        self.notes_scroll = 0;
                        self.notes_mode = NotesMode::View;
                    }
                    Err(e) => self.last_error = Some(format!("Failed to load note: {}", e)),
                }
            }
        }
    }

    pub fn notes_cycle_filter(&mut self) {
        self.notes_filter = self.notes_filter.next();
    }

    pub async fn notes_run_search(&mut self) {
        let q = self.notes_search_buf.trim().to_string();
        if q.is_empty() {
            self.fetch_notes_filtered().await;
            return;
        }
        match self.api_client.search_notes(&q).await {
            Ok(notes) => {
                self.notes = notes;
                if self.notes.is_empty() {
                    self.notes_list_state.select(None);
                } else {
                    self.notes_list_state.select(Some(0));
                }
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("Search error: {}", e)),
        }
    }

    pub async fn notes_advance_selected(&mut self) {
        if let Some(idx) = self.notes_list_state.selected() {
            if let Some(note) = self.notes.get(idx) {
                let uuid = note.uuid.clone();
                match self.api_client.advance_note_status(&uuid).await {
                    Ok(updated) => {
                        self.notes[idx] = updated;
                        self.last_error = None;
                    }
                    Err(e) => self.last_error = Some(format!("Advance failed: {}", e)),
                }
            }
        }
    }

    pub async fn notes_advance_current(&mut self) {
        if let Some(note) = self.notes_view_note.clone() {
            match self.api_client.advance_note_status(&note.uuid).await {
                Ok(updated) => {
                    if let Some(idx) = self.notes.iter().position(|n| n.uuid == note.uuid) {
                        self.notes[idx] = updated.clone();
                    }
                    self.notes_view_note = Some(updated);
                    self.last_error = None;
                }
                Err(e) => self.last_error = Some(format!("Advance failed: {}", e)),
            }
        }
    }

    pub async fn notes_delete_selected(&mut self) {
        let uuid = match &self.notes_view_note {
            Some(n) => n.uuid.clone(),
            None => match self.notes_list_state.selected()
                .and_then(|i| self.notes.get(i))
            {
                Some(n) => n.uuid.clone(),
                None => return,
            },
        };
        match self.api_client.delete_note(&uuid).await {
            Ok(()) => {
                self.notes_view_note = None;
                self.notes_mode = NotesMode::List;
                self.fetch_notes_filtered().await;
            }
            Err(e) => self.last_error = Some(format!("Delete failed: {}", e)),
        }
    }

    pub async fn notes_submit_create(&mut self) {
        let content = self.notes_create_content.trim().to_string();
        if content.is_empty() {
            return;
        }
        let tags: Vec<String> = self.notes_create_tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let title = self.notes_create_title.trim().to_string();
        let folder = self.notes_create_folder.trim().to_string();
        match self.api_client.create_note(&title, &content, tags, &folder).await {
            Ok(_) => {
                self.notes_create_title.clear();
                self.notes_create_tags.clear();
                self.notes_create_folder.clear();
                self.notes_create_content.clear();
                self.notes_create_focus = NotesCreateFocus::Content;
                self.notes_mode = NotesMode::List;
                self.fetch_notes_filtered().await;
            }
            Err(e) => self.last_error = Some(format!("Create failed: {}", e)),
        }
    }

    /// Suspend the TUI, open the note in an external editor, sync content back via API.
    pub async fn notes_open_editor(&mut self) {
        let note = match self.notes_view_note.clone() {
            Some(n) => n,
            None => return,
        };

        let tmp_path = std::env::temp_dir().join(format!("{}.md", note.uuid));
        if let Err(e) = std::fs::write(&tmp_path, &note.content) {
            self.last_error = Some(format!("Failed to write temp file: {}", e));
            return;
        }

        let editor = std::env::var("NOTES_EDITOR")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vi".to_string());

        // Restore terminal so the editor can take over.
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);

        let status = std::process::Command::new(&editor)
            .arg(&tmp_path)
            .status();

        // Re-acquire the terminal.
        let _ = enable_raw_mode();
        let _ = stdout().execute(EnterAlternateScreen);

        if let Err(e) = status {
            self.last_error = Some(format!("Editor '{}' failed: {}", editor, e));
            let _ = std::fs::remove_file(&tmp_path);
            return;
        }

        let new_content = match std::fs::read_to_string(&tmp_path) {
            Ok(c) => c,
            Err(e) => {
                self.last_error = Some(format!("Failed to read edited file: {}", e));
                let _ = std::fs::remove_file(&tmp_path);
                return;
            }
        };
        let _ = std::fs::remove_file(&tmp_path);

        match self.api_client.update_note(&note.uuid, &new_content, None).await {
            Ok(updated) => {
                if let Some(idx) = self.notes.iter().position(|n| n.uuid == note.uuid) {
                    self.notes[idx] = updated.clone();
                }
                self.notes_view_note = Some(updated);
                self.notes_scroll = 0;
                self.last_error = None;
            }
            Err(e) => self.last_error = Some(format!("Failed to sync note: {}", e)),
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
                                KeyCode::Char('f') => {
                                    self.todo_hide_completed = !self.todo_hide_completed;
                                    // Clamp selection to the new visible range
                                    let visible_count = self.visible_todo_indices().len();
                                    match self.todo_list_state.selected() {
                                        Some(i) if i >= visible_count => {
                                            self.todo_list_state.select(Some(visible_count.saturating_sub(1)));
                                        }
                                        None if visible_count > 0 => {
                                            self.todo_list_state.select(Some(0));
                                        }
                                        _ => {}
                                    }
                                }
                                KeyCode::Char('a') => self.start_add_mode(),
                                KeyCode::Char('p') => { self.print_selected().await; action_taken = true; }
                                KeyCode::Char('x') => { self.archive_selected().await; action_taken = true; }
                                KeyCode::Char('d') => { self.delete_selected().await; action_taken = true; }
                                _ => { self.handle_nav_key(key.code); }
                            }
                        }
                    }
                    TodoEditMode::Adding => {
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
            Screen::Notes => {
                if let CEvent::Key(key) = event {
                    match self.notes_mode {
                        NotesMode::List => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                self.current_screen = Screen::Dashboard;
                            }
                            KeyCode::Up | KeyCode::Char('k') => self.notes_move(-1),
                            KeyCode::Down | KeyCode::Char('j') => self.notes_move(1),
                            KeyCode::Enter => {
                                self.notes_open_selected().await;
                                action_taken = true;
                            }
                            KeyCode::Tab => {
                                self.notes_cycle_filter();
                                self.fetch_notes_filtered().await;
                                action_taken = true;
                            }
                            KeyCode::Char('/') => {
                                self.notes_mode = NotesMode::Search;
                                self.notes_search_buf.clear();
                            }
                            KeyCode::Char('r') => {
                                self.fetch_notes_filtered().await;
                                action_taken = true;
                            }
                            KeyCode::Char('a') => {
                                self.notes_advance_selected().await;
                                action_taken = true;
                            }
                            KeyCode::Char('d') => {
                                self.notes_mode = NotesMode::ConfirmDelete;
                            }
                            KeyCode::Char('n') => {
                                self.notes_mode = NotesMode::Create;
                            }
                            _ => { self.handle_nav_key(key.code); }
                        },
                        NotesMode::Create => {
                            match key.code {
                                KeyCode::Esc => {
                                    self.notes_mode = NotesMode::List;
                                }
                                KeyCode::Tab => {
                                    self.notes_create_focus = match self.notes_create_focus {
                                        NotesCreateFocus::Title => NotesCreateFocus::Folder,
                                        NotesCreateFocus::Folder => NotesCreateFocus::Tags,
                                        NotesCreateFocus::Tags => NotesCreateFocus::Content,
                                        NotesCreateFocus::Content => NotesCreateFocus::Title,
                                    };
                                }
                                KeyCode::BackTab => {
                                    self.notes_create_focus = match self.notes_create_focus {
                                        NotesCreateFocus::Title => NotesCreateFocus::Content,
                                        NotesCreateFocus::Folder => NotesCreateFocus::Title,
                                        NotesCreateFocus::Tags => NotesCreateFocus::Folder,
                                        NotesCreateFocus::Content => NotesCreateFocus::Tags,
                                    };
                                }
                                KeyCode::Char('s') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                                    self.notes_submit_create().await;
                                    action_taken = true;
                                }
                                KeyCode::Enter => {
                                    if self.notes_create_focus == NotesCreateFocus::Content {
                                        self.notes_create_content.push('\n');
                                    } else {
                                        self.notes_create_focus = match self.notes_create_focus {
                                            NotesCreateFocus::Title => NotesCreateFocus::Folder,
                                            NotesCreateFocus::Folder => NotesCreateFocus::Tags,
                                            NotesCreateFocus::Tags => NotesCreateFocus::Content,
                                            NotesCreateFocus::Content => NotesCreateFocus::Content,
                                        };
                                    }
                                }
                                KeyCode::Backspace => {
                                    let buf = match self.notes_create_focus {
                                        NotesCreateFocus::Title => &mut self.notes_create_title,
                                        NotesCreateFocus::Tags => &mut self.notes_create_tags,
                                        NotesCreateFocus::Folder => &mut self.notes_create_folder,
                                        NotesCreateFocus::Content => &mut self.notes_create_content,
                                    };
                                    buf.pop();
                                }
                                KeyCode::Char(c) => {
                                    let buf = match self.notes_create_focus {
                                        NotesCreateFocus::Title => &mut self.notes_create_title,
                                        NotesCreateFocus::Tags => &mut self.notes_create_tags,
                                        NotesCreateFocus::Folder => &mut self.notes_create_folder,
                                        NotesCreateFocus::Content => &mut self.notes_create_content,
                                    };
                                    buf.push(c);
                                }
                                _ => {}
                            }
                        }
                        NotesMode::View => match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                self.notes_mode = NotesMode::List;
                                self.notes_view_note = None;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                self.notes_scroll = self.notes_scroll.saturating_sub(3);
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                self.notes_scroll = self.notes_scroll.saturating_add(3);
                            }
                            KeyCode::Char('e') => {
                                self.notes_open_editor().await;
                                action_taken = true;
                            }
                            KeyCode::Char('a') => {
                                self.notes_advance_current().await;
                                action_taken = true;
                            }
                            KeyCode::Char('d') => {
                                self.notes_mode = NotesMode::ConfirmDelete;
                            }
                            KeyCode::Char('p') => {
                                if let Some(ref note) = self.notes_view_note.clone() {
                                    let _ = self.api_client.print_note(&note.uuid).await;
                                    action_taken = true;
                                }
                            }
                            _ => { self.handle_nav_key(key.code); }
                        },
                        NotesMode::Search => match key.code {
                            KeyCode::Esc => {
                                self.notes_mode = NotesMode::List;
                                self.notes_search_buf.clear();
                                self.fetch_notes_filtered().await;
                                action_taken = true;
                            }
                            KeyCode::Enter => {
                                self.notes_run_search().await;
                                self.notes_mode = NotesMode::List;
                                action_taken = true;
                            }
                            KeyCode::Backspace => { self.notes_search_buf.pop(); }
                            KeyCode::Char(c) => self.notes_search_buf.push(c),
                            _ => {}
                        },
                        NotesMode::ConfirmDelete => match key.code {
                            KeyCode::Char('y') | KeyCode::Char('Y') => {
                                self.notes_delete_selected().await;
                                action_taken = true;
                            }
                            _ => {
                                self.notes_mode = if self.notes_view_note.is_some() {
                                    NotesMode::View
                                } else {
                                    NotesMode::List
                                };
                            }
                        },
                    }
                }
            }
            Screen::Project => {
                if let CEvent::Key(key) = event {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => self.current_screen = Screen::Dashboard,
                        _ => { self.handle_nav_key(key.code); }
                    }
                }
            }
            Screen::Lists => {
                if let CEvent::Key(key) = event {
                    match self.lists_input_mode {
                        ListsInputMode::Normal => {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => {
                                    self.current_screen = Screen::Dashboard;
                                }
                                KeyCode::Tab => {
                                    self.lists_focus = match self.lists_focus {
                                        ListsFocus::Groups => ListsFocus::Categories,
                                        ListsFocus::Categories => ListsFocus::Items,
                                        ListsFocus::Items => ListsFocus::Groups,
                                    };
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    match self.lists_focus {
                                        ListsFocus::Groups => {
                                            self.lists_move_group(-1);
                                            self.fetch_list_categories_for_selected_group().await;
                                        }
                                        ListsFocus::Categories => {
                                            self.lists_move_category(-1);
                                            self.fetch_list_items_for_selected().await;
                                        }
                                        ListsFocus::Items => self.lists_move_item(-1),
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    match self.lists_focus {
                                        ListsFocus::Groups => {
                                            self.lists_move_group(1);
                                            self.fetch_list_categories_for_selected_group().await;
                                        }
                                        ListsFocus::Categories => {
                                            self.lists_move_category(1);
                                            self.fetch_list_items_for_selected().await;
                                        }
                                        ListsFocus::Items => self.lists_move_item(1),
                                    }
                                }
                                KeyCode::Char('a') => {
                                    match self.lists_focus {
                                        ListsFocus::Groups => {
                                            self.lists_input_mode = ListsInputMode::AddingGroup;
                                            self.lists_input_buffer.clear();
                                        }
                                        ListsFocus::Categories => {
                                            self.lists_input_mode = ListsInputMode::AddingCategory;
                                            self.lists_input_buffer.clear();
                                        }
                                        ListsFocus::Items => {
                                            self.lists_input_mode = ListsInputMode::AddingItem;
                                            self.lists_input_buffer.clear();
                                        }
                                    }
                                }
                                KeyCode::Char('d') => {
                                    match self.lists_focus {
                                        ListsFocus::Groups => self.lists_delete_group().await,
                                        ListsFocus::Categories => self.lists_delete_category().await,
                                        ListsFocus::Items => self.lists_delete_item().await,
                                    }
                                    action_taken = true;
                                }
                                KeyCode::Char(' ') | KeyCode::Char('c') => {
                                    self.lists_toggle_check().await;
                                    action_taken = true;
                                }
                                KeyCode::Char('C') => {
                                    self.lists_clear_checked().await;
                                    action_taken = true;
                                }
                                KeyCode::Char('p') => {
                                    self.lists_print_list().await;
                                }
                                // Save selected item as a common item template
                                KeyCode::Char('s') => {
                                    if self.lists_focus == ListsFocus::Items {
                                        self.lists_save_as_common().await;
                                    }
                                }
                                // Open Quick Add overlay (common items)
                                KeyCode::Char('A') => {
                                    if self.lists_focus == ListsFocus::Items
                                        || self.lists_focus == ListsFocus::Categories
                                    {
                                        self.lists_open_quick_add().await;
                                    }
                                }
                                KeyCode::Char('r') => {
                                    self.fetch_list_groups().await;
                                    action_taken = true;
                                }
                                _ => { self.handle_nav_key(key.code); }
                            }
                        }
                        ListsInputMode::QuickAdd => {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => {
                                    self.lists_input_mode = ListsInputMode::Normal;
                                    self.common_items.clear();
                                    self.common_item_state.select(None);
                                }
                                KeyCode::Up | KeyCode::Char('k') => {
                                    let len = self.common_items.len();
                                    if len > 0 {
                                        let cur = self.common_item_state.selected().unwrap_or(0) as i32;
                                        let next = (cur - 1).rem_euclid(len as i32) as usize;
                                        self.common_item_state.select(Some(next));
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    let len = self.common_items.len();
                                    if len > 0 {
                                        let cur = self.common_item_state.selected().unwrap_or(0) as i32;
                                        let next = (cur + 1).rem_euclid(len as i32) as usize;
                                        self.common_item_state.select(Some(next));
                                    }
                                }
                                KeyCode::Enter => {
                                    self.lists_quick_add_selected().await;
                                    action_taken = true;
                                }
                                KeyCode::Char('d') => {
                                    self.lists_delete_common_item().await;
                                }
                                _ => {}
                            }
                        }
                        ListsInputMode::AddingGroup
                        | ListsInputMode::AddingCategory
                        | ListsInputMode::AddingItem => {
                            match key.code {
                                KeyCode::Enter => {
                                    self.lists_submit_input().await;
                                    action_taken = true;
                                }
                                KeyCode::Esc => {
                                    self.lists_input_mode = ListsInputMode::Normal;
                                    self.lists_input_buffer.clear();
                                }
                                KeyCode::Backspace => { self.lists_input_buffer.pop(); }
                                KeyCode::Char(c) => { self.lists_input_buffer.push(c); }
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
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
                Screen::Lists => {
                    self.fetch_list_groups().await;
                }
                Screen::Notes => {
                    self.fetch_notes_filtered().await;
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

    fn handle_nav_key(&mut self, key_code: KeyCode) -> bool {
        match key_code {
            KeyCode::Char('1') => { self.current_screen = Screen::Todo; true }
            KeyCode::Char('2') => { self.current_screen = Screen::Notes; true }
            KeyCode::Char('3') => { self.current_screen = Screen::Project; true }
            KeyCode::Char('4') => { self.current_screen = Screen::Lists; true }
            _ => false,
        }
    }

    fn handle_dashboard_input(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Char('q') => self.current_screen = Screen::Quit,
            KeyCode::Char('r') => { /* update_status is called automatically */ }
            _ => { self.handle_nav_key(key_code); }
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
                                    self.due_by_toggle = !self.due_by_toggle;
                                }
                                TodoInputFocus::CalendarDate => {
                                    self.input_mode = InputMode::Insert;
                                }
                                TodoInputFocus::CalendarTime | TodoInputFocus::Priority | TodoInputFocus::Title | TodoInputFocus::Description | TodoInputFocus::Subtasks => {
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
        self.input_mode = InputMode::Normal;
        self.title_buffer.clear();
        self.description_buffer.clear();
        self.subtasks_buffer.clear();
        self.due_by_toggle = false;
        self.calendar_date = now;
        self.time_buffer = String::from("00:00");
        self.priority_buffer.clear();
        self.subtasks_scroll = 0;
        self.todo_input_focus = TodoInputFocus::Title;
        self.last_error = None;
    }

    fn cancel_edit_mode(&mut self) {
        let now = Local::now().date_naive();
        self.todo_edit_mode = TodoEditMode::Normal;
        self.input_mode = InputMode::Normal;
        self.title_buffer.clear();
        self.description_buffer.clear();
        self.subtasks_buffer.clear();
        self.due_by_toggle = false;
        self.calendar_date = now;
        self.time_buffer = String::from("00:00");
        self.priority_buffer.clear();
        self.subtasks_scroll = 0;
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

        let mut new_item = TodoItem::new(title, description);
        new_item.subtasks = parsed_subtasks;
        new_item.due_date = due_date_opt;
        new_item.priority = priority;
        let result = self.api_client.create_todo(new_item).await.map(|_| ());
        
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

    /// Returns the indices into `todo_items` that are currently visible
    /// (respecting the hide-completed filter).
    fn visible_todo_indices(&self) -> Vec<usize> {
        self.todo_items
            .iter()
            .enumerate()
            .filter(|(_, item)| !(self.todo_hide_completed && item.completed))
            .map(|(i, _)| i)
            .collect()
    }

    fn move_selection(&mut self, delta: i32) {
        let visible = self.visible_todo_indices();
        if visible.is_empty() {
            self.todo_list_state.select(None);
            return;
        }
        let current_index = self.todo_list_state.selected().unwrap_or(0);
        let new_index = (current_index as i32 + delta)
            .rem_euclid(visible.len() as i32) as usize;
        self.todo_list_state.select(Some(new_index));
    }
    
    async fn toggle_completed(&mut self) {
        let visible = self.visible_todo_indices();
        if let Some(vis_index) = self.todo_list_state.selected() {
            if let Some(&index) = visible.get(vis_index) {
                if let Some(item) = self.todo_items.get(index) {
                    if let Some(id) = item.id {
                        let new_done = !item.completed;
                        match self.api_client.complete_todo(id, new_done).await {
                            Ok(()) => { self.last_error = None; }
                            Err(e) => {
                                self.last_error = Some(format!("Failed to toggle completion: {}", e));
                            }
                        }
                    }
                }
            }
        }
    }
    
    fn selected_todo_item(&self) -> Option<&TodoItem> {
        let visible = self.visible_todo_indices();
        let vis_index = self.todo_list_state.selected()?;
        let &real_index = visible.get(vis_index)?;
        self.todo_items.get(real_index)
    }

    async fn print_selected(&mut self) {
        if let Some(item) = self.selected_todo_item() {
            if let Some(id) = item.id {
                self.last_error = Some(format!("Attempting manual print for ID {}...", id));
                match self.api_client.print_todo(id).await {
                    Ok(_) => {
                        self.last_error = Some(format!("Print job sent for ID {}.", id));
                    }
                    Err(e) => {
                        self.last_error = Some(format!("Failed to send print job for ID {}: {}", id, e));
                    }
                }
            } else {
                self.last_error = Some("Cannot print unsaved item.".to_string());
            }
        } else {
            self.last_error = Some("No item selected to print.".to_string());
        }
    }

    async fn archive_selected(&mut self) {
        if let Some(item) = self.selected_todo_item() {
            if let Some(id) = item.id {
                self.last_error = Some(format!("Attempting to archive ID {}...", id));
                match self.api_client.archive_todo(id).await {
                    Ok(_) => {
                        self.last_error = Some(format!("Item ID {} archived successfully.", id));
                    }
                    Err(e) => {
                        self.last_error = Some(format!("Failed to archive item ID {}: {}", id, e));
                    }
                }
            } else {
                self.last_error = Some("Cannot archive unsaved item.".to_string());
            }
        } else {
            self.last_error = Some("No item selected to archive.".to_string());
        }
    }

    async fn delete_selected(&mut self) {
        if let Some(item) = self.selected_todo_item() {
            if let Some(id) = item.id {
                match self.api_client.delete_todo(id).await {
                    Ok(_) => { self.last_error = None; }
                    Err(e) => {
                        self.last_error = Some(format!("Failed to delete item ID {}: {}", id, e));
                    }
                }
            } else {
                self.last_error = Some("Cannot delete unsaved item.".to_string());
            }
        } else {
            self.last_error = Some("No item selected to delete.".to_string());
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
                Screen::Notes => draw_notes_screen(frame, app, area),
                Screen::Project => draw_project_placeholder(frame, area),
                Screen::Lists => draw_lists_screen(frame, app, area),
                _ => {}
            }
        })?;
        Ok(())
    }
}

// --- Drawing Helper Functions ---

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn draw_section_header(
    frame: &mut ratatui::Frame,
    area: Rect,
    title: &str,
    summary: &str,
    color: Color,
) {
    let content = Line::from(vec![
        ratatui::text::Span::styled(
            format!(" {} ", title),
            Style::default().fg(color).add_modifier(ratatui::style::Modifier::BOLD),
        ),
        ratatui::text::Span::styled(
            format!(" v{}  ", VERSION),
            Style::default().fg(Color::Rgb(160, 160, 160)),
        ),
        ratatui::text::Span::styled(
            summary.to_string(),
            Style::default().fg(Color::Rgb(220, 220, 220)),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(color))),
        area,
    );
}

fn draw_dashboard(frame: &mut ratatui::Frame, app: &App, area: ratatui::layout::Rect) {
    let overall_status = app.status.as_ref().map(|s| s.overall.gono).unwrap_or(Status::Unknown);
    let systems_status = app.status.as_ref().map(|s| s.systems);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(area);
    let (header_area, content_area, footer_area) = (outer[0], outer[1], outer[2]);

    let status_label = match overall_status {
        Status::Go => "● GO",
        Status::Nogo => "● NOGO",
        Status::Degraded => "◐ DEGRADED",
        _ => "○ UNKNOWN",
    };
    let now_str = Local::now().format("%a %d %b %Y  %H:%M").to_string();
    draw_section_header(frame, header_area, "DASHBOARD", &format!("{now_str}  {status_label}"), Color::White);

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
            status_to_line("lists", systems.lists),
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
    let footer_text = "Q: Quit | 1: Tasks | 2: Notes | 3: Project | 4: Lists | R: Refresh";
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::White));
    frame.render_widget(footer, footer_area);
}


fn draw_todo_screen(frame: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let open_count = app.todo_items.iter().filter(|t| !t.completed).count();
    let done_count = app.todo_items.iter().filter(|t| t.completed).count();
    let summary = format!("{} open  {}  completed", open_count, done_count);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let (header_area, body) = (outer[0], outer[1]);
    draw_section_header(frame, header_area, "TASKS", &summary, Color::Blue);

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(body);

    let (list_area, details_area, footer_area) = (main_chunks[0], main_chunks[1], main_chunks[2]);

    // --- 1. Todo List Rendering ---
    let visible_indices = app.visible_todo_indices();
    let items: Vec<ListItem> = visible_indices.iter().map(|&i| {
        let item = &app.todo_items[i];
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
        
        let project_str = item.project_title.as_deref().unwrap_or("");
        let metadata = format!(
            " ({}, {} | Created: {}{})",
            priority_str, due_date_str, created_str,
            if project_str.is_empty() { String::new() } else { format!(" | {}", project_str) }
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
        TodoEditMode::Normal => {
            if app.todo_hide_completed {
                "TODO List [F: Show All] (A: Add | C: Toggle Complete | P: Print | X: Archive | D: Delete | J/K: Navigate)"
            } else {
                "TODO List [F: Hide Done] (A: Add | C: Toggle Complete | P: Print | X: Archive | D: Delete | J/K: Navigate)"
            }
        }
        TodoEditMode::Adding => "TODO List (Adding Item)",
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title)
            .border_style(Style::default().fg(Color::Blue)))
        .highlight_style(Style::default().bg(Color::Blue).fg(Color::White));

    frame.render_stateful_widget(list, list_area, &mut app.todo_list_state);

    // --- 2. Details Panel Rendering ---

    let details_block = Block::default().borders(Borders::ALL).title("Item Details")
        .border_style(Style::default().fg(Color::Blue));
    
    if let Some(vis_index) = app.todo_list_state.selected() {
        if let Some(item) = visible_indices.get(vis_index).and_then(|&i| app.todo_items.get(i)) {
            let mut text_lines = vec![
                Line::from(format!("ID: {}", item.id.unwrap_or(0))),
                Line::from(format!("Title: {}", item.title)).bold(),
                Line::from(format!("Status: {}", if item.completed { "COMPLETED" } else { "PENDING" })),
                Line::from(format!("Priority: {}", item.priority)).fg(if item.priority >= 8 { Color::Red } else { Color::White }),
                Line::from(format!("Project: {}", item.project_title.as_deref().unwrap_or("—"))),
            ];

            if let Some(due_date) = item.due_date {
                text_lines.push(Line::from(format!("Due Date: {}", due_date.format("%Y-%m-%d %H:%M:%S"))).fg(Color::Yellow));
            } else {
                text_lines.push(Line::from("Due Date: None"));
            }
            
            text_lines.push(Line::from(""));
            
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
    let footer_text = "Q: Back | R: Refresh | C: Toggle Complete | A: Add New | P: Print | X: Archive | D: Delete";
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::Blue));
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
    let height = 31; // 29 inner lines + 2 borders (accommodates calendar + completed field)
    let x = (parent_area.width.saturating_sub(width)) / 2;
    let y = (parent_area.height.saturating_sub(height)) / 2;
    let area = Rect::new(x, y, width, height);

    let mode_indicator = match app.input_mode {
        InputMode::Normal => "NORMAL (i/Enter: Insert | j/k/h/l: Navigate | </>: Month Nav)",
        InputMode::Insert => "INSERT (Esc/Ctrl+C: Normal | Enter: Newline in Subtasks)",
    };

    let title = format!("Add New Todo Item | {}", mode_indicator);

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
        vec![
            Constraint::Length(2), // 0: Title
            Constraint::Length(4), // 1: Description
            Constraint::Length(4), // 2: Subtasks
            Constraint::Length(2), // 3: Due By Toggle
            Constraint::Length(1), // 4: Date Label
            Constraint::Length(1), // 5: Date Input
            Constraint::Length(9), // 6: Calendar Grid
            Constraint::Length(2), // 7: Calendar Time
            Constraint::Length(2), // 8: Priority
            Constraint::Length(1), // 9: Spacer
            Constraint::Length(1), // 10: Submit button
        ]
    } else {
        vec![
            Constraint::Length(2), // 0: Title
            Constraint::Length(4), // 1: Description
            Constraint::Length(4), // 2: Subtasks
            Constraint::Length(2), // 3: Due By Toggle
            Constraint::Length(1), // 4: Date Label
            Constraint::Length(1), // 5: Date Input
            Constraint::Length(0), // 6: Calendar Grid (collapsed)
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

    // --- Description Input (Multiline) ---
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
    
    let submit_text = " [ SUBMIT ] ";
    
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

// --- Lists Screen ---

fn draw_lists_screen(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let summary = format!(
        "{} groups  {} lists  {} items",
        app.list_groups.len(),
        app.list_categories.len(),
        app.list_items.len(),
    );

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(3), Constraint::Length(3)])
        .split(area);
    let (header_area, main_area, footer_area) = (outer[0], outer[1], outer[2]);
    draw_section_header(frame, header_area, "LISTS", &summary, Color::Green);

    // Three-column layout: Groups | Categories | Items
    let panels = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(30),
            Constraint::Percentage(50),
        ])
        .split(main_area);

    let group_area   = panels[0];
    let cat_area     = panels[1];
    let item_area    = panels[2];

    let focused_style   = Style::default().fg(Color::Green);
    let unfocused_style = Style::default().fg(Color::Rgb(110, 110, 110));

    // --- Groups panel ---
    let group_items: Vec<ListItem> = app.list_groups.iter().map(|g| {
        ListItem::new(g.name.clone())
    }).collect();

    let group_list = List::new(group_items)
        .block(Block::default().borders(Borders::ALL).title("Groups")
            .border_style(if app.lists_focus == ListsFocus::Groups { focused_style } else { unfocused_style }))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green))
        .highlight_symbol("> ");
    frame.render_stateful_widget(group_list, group_area, &mut app.list_group_state);

    // --- Categories panel ---
    let cat_items: Vec<ListItem> = app.list_categories.iter().map(|c| {
        ListItem::new(c.name.clone())
    }).collect();

    let cat_list = List::new(cat_items)
        .block(Block::default().borders(Borders::ALL).title("Lists")
            .border_style(if app.lists_focus == ListsFocus::Categories { focused_style } else { unfocused_style }))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green))
        .highlight_symbol("> ");
    frame.render_stateful_widget(cat_list, cat_area, &mut app.list_category_state);

    // --- Items panel ---
    let category_name = app.list_category_state.selected()
        .and_then(|i| app.list_categories.get(i))
        .map(|c| c.name.as_str())
        .unwrap_or("—");

    let item_items: Vec<ListItem> = app.list_items.iter().map(|item| {
        let marker = if item.checked { "[x]" } else { "[ ]" };
        let label = match &item.quantity {
            Some(q) => format!("{} {} ({})", marker, item.name, q),
            None    => format!("{} {}",      marker, item.name),
        };
        let style = if item.checked {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        ListItem::new(label).style(style)
    }).collect();

    let item_title = format!("Items — {}", category_name);
    let item_list = List::new(item_items)
        .block(Block::default().borders(Borders::ALL).title(item_title)
            .border_style(if app.lists_focus == ListsFocus::Items { focused_style } else { unfocused_style }))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green))
        .highlight_symbol("> ");
    frame.render_stateful_widget(item_list, item_area, &mut app.list_item_state);

    // --- Footer / input bar ---
    let footer_content = match app.lists_input_mode {
        ListsInputMode::AddingGroup    => format!("New group name: {}_",  app.lists_input_buffer),
        ListsInputMode::AddingCategory => format!("New list name: {}_",   app.lists_input_buffer),
        ListsInputMode::AddingItem     => format!("New item: {}_",         app.lists_input_buffer),
        ListsInputMode::QuickAdd       =>
            "Quick Add — j/k:Navigate | Enter:Add to list | d:Delete template | Esc:Close".to_string(),
        ListsInputMode::Normal =>
            "Q:Back | Tab:Focus | j/k:Nav | a:Add | d:Del | Space:Check | C:Clear | p:Print | s:Save common | A:Quick add | r:Refresh".to_string(),
    };

    let footer_style = Style::default().fg(Color::Green);
    let footer = Paragraph::new(footer_content)
        .style(footer_style)
        .block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, footer_area);

    // --- Quick Add overlay ---
    if app.lists_input_mode == ListsInputMode::QuickAdd {
        draw_quick_add_overlay(frame, app, item_area);
    }
}

fn draw_quick_add_overlay(frame: &mut ratatui::Frame, app: &mut App, anchor: Rect) {
    // Centre a popup within the items panel area
    let height = (app.common_items.len() as u16 + 2).max(5).min(anchor.height.saturating_sub(2));
    let width = anchor.width.saturating_sub(4);
    let x = anchor.x + 2;
    let y = anchor.y + (anchor.height.saturating_sub(height)) / 2;
    let popup_area = Rect { x, y, width, height };

    // Clear background
    frame.render_widget(Clear, popup_area);

    if app.common_items.is_empty() {
        let msg = Paragraph::new("No saved items.  Press 's' on an item to save it as a template.")
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL)
                .title(" Quick Add ")
                .border_style(Style::default().fg(Color::Green)));
        frame.render_widget(msg, popup_area);
        return;
    }

    let items: Vec<ListItem> = app.common_items.iter().map(|c| {
        let label = match &c.quantity {
            Some(q) => format!("{} ({})", c.name, q),
            None    => c.name.clone(),
        };
        ListItem::new(label)
    }).collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL)
            .title(" Quick Add — Enter to add, d to delete ")
            .border_style(Style::default().fg(Color::Green)))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Green))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, popup_area, &mut app.common_item_state);
}

fn draw_notes_create(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title
            Constraint::Length(3), // tags
            Constraint::Length(3), // folder
            Constraint::Min(5),    // content
            Constraint::Length(1), // footer
        ])
        .split(area);

    let focused_style = Style::default().fg(Color::Yellow);
    let normal_style = Style::default().fg(Color::Rgb(110, 110, 110));

    let field_style = |f: NotesCreateFocus| {
        if app.notes_create_focus == f { focused_style } else { normal_style }
    };

    // Title
    let title_text = format!("{}_", app.notes_create_title);
    frame.render_widget(
        Paragraph::new(title_text)
            .block(Block::default().borders(Borders::ALL).title(" Title (optional) ").border_style(field_style(NotesCreateFocus::Title))),
        chunks[0],
    );

    // Folder
    let folder_text = format!("{}_", app.notes_create_folder);
    frame.render_widget(
        Paragraph::new(folder_text)
            .block(Block::default().borders(Borders::ALL).title(" Folder (optional) ").border_style(field_style(NotesCreateFocus::Folder))),
        chunks[1],
    );

    // Tags
    let tags_text = format!("{}_", app.notes_create_tags);
    frame.render_widget(
        Paragraph::new(tags_text)
            .block(Block::default().borders(Borders::ALL).title(" Tags (comma-separated, optional) ").border_style(field_style(NotesCreateFocus::Tags))),
        chunks[2],
    );

    // Content
    let content_display = if app.notes_create_focus == NotesCreateFocus::Content {
        format!("{}_", app.notes_create_content)
    } else {
        app.notes_create_content.clone()
    };
    frame.render_widget(
        Paragraph::new(content_display)
            .block(Block::default().borders(Borders::ALL).title(" Content * ").border_style(field_style(NotesCreateFocus::Content)))
            .wrap(Wrap { trim: false }),
        chunks[3],
    );

    // Footer
    frame.render_widget(
        Paragraph::new("Tab: next field  Ctrl+S: save  Esc: cancel")
            .style(Style::default().fg(Color::Rgb(160, 160, 160))),
        chunks[4],
    );
}

fn draw_notes_screen(frame: &mut ratatui::Frame, app: &mut App, area: Rect) {
    if app.notes_mode == NotesMode::Create {
        return draw_notes_create(frame, app, area);
    }

    let raw_count = app.notes.iter().filter(|n| n.status == NoteStatus::Raw).count();
    let note_count = app.notes.iter().filter(|n| n.status == NoteStatus::Note).count();
    let art_count = app.notes.iter().filter(|n| n.status == NoteStatus::Article).count();
    let summary = format!("{} raw  {} note  {} article", raw_count, note_count, art_count);

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    let (header_area, body) = (outer[0], outer[1]);
    draw_section_header(frame, header_area, "NOTES", &summary, Color::Yellow);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(body);
    let (filter_area, content_area, footer_area) = (chunks[0], chunks[1], chunks[2]);

    // Filter / search bar
    let all_filters = [
        NotesFilterStatus::All,
        NotesFilterStatus::Raw,
        NotesFilterStatus::NoteStatus,
        NotesFilterStatus::Article,
    ];
    let filter_label = format!(
        "  Filter: {}  {}",
        all_filters.iter()
            .map(|f| {
                let active = f == &app.notes_filter;
                if active { format!("[{}]", f.label()) } else { f.label().to_string() }
            })
            .collect::<Vec<_>>()
            .join("  "),
        if app.notes_mode == NotesMode::Search {
            format!("  Search: {}_", app.notes_search_buf)
        } else {
            format!("  {} notes", app.notes.len())
        }
    );
    frame.render_widget(
        Paragraph::new(filter_label).style(Style::default().fg(Color::Rgb(180, 180, 180))),
        filter_area,
    );

    // Content split: list (left) | viewer (right)
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(content_area);
    let (list_area, viewer_area) = (content_chunks[0], content_chunks[1]);

    // Note list
    let list_items: Vec<ListItem> = app.notes.iter().map(|note| {
        let status_tag = match note.status {
            NoteStatus::Raw => "[raw] ",
            NoteStatus::Note => "[note]",
            NoteStatus::Article => "[art] ",
        };
        let title = if note.title.is_empty() { "(untitled)" } else { &note.title };
        let folder_prefix = if !note.folder.is_empty() {
            format!("{}/", note.folder)
        } else {
            String::new()
        };
        let date = note.updated_at.format("%d %b").to_string();
        ListItem::new(Line::from(vec![
            ratatui::text::Span::styled(
                format!("{} ", status_tag),
                Style::default().fg(match note.status {
                    NoteStatus::Raw => Color::Magenta,
                    NoteStatus::Note => Color::Cyan,
                    NoteStatus::Article => Color::Green,
                }),
            ),
            ratatui::text::Span::raw(format!("{}{}", folder_prefix, title)),
            ratatui::text::Span::styled(
                format!("  {}", date),
                Style::default().fg(Color::Rgb(160, 160, 160)),
            ),
        ]))
    }).collect();

    let list_title = format!(" Notes ({}) ", app.notes.len());
    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(list_title)
        .border_style(Style::default().fg(
            if app.notes_mode == NotesMode::List { Color::Yellow } else { Color::Rgb(110, 110, 110) }
        ));
    let list_widget = List::new(list_items)
        .block(list_block)
        .highlight_style(Style::default().bg(Color::Yellow).fg(Color::Black));
    frame.render_stateful_widget(list_widget, list_area, &mut app.notes_list_state);

    // Note viewer
    let (viewer_title, viewer_content) = match &app.notes_view_note {
        Some(note) => {
            let tags = if note.tags.is_empty() {
                String::new()
            } else {
                format!("  tags: {}", note.tags.join(", "))
            };
            let folder = if !note.folder.is_empty() {
                format!("  folder: {}", note.folder)
            } else {
                String::new()
            };
            let header = format!(
                "status: [{}]{}{}  updated: {}\n{}\n",
                note.status.as_str(),
                folder,
                tags,
                note.updated_at.format("%d %b %Y %H:%M"),
                "─".repeat(viewer_area.width.saturating_sub(2) as usize),
            );
            let title = format!(" {} [{}] ",
                if note.title.is_empty() { "Untitled" } else { &note.title },
                note.status.as_str()
            );
            (title, format!("{}{}", header, note.content))
        }
        None => {
            let help = match app.notes_mode {
                NotesMode::ConfirmDelete => "Delete this note? Press y to confirm, any other key to cancel.".to_string(),
                _ => "  Select a note to view its content.\n\n\
                  Navigation:\n\
                  \u{2022} j / k or arrows — move selection\n\
                  \u{2022} Enter         — open note\n\
                  \u{2022} Tab           — cycle filter (All→Raw→Note→Article)\n\
                  \u{2022} /             — search\n\
                  \u{2022} a             — advance status\n\
                  \u{2022} d             — delete\n\
                  \u{2022} r             — refresh\n\
                  \u{2022} q             — back to dashboard\n\n\
                  In view mode:\n\
                  \u{2022} j / k         — scroll\n\
                  \u{2022} e             — edit in $NOTES_EDITOR / $EDITOR / vi\n\
                  \u{2022} a             — advance status\n\
                  \u{2022} q             — back to list".to_string(),
            };
            (" Notes ".to_string(), help)
        }
    };

    let viewer_block = Block::default()
        .borders(Borders::ALL)
        .title(viewer_title)
        .border_style(Style::default().fg(
            if app.notes_mode == NotesMode::View { Color::Yellow } else { Color::Rgb(110, 110, 110) }
        ));
    let viewer = Paragraph::new(viewer_content)
        .block(viewer_block)
        .wrap(Wrap { trim: false })
        .scroll((app.notes_scroll, 0));
    frame.render_widget(viewer, viewer_area);

    // Footer
    let footer_text = match app.notes_mode {
        NotesMode::List   => "j/k: move  Enter: open  Tab: filter  /: search  n: new  a: advance  d: delete  r: refresh  q: back",
        NotesMode::View   => "j/k: scroll  e: edit  a: advance status  p: print  d: delete  q: back to list",
        NotesMode::Search => "Type to search  Enter: run  Esc: cancel",
        NotesMode::Create => "Tab: next field  Ctrl+S: save  Esc: cancel",
        NotesMode::ConfirmDelete => "y: confirm delete  any other key: cancel",
    };
    frame.render_widget(
        Paragraph::new(footer_text).style(Style::default().fg(Color::Yellow)),
        footer_area,
    );

    // Delete confirmation overlay
    if app.notes_mode == NotesMode::ConfirmDelete {
        let popup = Rect {
            x: area.width.saturating_sub(50) / 2,
            y: area.height.saturating_sub(5) / 2,
            width: 50.min(area.width),
            height: 5.min(area.height),
        };
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new("Delete this note? (y = confirm, any other key = cancel)")
                .block(Block::default().borders(Borders::ALL).title(" Delete Note ")
                    .border_style(Style::default().fg(Color::Red)))
                .wrap(Wrap { trim: true }),
            popup,
        );
    }
}

fn draw_project_placeholder(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    draw_section_header(frame, outer[0], "PROJECT", "Coming soon", Color::Magenta);
    let text = Paragraph::new("\nPress Q or Esc to go back")
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)));
    frame.render_widget(text, outer[1]);
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
///
/// The API base URL is read from the `MANAGE_API_URL` environment variable,
/// defaulting to `http://127.0.0.1:8080`.
///
/// Examples:
///   Running against local server:  (no env var needed)
///   Running against Docker Compose: MANAGE_API_URL=http://localhost cargo run -p tui
pub async fn run_tui() -> Result<()> {
    let mut tui = Tui::new()?;
    let api_url = std::env::var("MANAGE_API_URL")
        .unwrap_or_else(|_| "http://localhost".to_string());
    let api_client = ApiClient::new(&api_url);
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
