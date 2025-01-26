#![warn(clippy::pedantic, clippy::nursery)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{env, net::SocketAddr, sync::Arc};

use axum::{Router, routing::get};
use color_eyre::eyre::{Context, OptionExt};
use fossil::RepoHandler;
use minijinja::{Environment, context};
use tokio::{fs, net::TcpListener};
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};

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

    if env::var("RUST_LOG").is_err() {
        unsafe {
            env::set_var("RUST_LOG", "debug");
        }
    }

    fmt()
        .with_target(true)
        .with_thread_ids(true)
        .with_level(true)
        .with_line_number(true)
        .with_file(true)
        .pretty()
        .with_env_filter(EnvFilter::from_default_env())
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
    pub repo_handler: RepoHandler,
}

async fn run() -> color_eyre::Result<()> {
    let addr = "0.0.0.0:1337"
        .parse::<SocketAddr>()
        .context("Couldn't parse `0.0.0.0:1337`")?;

    let jinja_env = build_jinja_env().await?;

    let router = Router::new()
        .route("/", get(routes::index))
        .route("/style/{path}", get(routes::style))
        .fallback(routes::not_found)
        .method_not_allowed_fallback(routes::method_not_allowed)
        .with_state(Arc::new(AppState {
            jinja_env,
            repo_handler: RepoHandler::init()?,
        }))
        .layer(TraceLayer::new_for_http());

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
    };

    Ok(())
}

async fn build_jinja_env() -> color_eyre::Result<Environment<'static>> {
    let mut jinja_env = Environment::new();

    let mut read_dir = fs::read_dir("./templates/html")
        .await
        .context("Couldn't read template path.")?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();

        if !path.exists() || !path.is_file() {
            continue;
        }

        let file_name = path
            .as_path()
            .file_name()
            .ok_or_eyre("no file name, should be there though")?;
        let file_name = file_name.to_str().ok_or_eyre("could not convert to str")?;

        if let Some(ext) = path.extension() {
            if !ext.eq_ignore_ascii_case("jinja") {
                continue;
            }

            jinja_env.add_template_owned(
                format!("html/{file_name}"),
                fs::read_to_string(&path).await?,
            )?;
        }
    }

    let links = vec![context! {
        uri => "https://potato.com",
        name => "Potato"
    }];

    jinja_env.add_global("links", links);

    Ok(jinja_env)
}
