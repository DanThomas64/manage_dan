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

#[derive(Serialize, Debug)]
pub struct UpdateTask {
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

    pub fn has_label(&self, label_title: &str) -> bool {
        if let Some(labels) = &self.labels {
            labels.iter().any(|label| label.title == label_title)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_task(id: i32, labels: Option<Vec<Label>>) -> Task {
        Task {
            id,
            title: "Test Task".to_string(),
            description: "Test Description".to_string(),
            updated: "2025-01-01T12:00:00Z".to_string(),
            done: false,
            labels,
            project_id: 1,
            due_date: "2025-01-02T12:00:00Z".to_string(),
            reminders: None,
        }
    }

    #[test]
    fn test_is_recurring() {
        let recurring_labels = vec![
            Label {
                title: "Daily".to_string(),
            },
            Label {
                title: "Weekly".to_string(),
            },
            Label {
                title: "Monthly".to_string(),
            },
            Label {
                title: "Bi-Weekly".to_string(),
            },
            Label {
                title: "Quarterly".to_string(),
            },
            Label {
                title: "Yearly/Beyond".to_string(),
            },
        ];

        for label in recurring_labels {
            let task = create_test_task(1, Some(vec![label]));
            assert!(task.is_recurring());
        }

        let non_recurring_task =
            create_test_task(2, Some(vec![Label {
                title: "Urgent".to_string(),
            }]));
        assert!(!non_recurring_task.is_recurring());

        let task_no_labels = create_test_task(3, None);
        assert!(!task_no_labels.is_recurring());
    }

    #[test]
    fn test_has_label() {
        let task_with_label = create_test_task(1, Some(vec![Label {
            title: "Today".to_string(),
        }]));
        assert!(task_with_label.has_label("Today"));
        assert!(!task_with_label.has_label("Tomorrow"));

        let task_with_multiple_labels = create_test_task(
            2,
            Some(vec![
                Label {
                    title: "Today".to_string(),
                },
                Label {
                    title: "Urgent".to_string(),
                },
            ]),
        );
        assert!(task_with_multiple_labels.has_label("Today"));
        assert!(task_with_multiple_labels.has_label("Urgent"));
        assert!(!task_with_multiple_labels.has_label("Weekly"));

        let task_no_labels = create_test_task(3, None);
        assert!(!task_no_labels.has_label("Today"));
    }
}
