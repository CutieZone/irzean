#![warn(clippy::pedantic, clippy::nursery)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{env, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use color_eyre::eyre::{Context, OptionExt};
use fossil::RepoHandler;
use minijinja::{Environment, context};
use tokio::{net::TcpListener, sync::RwLock, time};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::{debug, error, info, warn};
use util::Templates;

use crate::{
    fossil::{Writing, WritingMeta},
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

    let _guard = {
        #[cfg(feature = "development")]
        {
            match dotenvy::dotenv() {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Error setting up dotenvy: {e}");
                    return;
                }
            }

            Some(
                match init_tracing_opentelemetry::tracing_subscriber_ext::init_subscribers() {
                    Ok(guard) => guard,
                    Err(e) => {
                        eprintln!("Error setting up otel: {e}");
                        return;
                    }
                },
            )
        }

        #[cfg(not(feature = "development"))]
        {
            use tracing_subscriber::{
                EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, util::SubscriberInitExt,
            };

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
                .init();

            Option::<()>::None
        }
    };

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
    pub writing_cache: Arc<RwLock<Vec<Writing>>>,
    pub css_hash: String,
}

impl AppState {
    pub async fn get_writing(&self, path: &str) -> Option<Writing> {
        let cache = self.writing_cache.read().await;

        cache
            .iter()
            .find(|v| slugify_path(&v.rel_path) == path)
            .cloned()
    }

    pub async fn get_writing_metas(&self) -> Vec<WritingMeta> {
        return self
            .writing_cache
            .read()
            .await
            .iter()
            .map(|v| &v.meta)
            .cloned()
            .collect();
    }
}

async fn background_task(
    writing_cache: Arc<RwLock<Vec<Writing>>>,
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
                    *writing_cache.write().await = new_files;
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
    let writing_cache = Arc::new(RwLock::new(repo_handler.read().await.file_list().await?));

    tokio::spawn(background_task(writing_cache.clone(), repo_handler.clone()));

    insert_links(&mut jinja_env);

    let css_hash = util::hash_scss();

    jinja_env.add_global("css_hash", &css_hash);
    let prerendered = util::prerender_css()?;
    jinja_env.add_global("prerendered_css", &prerendered);
    jinja_env.add_global("parental_mode", parental_mode());

    let app_state = AppState {
        jinja_env,
        repo_handler,
        writing_cache,
        css_hash,
    };

    let router = Router::new()
        .route("/", get(routes::index))
        .route("/list", get(routes::list))
        .route("/tags", get(routes::tags))
        .route("/tag/{name}", get(routes::specific_tag))
        .route("/style/{path}", get(routes::style))
        .route("/writing/{*path}", get(routes::writing))
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .with_state(Arc::new(app_state))
        .layer(TraceLayer::new_for_http())
        .layer(
            CompressionLayer::new()
                .gzip(true)
                .br(true)
                .deflate(true)
                .zstd(true),
        );

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
