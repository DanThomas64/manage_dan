use html2text;
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct RequestLogin {
    pub long_token: bool,
    pub password: String,
    pub totp_passcode: String,
    pub username: String,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct RequestAllProjects {
    pub page: u8,
    pub per_page: u8,
    pub s: String,
    pub is_archived: bool,
}

#[derive(Deserialize, Debug)]
pub struct Auth {
    pub token: String,
}

#[derive(Deserialize, Debug)]
pub struct User {
    pub id: i32,
    pub name: String,
    pub username: String,
}

#[derive(Deserialize, Debug)]
pub struct Bucket {
    pub filter: String,
    pub title: String,
}

#[derive(Deserialize, Debug)]
pub struct View {
    pub bucket_configuration: Option<Vec<Bucket>>,
    pub bucket_configuration_mode: String,
    pub created: String,
    pub default_bucket_id: i32,
    pub done_bucket_id: i32,
    pub filter: String,
    pub id: i32,
    pub position: f32,
    pub project_id: i32,
    pub title: String,
    pub updated: String,
    pub view_kind: String,
}

#[derive(Deserialize, Debug)]
pub struct Subscription {
    pub created: String,
    pub entity: u32,
    pub entity_id: i32,
    pub id: i32,
}

#[derive(Deserialize, Debug)]
pub struct Project {
    pub background_blur_hash: String,
    pub background_information: Option<String>,
    pub created: String,
    pub description: String,
    pub hex_color: String,
    pub id: i32,
    pub identifier: String,
    pub is_archived: bool,
    pub is_favorite: bool,
    pub owner: User,
    pub parent_project_id: i32,
    pub position: f32,
    pub subscription: Option<Subscription>,
    pub title: String,
    pub updated: String,
    pub views: Vec<View>,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct RequestAllTasks {
    pub page: u8,
    pub per_page: u8,
    pub s: String,
    pub done: bool,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Label {
    pub title: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ReminderConfig {
    pub relative_period: u8,
    pub relative_to: String,
    pub reminder: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Task {
    pub id: i32,
    pub title: String,
    pub description: String,
    pub updated: String,
    pub done: bool,
    pub labels: Option<Vec<Label>>,
    pub project_id: i32,
    pub due_date: String,
    pub reminders: Option<Vec<ReminderConfig>>,
}

impl Task {
    pub fn description_as_text(&self, width: usize) -> String {
        html2text::from_read(self.description.as_bytes(), width)
    }

    pub fn is_recurring(&self) -> bool {
        const RECURRING_LABELS: &[&str] = &[
            "Daily",
            "Weekly",
            "Monthly",
            "Bi-Weekly",
            "Quarterly",
            "Yearly/Beyond",
        ];
        if let Some(labels) = &self.labels {
            labels
                .iter()
                .any(|label| RECURRING_LABELS.contains(&label.title.as_str()))
        } else {
            false
        }
    }
}
