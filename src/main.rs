#![warn(clippy::pedantic, clippy::nursery)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{env, net::SocketAddr, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use color_eyre::eyre::{Context, OptionExt};
use fossil::RepoHandler;
use minijinja::{Environment, context};
use tokio::{fs, net::TcpListener, sync::RwLock, time};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::{debug, error, info, warn};
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
    pub repo_handler: Arc<RwLock<RepoHandler>>,
    pub css_hash: String,
}

async fn background_task(repo_handler: Arc<RwLock<RepoHandler>>) {
    let mut interval = time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        let value = repo_handler.write().await.update();
        match value {
            Ok(()) => {}
            Err(e) => {
                warn!(?e, "Failed during repo_handler update");
            }
        }
    }
}

async fn run() -> color_eyre::Result<()> {
    let addr = "0.0.0.0:1337"
        .parse::<SocketAddr>()
        .context("Couldn't parse `0.0.0.0:1337`")?;

    let mut jinja_env = build_jinja_env().await?;
    let mut repo_handler = RepoHandler::init()?;
    repo_handler.update()?;

    let repo_handler = Arc::new(RwLock::new(repo_handler));

    tokio::spawn(background_task(repo_handler.clone()));

    insert_links(&mut jinja_env);

    let css_hash = util::hash_scss().await?;

    jinja_env.add_global("css_hash", &css_hash);

    let app_state = AppState {
        jinja_env,
        repo_handler,
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
        .layer(CompressionLayer::new().gzip(true).br(true));

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

fn insert_links(env: &mut Environment<'static>) {
    let root_uri = root_url();
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
    links.push(context! {
        name => "Leave",
        uri => "https://cutie.zone",
        target => "_self",
    });

    env.add_global("links", links);
}

async fn build_jinja_env() -> color_eyre::Result<Environment<'static>> {
    let mut jinja_env = Environment::new();

    let template_dir =
        env::var("IRZEAN_TEMPLATE_DIR").unwrap_or_else(|_| "./templates".to_string());

    debug!(?template_dir, "Using");

    let template_dir = template_dir + "/html";

    let mut read_dir = fs::read_dir(template_dir)
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

            debug!(?path, "Adding template {file_name} as `html/{file_name}`");

            jinja_env.add_template_owned(
                format!("html/{file_name}"),
                fs::read_to_string(&path).await?,
            )?;
        }
    }

    jinja_env.add_function("tag_url_for", util::tag_url_for);
    jinja_env.add_function("writing_url_for", util::writing_url_for);
    jinja_env.add_function("writing_url_from", util::writing_url_from);
    jinja_env.add_function("to_markdown", util::to_markdown);

    Ok(jinja_env)
}

pub(crate) fn root_url() -> String {
    env::var("IRZEAN_ROOT_URL").unwrap_or_else(|_| "http://0.0.0.0:1337".to_string())
}
