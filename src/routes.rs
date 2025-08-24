use std::{
    cmp::Ordering,
    fmt::{self, Write},
    mem,
    str::FromStr,
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{Path as UriPath, Query, State},
    http::{Method, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use color_eyre::eyre::{Context, OptionExt, eyre};
use serde::{Deserialize, Deserializer, de};
use tantivy::{TantivyDocument, collector::TopDocs, query::QueryParser, schema::Value};
use tracing::warn;

use crate::{
    AppState,
    err::Error,
    fossil::WritingMeta,
    parental_mode, root_url,
    util::{UrlEntry, render_sitemap, tag_url_for, writing_url_for},
};

mod templates;

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn index(s: State<Arc<AppState>>) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/index.jinja")?;

    let rendered = tmpl.render(Option::<()>::None)?;

    Ok(Html(rendered))
}

fn empty_string_as_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let opt = Option::<String>::deserialize(de)?;
    match opt.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => FromStr::from_str(s).map_err(de::Error::custom).map(Some),
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub q: Option<String>,
}

#[axum::debug_handler]
#[tracing::instrument(skip(s))]
pub async fn search(
    s: State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Html<String>, Error> {
    let tmpl = s.jinja_env.get_template("html/search.jinja")?;

    if query.q.is_none() {
        let rendered = tmpl.render(templates::Search::new(None, Vec::new()))?;

        return Ok(Html(rendered));
    }

    let Some(query) = query.q else {
        return Err(Error::Internal(eyre!("oops")));
    };

    let query_str = urlencoding::decode(&query).context("oops")?.to_string();
    let query = if query_str.contains("tag:") {
        let mut b = String::new();

        for chunk in query_str.split_whitespace() {
            if let Some(chunk) = chunk.strip_prefix("tag:") {
                write!(b, "tag:/tag/{chunk}").context("failed to write")?;
            } else {
                b.push_str(chunk);
            }

            b.push(' ');
        }

        b
    } else {
        query_str.clone()
    };

    let searcher = s.reader.searcher();

    let [title, description, content, tags, slug] = [
        s.schema
            .get_field("title")
            .map_err(color_eyre::Report::from)?,
        s.schema
            .get_field("description")
            .map_err(color_eyre::Report::from)?,
        s.schema
            .get_field("content")
            .map_err(color_eyre::Report::from)?,
        s.schema
            .get_field("tags")
            .map_err(color_eyre::Report::from)?,
        s.schema
            .get_field("slug")
            .map_err(color_eyre::Report::from)?,
    ];

    let mut query_parser =
        QueryParser::for_index(&s.index, vec![title, description, content, tags]);
    query_parser.set_conjunction_by_default();

    query_parser.set_field_boost(title, 3.0);
    query_parser.set_field_boost(tags, 2.0);
    query_parser.set_field_boost(description, 1.5);
    query_parser.set_field_boost(content, 1.0);

    query_parser.set_field_fuzzy(title, false, 1, false);
    query_parser.set_field_fuzzy(description, false, 2, false);

    let t_query = query_parser.parse_query(&query);

    let t_query = match t_query {
        Ok(q) => q,
        Err(e) => {
            warn!(?e, "oops");

            let rendered = tmpl.render(templates::Search::error(Some(&query_str), &e))?;

            return Ok(Html(rendered));
        }
    };

    let mut top_docs = searcher
        .search(&t_query, &TopDocs::with_limit(10))
        .map_err(color_eyre::Report::from)?;

    top_docs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let mut results: Vec<(f32, WritingMeta)> = Vec::new();

    for (score, addr) in top_docs {
        let doc: TantivyDocument = searcher.doc(addr).context("meow")?;
        let slug = doc.get_first(slug).ok_or_eyre("uh oh")?;

        let writing = s
            .get_writing(slug.as_str().ok_or_eyre("welp, this went wrong")?)
            .await;

        if let Some(writing) = writing {
            results.push((score, writing.meta));
        }
    }

    results.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(Ordering::Equal));

    let results = results.into_iter().map(|(_, v)| v).collect();

    let rendered = tmpl.render(templates::Search::new(Some(&query_str), results))?;

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
pub async fn sitemap(s: State<Arc<AppState>>) -> Result<Response, Error> {
    let cache = s.writing_cache.read().await;
    let root = root_url();
    let parental = parental_mode();

    let writings = cache
        .writings
        .iter()
        .filter(|w| !(parental && w.meta.is_nsfw))
        .cloned()
        .collect::<Vec<_>>();

    let site_lastmod = writings
        .iter()
        .flat_map(|w| w.date_authored.into_real_datetime())
        .max();

    let mut tags = Vec::new();
    for tag in cache.tags.iter() {
        let last = writings
            .iter()
            .filter_map(|w| {
                if w.is_hidden {
                    return None;
                }

                if parental_mode() && w.is_nsfw {
                    return None;
                }

                w.tags
                    .contains(tag)
                    .then_some(w.date_authored.into_real_datetime())
            })
            .flatten()
            .max();

        tags.push((tag.clone(), last));
    }

    mem::drop(cache); // early release

    let mut entries = Vec::new();

    entries.push(UrlEntry::new(format!("{root}/"), site_lastmod));
    entries.push(UrlEntry::new(format!("{root}/search"), site_lastmod));
    entries.push(UrlEntry::new(format!("{root}/list"), site_lastmod));
    entries.push(UrlEntry::new(format!("{root}/tags"), site_lastmod));

    for (tag, last) in tags {
        entries.push(UrlEntry::new(tag_url_for(&tag)?, last));
    }

    for w in writings {
        let realdt = w.meta.date_authored.into_real_datetime()?;

        // debug!(?realdt, "real dt for {}", w.meta.title);
        entries.push(UrlEntry::new(writing_url_for(&w.meta), Some(realdt)));
    }

    let xml = render_sitemap(entries);

    let mut resp = Response::new(Body::new(xml));
    resp.headers_mut()
        .insert("Content-Type", "application/xml; charset=utf-8".parse()?);

    Ok(resp)
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
