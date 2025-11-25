use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use dotenv::dotenv;
use std::env;
use tokio::time::{sleep, Duration};

mod datatypes;
mod http_methods;
mod escpos;
mod database;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let api_base_url = env::var("API_URL").context("API_URL environment variable not set")?;
    let printer_device =
        env::var("PRINTER_DEVICE").context("PRINTER_DEVICE environment variable not set")?;
    let db_path = env::var("DATABASE_PATH").unwrap_or_else(|_| "tasks.db".to_string());


    // Setup database
    let conn = database::init_db(&db_path)?;

    // Get the auth token
    let auth: datatypes::Auth = http_methods::auth(api_base_url.clone())
        .await
        .context("Failed to get auth token")?
        .json()
        .await
        .context("Failed to parse auth token from response")?;

    let mut last_summary_print_date = Local::now().date_naive().pred_opt().unwrap();

    loop {
        // Now that we have the token lets get some info from the api.
        // Lets get a list of uncompleted tasks.
        // NOTE: The endpoint is a guess, you may need to adjust it.
        let get_string = format!("{}/tasks/all", api_base_url);
        let all_tasks_request = datatypes::RequestAllTasks {
            page: 1,
            per_page: 50,
            s: "".to_string(),
            done: false,
        };
        let json_str = serde_json::to_string(&all_tasks_request)?;
        println!("Fetching tasks...");

        let response = match http_methods::get_request(get_string, &auth, Some(json_str)).await {
            Ok(resp) => resp,
            Err(e) => {
                eprintln!("Failed to request tasks from API: {}. Retrying in 60s.", e);
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let tasks: Vec<datatypes::Task> = match response.json().await {
            Ok(tasks) => tasks,
            Err(e) => {
                eprintln!(
                    "Failed to parse tasks from API response: {}. Retrying in 60s.",
                    e
                );
                sleep(Duration::from_secs(60)).await;
                continue;
            }
        };

        let uncompleted_tasks: Vec<datatypes::Task> =
            tasks.into_iter().filter(|t| !t.done).collect();

        let today = Local::now().date_naive();
        if today > last_summary_print_date {
            println!("Printing daily summary for {}", today);

            let daily_tasks: Vec<datatypes::Task> = uncompleted_tasks
                .iter()
                .filter(|task| {
                    let has_today_label = task.has_label("Today");
                    let has_tomorrow_label = task.has_label("Tomorrow");

                    let due_date = if let Ok(dt) = DateTime::parse_from_rfc3339(&task.due_date) {
                        Some(dt.with_timezone(&Local).date_naive())
                    } else {
                        None
                    };
                    let is_due_today = due_date == Some(today);

                    is_due_today || has_today_label || has_tomorrow_label
                })
                .cloned()
                .collect();

            if !daily_tasks.is_empty() {
                if let Err(e) = escpos::print_daily_summary(&daily_tasks, &printer_device) {
                    eprintln!("Failed to print daily summary: {}", e);
                }
            }
            last_summary_print_date = today;
        }


        for task in uncompleted_tasks {
            if task.is_recurring() {
                if let Ok(due_date) = DateTime::parse_from_rfc3339(&task.due_date) {
                    let now = Utc::now();
                    if due_date.with_timezone(&Utc) > now {
                        println!(
                            "Skipping recurring task for the future: \"{}\" (due: {})",
                            task.title,
                            due_date.with_timezone(&Local).format("%Y-%m-%d %H:%M")
                        );
                        continue;
                    }
                }
            }

            if database::needs_printing(&conn, &task)? {
                println!(
                    "Found new or updated task, printing: \"{}\"",
                    task.title
                );
                match escpos::print_task(&task, &printer_device) {
                    Ok(_) => {
                        if let Err(e) = database::mark_as_printed(&conn, &task) {
                            eprintln!("Failed to mark task {} as printed: {}", task.id, e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to print task {}: {}", task.id, e);
                    }
                }
            }
        }

        println!("Waiting for 60 seconds before next check...");
        sleep(Duration::from_secs(60)).await;
    }
}
