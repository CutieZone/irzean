use std::path::{Component, Path};

use blake3::Hasher;
use comrak::{Arena, Options, Plugins};
use lol_html::{HtmlRewriter, Settings, text};
use minijinja::{Error, ErrorKind, value::ViaDeserialize};
use slug::slugify;
use tracing::{debug, warn};

use crate::{fossil::Writing, root_url};

mod embed;
pub mod tokio_fs;
pub use embed::{Statics, Templates};

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
pub fn writing_url_from(slugified: &str) -> Result<String, Error> {
    Ok(format!("{}/writing/{}", root_url(), slugified))
}

#[allow(clippy::unnecessary_wraps)]
pub fn to_markdown(input: &str) -> Result<String, Error> {
    let arena = Arena::new();
    let mut options = Options::default();

    options.extension.strikethrough = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    options.extension.table = true;
    options.extension.superscript = true;
    options.extension.underline = true;
    options.extension.subscript = true;

    options.parse.smart = true;

    options.render.hardbreaks = true;
    options.render.prefer_fenced = true;
    options.render.figure_with_caption = true;

    let plugins = Plugins::builder().build();

    let mut output = Vec::new();

    let root = comrak::parse_document(&arena, input, &options);

    comrak::format_html_with_plugins(root, &options, &mut output, &plugins).map_err(|e| {
        Error::new(
            ErrorKind::SyntaxError,
            format!("Unable to htmlify the markdown. {e}"),
        )
    })?;

    let maybe = String::from_utf8(output)
        .map_err(|e| Error::new(ErrorKind::SyntaxError, format!("Invalid UTF8. {e}")))?;

    output = Vec::new();

    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![text!("blockquote p", |el| {
                let text = el.as_str();

                if el.as_str().contains("<br>") {
                    el.set_str(text.replace("<br>", "<\\p><p>"));
                }

                Ok(())
            })],
            ..Settings::new()
        },
        |c: &[u8]| output.extend_from_slice(c),
    );

    rewriter
        .write(maybe.as_bytes())
        .map_err(|e| Error::new(ErrorKind::SyntaxError, format!("Failed to write. {e}")))?;
    rewriter
        .end()
        .map_err(|e| Error::new(ErrorKind::SyntaxError, format!("Failed to end. {e}")))?;

    String::from_utf8(output)
        .map_err(|e| Error::new(ErrorKind::SyntaxError, format!("Invalid UTF8. {e}")))
}

pub fn hash_scss() -> String {
    let items = Statics::iter().filter(|v| v.ends_with("scss"));

    let mut hasher = Hasher::new();

    for path in items {
        let Some(data) = Statics::get(&path) else {
            warn!("No data found for {path}");
            continue;
        };

        hasher.update(&data.data);
    }

    hasher.finalize().to_hex().to_string()
}
