use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Path as UriPath, State},
    http::{Method, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};

use crate::{AppState, err::Error, util::tokio_fs::TokioFs};

mod templates;

pub async fn index(State(s): State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/index.jinja")?;

    let rendered = tmpl.render(Option::<()>::None)?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
pub async fn not_found(
    uri: Uri,
    State(s): State<Arc<AppState>>,
) -> Result<(StatusCode, Html<String>), Error> {
    let tmpl = s.jinja_env.get_template("html/error.jinja")?;

    let rendered = tmpl.render(templates::Error::not_found(uri.path()))?;

    Ok((StatusCode::NOT_FOUND, Html(rendered)))
}

#[axum::debug_handler]
pub async fn method_not_allowed(
    method: Method,
    uri: Uri,
    State(s): State<Arc<AppState>>,
) -> Result<(StatusCode, Html<String>), Error> {
    let tmpl = s.jinja_env.get_template("html/error.jinja")?;

    let rendered = tmpl.render(templates::Error::method_not_allowed(&method, uri.path()))?;

    Ok((StatusCode::METHOD_NOT_ALLOWED, Html(rendered)))
}

#[axum::debug_handler]
pub async fn style(
    UriPath(name): UriPath<String>,
    uri: Uri,
    s: State<Arc<AppState>>,
) -> Result<Response, Error> {
    let mut base_path = PathBuf::from(format!("./static/style/{name}"));

    if check_for_traversal(&base_path) {
        return Ok(not_found(uri, s).await.into_response());
    }

    base_path.set_extension("scss");

    if !base_path.exists() {
        return Ok(not_found(uri, s).await.into_response());
    }

    let rendered = grass::from_path(&base_path, &grass_options())?;

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/css")
        .body(Body::from(rendered))
        .map_err(Into::into)
}

/// Returns `true` if there's any path traversal attempts.
fn check_for_traversal(path: &Path) -> bool {
    path.components()
        .any(|x| matches!(x, Component::ParentDir | Component::RootDir))
}

fn grass_options() -> grass::Options<'static> {
    grass::Options::default().fs(&TokioFs)
}
