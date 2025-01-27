use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as UriPath, Query, State},
    http::{Method, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
};
use color_eyre::{Report, eyre::OptionExt};
use serde::Deserialize;
use tracing::{debug, warn};

use crate::{
    AppState,
    err::Error,
    util::{Statics, tokio_fs::TokioFs},
};

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
    let writing = s.repo_handler.read().await.get_writing(&path).await;

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

    let rendered = tmpl.render(templates::Tags::new(
        s.repo_handler.read().await.tag_list().await?,
    ))?;

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
        .read()
        .await
        .tag_list()
        .await?
        .get(&name)
        .ok_or_eyre(format!("No tag with name `{name}` found"))?;

    let writings_with_tag = s
        .repo_handler
        .read()
        .await
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

    let writings = s.repo_handler.read().await.file_list().await?;

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

#[derive(Debug, Deserialize)]
pub struct CacheBust {
    pub v: String,
}

#[axum::debug_handler]
pub async fn style(
    UriPath(name): UriPath<String>,
    Query(v): Query<CacheBust>,
    uri: Uri,
    s: State<Arc<AppState>>,
) -> Result<Response, Error> {
    if v.v != s.css_hash {
        return Ok(Redirect::permanent(&format!("/style/{name}?v={}", s.css_hash)).into_response());
    }

    let base_path = format!("style/{name}").replace(".css", ".scss");

    if let Some(data) = Statics::get(&base_path) {
        let string = String::from_utf8(data.data.to_vec()).map_err(Report::from)?;
        let rendered = grass::from_string(&string, &grass_options())?;

        Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/css")
            .header("cache-control", "public, max-age=31536000, immutable")
            .body(Body::from(rendered))
            .map_err(Into::into)
    } else {
        debug!(?base_path, ?name, "Couldn't find a style there...");
        Ok(not_found(uri, s).await.into_response())
    }
}

fn grass_options() -> grass::Options<'static> {
    grass::Options::default().fs(&TokioFs)
}
