use anyhow::{Context, Result};
use dotenv::dotenv;
use std::collections::HashSet;
use std::env;
use tokio::time::{sleep, Duration};

mod datatypes;
mod http_methods;
mod escpos;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let api_base_url = env::var("API_URL").context("API_URL environment variable not set")?;
    let printer_device =
        env::var("PRINTER_DEVICE").context("PRINTER_DEVICE environment variable not set")?;

    // Get the auth token
    let auth: datatypes::Auth = http_methods::auth(api_base_url.clone())
        .await
        .context("Failed to get auth token")?
        .json()
        .await
        .context("Failed to parse auth token from response")?;

    let mut printed_task_ids = HashSet::new();

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

        for task in uncompleted_tasks {
            if !printed_task_ids.contains(&task.id) {
                println!("Found new task, printing: \"{}\"", task.title);
                match escpos::print_task(&task, &printer_device) {
                    Ok(_) => {
                        printed_task_ids.insert(task.id);
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
