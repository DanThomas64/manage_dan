use anyhow::Result;
use reqwest::Response;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::env;

use crate::datatypes::*;

pub async fn auth(url: String) -> Result<Response> {
    let username = env::var("USERNAME").expect("Unable to find USERNAME env");
    let password = env::var("PASSWORD").expect("Unable to find PASSWORD env");

    let user = RequestLogin {
        long_token: true,
        password,
        totp_passcode: "".to_string(),
        username,
    };

    let request_url = format!("{}/login", url);

    let client = reqwest::Client::new();
    let response = client
        .post(request_url)
        .header(CONTENT_TYPE, "application/json")
        .json(&user)
        .send()
        .await?;
    Ok(response)
}

pub async fn get_request(url: String, auth: &Auth, json: Option<String>) -> Result<Response> {
    let client = reqwest::Client::new();
    let mut request_builder = client
        .get(url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", auth.token));

    if let Some(json_str) = json {
        let params: serde_json::Value = serde_json::from_str(&json_str)?;
        request_builder = request_builder.query(&params);
    }

    let response = request_builder.send().await?;
    Ok(response)
}

pub async fn post_request(url: String, auth: &Auth, json_body: String) -> Result<Response> {
    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {}", auth.token))
        .body(json_body)
        .send()
        .await?;
    Ok(response)
}
