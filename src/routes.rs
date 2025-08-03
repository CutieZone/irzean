use std::sync::Arc;

use axum::{
    extract::{Path as UriPath, State},
    http::{Method, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use color_eyre::eyre::OptionExt;

use crate::{AppState, err::Error};

mod templates;

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn index(s: State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/index.jinja")?;

    let rendered = tmpl.render(Option::<()>::None)?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn writing(
    uri: Uri,
    UriPath(path): UriPath<String>,
    s: State<Arc<AppState>>,
) -> Result<Response, Error> {
    let writing = s.get_writing(&path).await;

    let Some(writing) = writing else {
        let resp = not_found(uri, s).await?;

        return Ok(resp.into_response());
    };

    let tmpl = s.jinja_env.get_template("html/writing.jinja")?;

    let rendered = tmpl.render(templates::Reader::new(writing))?;

    Ok(Html(rendered).into_response())
}

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn tags(s: State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/tags.jinja")?;

    let rendered = tmpl.render(templates::Tags::new(
        s.repo_handler.read().await.tag_list().await?,
    ))?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
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
        .get_writing_metas()
        .await
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
#[tracing::instrument(skip(s))]
pub async fn list(s: State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/list.jinja")?;

    let writings = s.get_writing_metas().await;

    let rendered = tmpl.render(templates::List::new(writings))?;

    Ok(Html(rendered))
}

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn not_found(
    uri: Uri,
    s: State<Arc<AppState>>,
) -> Result<(StatusCode, Html<String>), Error> {
    let tmpl = s.jinja_env.get_template("html/error.jinja")?;

    let rendered = tmpl.render(templates::Error::not_found(uri.path()))?;

    Ok((StatusCode::NOT_FOUND, Html(rendered)))
}

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn method_not_allowed(
    method: Method,
    uri: Uri,
    s: State<Arc<AppState>>,
) -> Result<(StatusCode, Html<String>), Error> {
    let tmpl = s.jinja_env.get_template("html/error.jinja")?;

    let rendered = tmpl.render(templates::Error::method_not_allowed(&method, uri.path()))?;

    Ok((StatusCode::METHOD_NOT_ALLOWED, Html(rendered)))
}
