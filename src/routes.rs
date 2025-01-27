use std::{
    env,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Path as UriPath, State},
    http::{Method, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use color_eyre::eyre::OptionExt;
use tracing::{debug, warn};

use crate::{AppState, err::Error, util::tokio_fs::TokioFs};

mod templates;

#[axum::debug_handler]
pub async fn index(State(s): State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/index.jinja")?;

    let rendered = tmpl.render(Option::<()>::None)?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
pub async fn writing(
    uri: Uri,
    UriPath(path): UriPath<String>,
    s: State<Arc<AppState>>,
) -> Result<Response, Error> {
    let writing = s.repo_handler.get_writing(&path).await;

    let writing = match writing {
        Ok(writing) => writing,
        Err(e) => {
            warn!(?e, "Couldn't get writing.");
            let resp = not_found(uri, s).await?;

            return Ok(resp.into_response());
        }
    };

    let tmpl = s.jinja_env.get_template("html/writing.jinja")?;

    let rendered = tmpl.render(templates::Reader::new(writing)?)?;

    Ok(Html(rendered).into_response())
}

#[axum::debug_handler]
pub async fn tags(State(s): State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/tags.jinja")?;

    let rendered = tmpl.render(templates::Tags::new(s.repo_handler.tag_list().await?))?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
pub async fn specific_tag(
    UriPath(name): UriPath<String>,
    s: State<Arc<AppState>>,
) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/list.jinja")?;

    let _ = s
        .repo_handler
        .tag_list()
        .await?
        .get(&name)
        .ok_or_eyre(format!("No tag with name `{name}` found"))?;

    let writings_with_tag = s
        .repo_handler
        .file_list()
        .await?
        .into_iter()
        .filter(|v| {
            if name == "nsfw" {
                v.is_nsfw
            } else if name == "sfw" {
                !v.is_nsfw
            } else {
                v.tags.contains(&name.to_lowercase())
            }
        })
        .collect();

    let rendered = tmpl.render(templates::SpecificTag::new(&name, writings_with_tag))?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
pub async fn list(s: State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/list.jinja")?;

    let writings = s.repo_handler.file_list().await?;

    let rendered = tmpl.render(templates::List::new(writings))?;

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
    let base_path = static_path();
    let mut base_path: PathBuf = (base_path + "/style").into();
    base_path.push(&name);

    if check_for_traversal(&base_path) {
        warn!(?base_path, ?name, "Attempted path traversal. Blocking.");
        return Ok(not_found(uri, s).await.into_response());
    }

    base_path.set_extension("scss");

    if !base_path.exists() {
        debug!(?base_path, ?name, "Couldn't find a style there...");
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
    // We skip 1 because if it *is* a RootDir, that's fine. We just want to know if anywhere ELSE there's problems.
    path.components()
        .skip(1)
        .any(|x| matches!(x, Component::ParentDir | Component::RootDir))
}

fn grass_options() -> grass::Options<'static> {
    grass::Options::default().fs(&TokioFs)
}

fn static_path() -> String {
    env::var("IRZEAN_STATIC_DIR").unwrap_or_else(|_| "./static".to_string())
}
