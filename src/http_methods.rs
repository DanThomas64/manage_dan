use reqwest::Response;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::env;

use crate::datatypes::*;

pub async fn auth(url: String) -> Result<Response, Box<dyn std::error::Error>> {
    let username = env::var("USERNAME").expect("Unable to find USERNAME env");
    let password = env::var("PASSWORD").expect("Unable to find PASSWORD env");

    let user = RequestLogin {
        long_token: true,
        password: password,
        totp_passcode: "".to_string(),
        username: username,
    };

    let request_url = format!("{}/login", url);

    let client = reqwest::Client::new();
    let response = client
        .post(request_url)
        .header(CONTENT_TYPE, "application/json")
        .json(&user)
        .send()
        .await;
    Ok(response?)
}

pub async fn get_request(url: String, auth: Auth, json: String) -> Result<Response, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", auth.token))
        .body(json)
        .send()
        .await;
    Ok(response?)
}
