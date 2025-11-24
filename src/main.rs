use anyhow::{Context, Result};
use dotenv::dotenv;
use std::env;

mod datatypes;
mod http_methods;
mod gui;
mod escpos;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let api_base_url = env::var("API_URL").context("API_URL environment variable not set")?;
    let printer_device =
        env::var("PRINTER_DEVICE").context("PRINTER_DEVICE environment variable not set")?;
    let web_url =
        env::var("WEB_URL").unwrap_or_else(|_| "https://todo.dandoesthings.online".to_string());

    // Get the auth token
    let auth: datatypes::Auth = http_methods::auth(api_base_url.clone())
        .await
        .context("Failed to get auth token")?
        .json()
        .await
        .context("Failed to parse auth token from response")?;

    // Now that we have the token lets get some info from the api.
    // Lets get a list of uncompleted tasks.
    // NOTE: The endpoint is a guess, you may need to adjust it.
    let get_string = format!("{}/tasks", api_base_url);
    let json = datatypes::RequestAllTasks {
        page: 0,
        per_page: 50,
        s: "".to_string(),
        done: false,
    };
    let json_str = serde_json::to_string(&json).context("Failed to serialize tasks request")?;

    println!("{:?}", json_str);
    let tasks: Vec<datatypes::Task> = http_methods::get_request(get_string, auth, json_str)
        .await
        .context("Failed to request tasks from API")?
        .json()
        .await
        .context("Failed to parse tasks from API response")?;

    gui::tui(tasks, printer_device, web_url)?;

    Ok(())
}
