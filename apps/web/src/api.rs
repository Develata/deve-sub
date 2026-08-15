//! REST API client for `/api/v1/*` endpoints.
//!
//! Thin wrapper around `web_sys::fetch` with typed (de)serialization.
//! Cookie-based auth is sent automatically by the browser (`credentials:
//! same-origin`).

#![cfg(target_family = "wasm")]

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, RequestInit, Response};

const API_BASE: &str = "/api/v1";

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

fn fetch_err(msg: &str) -> ApiError {
    ApiError {
        status: 0,
        code: "fetch_error".to_string(),
        message: msg.to_string(),
    }
}

pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, ApiError> {
    let url = format!("{API_BASE}{path}");
    let mut init = RequestInit::new();
    init.set_method("GET");
    init.set_credentials(web_sys::RequestCredentials::SameOrigin);
    let response = js_fetch(&url, &init).await?;
    let status = response.status();
    if !response.ok() {
        return Err(parse_error(status, &response).await);
    }
    let json = response_json(&response).await?;
    serde_json::from_value::<T>(json).map_err(|e| ApiError {
        status,
        code: "parse_error".to_string(),
        message: e.to_string(),
    })
}

pub async fn send<T: DeserializeOwned, B: Serialize>(
    method: &str,
    path: &str,
    body: Option<&B>,
) -> Result<T, ApiError> {
    let url = format!("{API_BASE}{path}");
    let mut init = RequestInit::new();
    init.set_method(method);
    init.set_credentials(web_sys::RequestCredentials::SameOrigin);

    if let Some(b) = body {
        let json = serde_json::to_string(b).map_err(|e| ApiError {
            status: 0,
            code: "serialize_error".to_string(),
            message: e.to_string(),
        })?;
        init.set_body(&wasm_bindgen::JsValue::from_str(&json));
        let headers = Headers::new().map_err(|_| fetch_err("failed to create headers"))?;
        headers
            .set("Content-Type", "application/json")
            .map_err(|_| fetch_err("failed to set content-type"))?;
        init.set_headers(&headers);
    }

    let response = js_fetch(&url, &init).await?;
    let status = response.status();
    if !response.ok() {
        return Err(parse_error(status, &response).await);
    }

    if status == 204 {
        return serde_json::from_str("null").map_err(|e| ApiError {
            status,
            code: "parse_error".to_string(),
            message: e.to_string(),
        });
    }

    let json = response_json(&response).await?;
    serde_json::from_value::<T>(json).map_err(|e| ApiError {
        status,
        code: "parse_error".to_string(),
        message: e.to_string(),
    })
}

pub async fn delete(path: &str) -> Result<(), ApiError> {
    let url = format!("{API_BASE}{path}");
    let mut init = RequestInit::new();
    init.set_method("DELETE");
    init.set_credentials(web_sys::RequestCredentials::SameOrigin);
    let response = js_fetch(&url, &init).await?;
    if !response.ok() {
        return Err(parse_error(response.status(), &response).await);
    }
    Ok(())
}

pub mod auth {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SetupAdminRequest {
        pub username: String,
        pub password: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UserDto {
        pub id: String,
        pub username: String,
        pub role: String,
        pub enabled: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub expires_at: Option<String>,
        pub traffic_quota: u64,
        pub two_factor_enabled: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub last_login_at: Option<String>,
        pub created_at: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SetupAdminResponse {
        pub user: UserDto,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LoginRequest {
        pub username: String,
        pub password: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LoginResponse {
        pub user: UserDto,
        pub requires_2fa: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub challenge_token: Option<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CurrentUserResponse {
        pub user: UserDto,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct AuthStatusResponse {
        pub initialized: bool,
    }

    pub async fn setup(username: &str, password: &str) -> Result<SetupAdminResponse, ApiError> {
        send(
            "POST",
            "/auth/setup",
            Some(&SetupAdminRequest {
                username: username.to_string(),
                password: password.to_string(),
            }),
        )
        .await
    }

    pub async fn login(username: &str, password: &str) -> Result<LoginResponse, ApiError> {
        send(
            "POST",
            "/auth/login",
            Some(&LoginRequest {
                username: username.to_string(),
                password: password.to_string(),
            }),
        )
        .await
    }

    pub async fn logout() -> Result<(), ApiError> {
        send::<(), _>("POST", "/auth/logout", None::<&Value>).await
    }

    pub async fn me() -> Result<CurrentUserResponse, ApiError> {
        get("/auth/me").await
    }

    pub async fn status() -> Result<AuthStatusResponse, ApiError> {
        get("/auth/status").await
    }
}

async fn js_fetch(url: &str, init: &RequestInit) -> Result<Response, ApiError> {
    let window = web_sys::window().ok_or(fetch_err("no global window"))?;
    let request = web_sys::Request::new_with_str_and_init(url, init)
        .map_err(|_| fetch_err("failed to construct request"))?;
    let promise = window.fetch_with_request(&request);
    let result = JsFuture::from(promise).await.map_err(|e| ApiError {
        status: 0,
        code: "fetch_error".to_string(),
        message: format!("{e:?}"),
    })?;
    result
        .dyn_into::<Response>()
        .map_err(|_| fetch_err("invalid response type"))
}

async fn response_json(response: &Response) -> Result<Value, ApiError> {
    let promise = response.text().map_err(|_| ApiError {
        status: response.status(),
        code: "parse_error".to_string(),
        message: "failed to read response body".to_string(),
    })?;
    let result = JsFuture::from(promise).await.map_err(|e| ApiError {
        status: response.status(),
        code: "parse_error".to_string(),
        message: format!("{e:?}"),
    })?;
    let text = js_sys::JsString::from(result)
        .as_string()
        .unwrap_or_default();
    if text.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| ApiError {
        status: response.status(),
        code: "parse_error".to_string(),
        message: e.to_string(),
    })
}

async fn parse_error(status: u16, response: &Response) -> ApiError {
    match response_json(response).await {
        Ok(v) => {
            let code = v
                .get("error")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            let message = v
                .get("message")
                .and_then(|s| s.as_str())
                .unwrap_or("request failed")
                .to_string();
            ApiError {
                status,
                code,
                message,
            }
        }
        Err(_) => ApiError {
            status,
            code: "http_error".to_string(),
            message: format!("HTTP {status}"),
        },
    }
}
