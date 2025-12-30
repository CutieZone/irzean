#![warn(clippy::pedantic, clippy::nursery)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::unsafe_derive_deserialize)]

use std::{env, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use color_eyre::eyre::{Context, OptionExt};
use fossil::RepoHandler;
use minijinja::{Environment, context};
use tantivy::{Index, IndexReader, ReloadPolicy, schema::Schema};
use tokio::{net::TcpListener, sync::RwLock, time};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{
    EnvFilter, Layer as _, Registry, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};
use util::Templates;

use crate::{
    fossil::{Writing, WritingCache, WritingMeta},
    util::slugify_path,
};

mod err;
mod fossil;
mod routes;
mod util;

#[tokio::main]
async fn main() {
    match color_eyre::install() {
        Ok(()) => {}
        Err(e) => {
            error!(?e, "Could not install color-eyre");
            return;
        }
    }

    #[cfg(feature = "development")]
    {
        match dotenvy::dotenv() {
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error setting up dotenvy: {e}");
                return;
            }
        }
    }

    if env::var("RUST_LOG").is_err() {
        unsafe {
            // SAFETY: Required as of Rust 2024, should be fine though
            env::set_var("RUST_LOG", "debug");
        }
    }

    Registry::default()
        .with(
            fmt::layer()
                .pretty()
                .with_target(true)
                .with_thread_ids(true)
                .with_level(true)
                .with_line_number(true)
                .with_file(true)
                .with_filter(EnvFilter::from_default_env()),
        )
        .with(tracing_error::ErrorLayer::default())
        .init();

    match run().await {
        Ok(()) => {}
        Err(e) => {
            error!(?e, "Failed to run");
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub jinja_env: Environment<'static>,
    pub repo_handler: Arc<RwLock<RepoHandler>>,
    pub writing_cache: Arc<RwLock<WritingCache>>,

    pub schema: Schema,
    pub index: Index,
    pub reader: IndexReader,
}

impl AppState {
    pub async fn get_writing(&self, path: &str) -> Option<Writing> {
        let cache = self.writing_cache.read().await;

        cache
            .writings
            .iter()
            .find(|v| slugify_path(&v.rel_path) == path)
            .cloned()
    }

    pub async fn get_writing_metas(&self) -> Vec<WritingMeta> {
        self.writing_cache.read().await.metas()
    }
}

async fn background_task(
    writing_cache: Arc<RwLock<WritingCache>>,
    repo_handler: Arc<RwLock<RepoHandler>>,
) {
    let interval = env::var("IRZEAN_UPDATE_INTERVAL")
        .map(|v| v.parse().ok())
        .ok()
        .flatten()
        .unwrap_or(60);

    let mut interval = time::interval(Duration::from_secs(interval));

    loop {
        interval.tick().await;

        let previous_commit_ref = repo_handler.read().await.latest_commit.clone();
        let value = repo_handler.write().await.update();

        if repo_handler.read().await.latest_commit != previous_commit_ref {
            let new_files = repo_handler.read().await.file_list().await;

            match new_files {
                Ok(new_files) => {
                    let new_writings = Arc::new(new_files);
                    let new_tags = Arc::new(
                        new_writings
                            .iter()
                            .flat_map(|v| &v.tags)
                            .map(|v| v.to_lowercase())
                            .collect(),
                    );
                    let mut writing_cache = writing_cache.write().await;

                    writing_cache.writings = new_writings;
                    writing_cache.tags = new_tags;
                }
                Err(err) => {
                    warn!(?err, "failed to update cache");
                }
            }

            // TODO: only re-parse and re-cache changed files
        }

        match value {
            Ok(()) => {}
            Err(e) => {
                warn!(?e, "Failed during repo_handler update");
            }
        }
    }
}

fn build_schema() -> Schema {
    use tantivy::schema::{FAST, INDEXED, STORED, TEXT};
    let mut sb = Schema::builder();

    sb.add_text_field("title", TEXT | STORED); // Searchable + returned
    sb.add_text_field("content", TEXT); // Searchable, not stored
    sb.add_text_field("description", TEXT | STORED); // Searchable + returned

    sb.add_text_field("tags", TEXT); // For text search within tags
    sb.add_facet_field("tag", INDEXED); // For exact tag filtering

    sb.add_date_field("date", INDEXED | STORED | FAST);
    sb.add_bool_field("nsfw", INDEXED | STORED | FAST);
    sb.add_bool_field("hidden", INDEXED | FAST);

    sb.add_text_field("slug", STORED);

    sb.add_u64_field("word_count", INDEXED | STORED | FAST);

    sb.build()
}

async fn run() -> color_eyre::Result<()> {
    let port = env::var("IRZEAN_PORT").unwrap_or_else(|_| {
        warn!("No `IRZEAN_PORT` provided: defaulting to port 1337");
        "1337".to_string()
    });

    let addr = format!("0.0.0.0:{port}")
        .parse::<SocketAddr>()
        .context(format!("Couldn't parse `0.0.0.0:{port}`"))?;

    let mut jinja_env = build_jinja_env()?;
    let mut repo_handler = RepoHandler::init()?;
    repo_handler.update()?;

    let repo_handler = Arc::new(RwLock::new(repo_handler));

    let file_list = Arc::new(repo_handler.read().await.file_list().await?);

    let writing_cache = WritingCache {
        writings: file_list.clone(),
        tags: Arc::new(
            file_list
                .iter()
                .flat_map(|v| &v.tags)
                .map(|v| v.to_lowercase()) // canonicalize :3
                .collect(),
        ),
    };
    let writing_cache = Arc::new(RwLock::new(writing_cache));

    tokio::spawn(background_task(writing_cache.clone(), repo_handler.clone()));

    insert_links(&mut jinja_env);

    let prerendered = util::prerender_css()?;
    jinja_env.add_global("prerendered_css", &prerendered);
    jinja_env.add_global("parental_mode", parental_mode());

    let schema = build_schema();
    let index = Index::create_in_ram(schema.clone());
    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()?;

    let app_state = AppState {
        jinja_env,
        repo_handler,
        writing_cache,

        schema,
        index,
        reader,
    };

    util::reindex(&app_state, app_state.index.writer(50_000_000)?).await?;

    let router = Router::new()
        .route("/", get(routes::index))
        .route("/search", get(routes::search))
        .route("/list", get(routes::list))
        .route("/tags", get(routes::tags))
        .route("/tag/{name}", get(routes::specific_tag))
        .route("/writing/{*path}", get(routes::writing))
        .route("/sitemap.xml", get(routes::sitemap))
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .with_state(Arc::new(app_state))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new().gzip(true));

    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;

    info!("Listening on http://{addr}");
    match axum::serve(listener, router).await {
        Ok(()) => {}
        Err(e) => {
            error!(?e, "Failed during axum serving");
            return Err(e.into());
        }
    }

    Ok(())
}

fn insert_links(env: &mut Environment<'static>) {
    let root_uri = root_url();
    info!(
        "Using the root URI `{root_uri}`. To override, use the `IRZEAN_ROOT_URL` environment variable"
    );

    #[cfg(feature = "development")]
    {
        env.add_global("environment", "DEV");
    }

    let mut links = vec![];

    links.push(context! {
        name => "Home",
        uri => format!("{root_uri}/"),
        target => "_self",
    });
    links.push(context! {
        name => "Search",
        uri => format!("{root_uri}/search"),
        target => "_self",
    });
    links.push(context! {
        name => "Tags",
        uri => format!("{root_uri}/tags"),
        target => "_self",
    });
    links.push(context! {
        name => "List",
        uri => format!("{root_uri}/list"),
        target => "_self",
    });

    if !parental_mode() {
        links.push(context! {
            name => "Contact",
            uri => "https://cutie.zone/social",
            target => "_blank",
        });
        links.push(context! {
            name => "Leave",
            uri => "https://cutie.zone",
            target => "_self",
        });
    }

    env.add_global("links", links);
}

fn build_jinja_env() -> color_eyre::Result<Environment<'static>> {
    let mut jinja_env = Environment::new();

    let template_paths = Templates::iter().filter(|v| v.starts_with("html/"));

    for path in template_paths {
        let Some(data) = Templates::get(&path) else {
            warn!("Could not find template data for {path}");
            continue;
        };

        let path_buf = PathBuf::from(path.as_ref());
        let file_name = path_buf
            .file_name()
            .ok_or_eyre("no file name even if there should be one")?;
        let file_name = file_name.to_str().ok_or_eyre("could not convert to str")?;

        if file_name.ends_with("jinja") {
            debug!(?path, "Adding template {file_name} as `{path}`");

            jinja_env
                .add_template_owned(path.to_string(), String::from_utf8(data.data.to_vec())?)?;
        }
    }

    jinja_env.add_function("tag_url_for", util::tag_url_for);
    jinja_env.add_function("writing_url_for", util::writing_url_for_jinja);
    jinja_env.add_function("writing_url_from", util::writing_url_from);
    jinja_env.add_function("to_markdown", util::to_markdown);

    Ok(jinja_env)
}

pub(crate) fn root_url() -> String {
    env::var("IRZEAN_ROOT_URL").unwrap_or_else(|_| {
        format!(
            "http://0.0.0.0:{}",
            env::var("IRZEAN_PORT").unwrap_or_else(|_| "1337".to_string())
        )
    })
}

pub(crate) fn parental_mode() -> bool {
    env::var("IRZEAN_PARENTAL_MODE").is_ok()
}
