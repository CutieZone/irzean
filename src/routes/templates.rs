#![allow(clippy::struct_field_names)]

use axum::http::{Method, StatusCode};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Error {
    pub title: String,
    pub error: Option<String>,
    pub code: u16,
    pub reason: String,
    pub path: String,
}

impl Error {
    pub fn not_found(path: &str) -> Self {
        Self {
            title: "Not Found".to_string(),
            error: None,
            code: StatusCode::NOT_FOUND.as_u16(),
            reason: "The requested resource simply could not be found.".to_string(),
            path: path.to_string(),
        }
    }

    pub fn method_not_allowed(method: &Method, path: &str) -> Self {
        Self {
            title: "Method Not Allowed".to_string(),
            error: Some(format!("Method {method} Not Allowed")),
            code: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
            reason: format!("The method {method} is disallowed on this path."),
            path: path.to_string(),
        }
    }
}
