use std::{
    env,
    path::{Component, Path, PathBuf},
};

use async_walkdir::{Filtering, WalkDir};
use blake3::Hasher;
use comrak::{Arena, Options, Plugins};
use futures_lite::StreamExt;
use minijinja::{Error, ErrorKind, value::ViaDeserialize};
use slug::slugify;
use tokio::fs;
use tracing::debug;

use crate::{fossil::Writing, root_url};

pub mod tokio_fs;

#[allow(clippy::unnecessary_wraps)]
pub fn tag_url_for(name: &str) -> Result<String, Error> {
    Ok(format!("{}/tag/{name}", root_url()))
}

pub fn slugify_path(path: &Path) -> String {
    let mut out = String::new();

    let mut comps = 0;
    for comp in path.components() {
        debug!(?comp, "the component");
        comps += 1;

        match comp {
            Component::Normal(str) => {
                if comps > 1 {
                    out.push('/');
                }

                let str = str.to_string_lossy().to_string();

                let str = if Path::new(&str)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
                {
                    str.trim_end_matches(".md")
                } else {
                    str.as_str()
                };

                out.push_str(&slugify(str));
            }

            _ => {
                unimplemented!()
            }
        }
    }

    out
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)]
pub fn writing_url_for(writing: ViaDeserialize<Writing>) -> Result<String, Error> {
    let out = format!("{}/writing/{}", root_url(), slugify_path(&writing.rel_path));

    Ok(out)
}

#[allow(clippy::unnecessary_wraps)]
pub fn to_markdown(input: &str) -> Result<String, Error> {
    let arena = Arena::new();
    let mut options = Options::default();

    options.extension.strikethrough = true;
    options.extension.front_matter_delimiter = Some("---".to_string());

    let plugins = Plugins::builder().build();

    let mut output = Vec::new();

    let root = comrak::parse_document(&arena, input, &options);

    comrak::format_html_with_plugins(root, &options, &mut output, &plugins).map_err(|e| {
        Error::new(
            ErrorKind::SyntaxError,
            format!("Unable to htmlify the markdown. {e}"),
        )
    })?;

    String::from_utf8(output)
        .map_err(|e| Error::new(ErrorKind::SyntaxError, format!("Invalid UTF8. {e}")))
}

pub async fn hash_scss() -> color_eyre::Result<String> {
    let root = PathBuf::from(env::var("IRZEAN_STATIC_DIR")?).join("style/");

    let mut walk = WalkDir::new(&root).filter(async |v| {
        if v.path()
            .extension()
            .unwrap_or_default()
            .eq_ignore_ascii_case("scss")
        {
            Filtering::Continue
        } else {
            Filtering::Ignore
        }
    });

    let mut hasher = Hasher::new();

    while let Some(entry) = walk.try_next().await? {
        let path = entry.path();

        if !path.exists() {
            continue;
        }

        let data = fs::read(path).await?;

        hasher.update(&data);
    }

    let hash = hasher.finalize().to_hex().to_string();

    Ok(hash)
}
