#![allow(clippy::struct_field_names)]

use std::collections::HashMap;

use axum::http::{Method, StatusCode};
use facet::Facet;
use serde::Serialize;
use tantivy::query::QueryParserError;

use crate::{
    fossil::{Writing, WritingMeta},
    root_url,
    util::writing_url_for,
};

#[derive(Debug, Serialize)]
pub struct Error {
    pub title: String,
    pub error: Option<String>,
    pub code: u16,
    pub reason: String,
    pub path: String,
    pub url: String,
}

impl Error {
    pub fn not_found(path: &str) -> Self {
        Self {
            title: "Not Found".to_string(),
            error: None,
            code: StatusCode::NOT_FOUND.as_u16(),
            reason: "The requested resource simply could not be found.".to_string(),
            path: path.to_string(),
            url: format!("{}{path}", root_url()),
        }
    }

    pub fn method_not_allowed(method: &Method, path: &str) -> Self {
        Self {
            title: "Method Not Allowed".to_string(),
            error: Some(format!("Method {method} Not Allowed")),
            code: StatusCode::METHOD_NOT_ALLOWED.as_u16(),
            reason: format!("The method {method} is disallowed on this path."),
            path: path.to_string(),
            url: format!("{}{path}", root_url()),
        }
    }
}

#[derive(Facet, Serialize)]
pub struct Search {
    pub title: String,
    pub query: String,
    pub results: Vec<WritingMeta>,
    pub error: Option<String>,
    pub description: String,
    pub url: String,
}

impl Search {
    pub fn error(query: Option<&String>, error: &QueryParserError) -> Self {
        Self {
            title: "Search".to_string(),
            query: query.cloned().unwrap_or_default(),
            results: Vec::new(),
            error: Some(error.to_string()),
            description: format!(
                "A search page.{}",
                query
                    .as_ref()
                    .map_or_else(String::new, |query| format!("\n{query}"))
            ),
            url: format!(
                "{}/search{}",
                root_url(),
                query.as_ref().map_or_else(String::new, |query| format!(
                    "?q={}",
                    urlencoding::encode(query)
                ))
            ),
        }
    }

    pub fn new(query: Option<&String>, results: Vec<WritingMeta>) -> Self {
        Self {
            title: "Search".to_string(),
            query: query.cloned().unwrap_or_default(),
            results,
            error: None,
            description: format!(
                "A search page.{}",
                query
                    .as_ref()
                    .map_or_else(String::new, |query| format!("\n{query}"))
            ),
            url: format!(
                "{}/search{}",
                root_url(),
                query.as_ref().map_or_else(String::new, |query| format!(
                    "?q={}",
                    urlencoding::encode(query)
                ))
            ),
        }
    }
}

#[derive(Facet, Serialize)]
pub struct List {
    pub title: String,
    pub writings: Vec<WritingMeta>,
    pub description: String,
    pub url: String,
}

impl List {
    pub fn new(w: Vec<WritingMeta>) -> Self {
        Self {
            title: "List".to_string(),
            writings: w,
            description: "A list of everything on the website".to_string(),
            url: format!("{}/list", root_url()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Tags {
    pub title: String,
    pub tags: HashMap<String, u64>,
    pub description: String,
    pub url: String,
}

impl Tags {
    pub fn new(tags: HashMap<String, u64>) -> Self {
        Self {
            title: "Tags".to_string(),
            tags,
            description: "A list of all the tags".to_string(),
            url: format!("{}/tags", root_url()),
        }
    }
}

#[derive(Facet, Serialize)]
pub struct SpecificTag {
    pub title: String,
    pub tag_name: String,
    pub writings: Vec<WritingMeta>,
    pub description: String,
    pub url: String,
}

impl SpecificTag {
    pub fn new(tag_name: &str, writings: Vec<WritingMeta>) -> Self {
        Self {
            title: format!("Writings Tagged {tag_name}"),
            tag_name: tag_name.to_string(),
            writings,
            description: format!("All the writings tagged with {tag_name}"),
            url: format!("{}/tag/{tag_name}", root_url()),
        }
    }
}

#[derive(Facet, Serialize)]
pub struct Reader {
    pub writing: Writing,
    pub title: String,
    #[facet(rename = "type")]
    pub kind: String,
    pub article: Article,
    pub description: Option<String>,
    pub url: String,
}

impl Reader {
    pub fn new(writing: Writing) -> Self {
        Self {
            title: writing.title.clone(),
            kind: "article".to_string(),
            article: Article {
                published_time: writing.date_authored.to_string(),
                author: "Lys".to_string(),
                tags: writing.tags.clone(),
            },
            description: writing.description.clone(),
            url: writing_url_for(&writing.meta),
            writing,
        }
    }
}

#[derive(Facet, Serialize)]
pub struct Article {
    pub published_time: String,
    pub author: String,
    pub tags: Vec<String>,
}
