use dotenv::dotenv;
use std::env;

mod datatypes;
mod http_methods;
mod gui;
mod escpos;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let api_base_url = env::var("API_URL").expect("Unable to find USERNAME env");
    let printer_device = env::var("PRINTER_DEVICE").expect("Unable to find PRINTER_DEVICE env");

    // Get the auth token
    let auth: datatypes::Auth = http_methods::auth(api_base_url.clone())
        .await
        .expect("Unable to get auth token from auth function")
        .json()
        .await
        .expect("Unable to parse token from auth function response");

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
    let json_str = serde_json::to_string(&json).expect("unable to turn struct into string");

    println!("{:?}", json_str);
    let tasks: Vec<datatypes::Task> = http_methods::get_request(get_string, auth, json_str)
        .await
        .expect("unable to complete get request")
        .json()
        .await
        .expect("Unable to parse the response");

    gui::tui(tasks, printer_device)?;

    Ok(())
}
