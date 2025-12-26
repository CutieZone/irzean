use std::path::{Component, Path};

use blake3::Hasher;
use color_eyre::{Report, eyre::OptionExt};
use comrak::{Arena, Options, options::Plugins};
use minijinja::{Error, ErrorKind, value::ViaDeserialize};
use slug::slugify;
use tantivy::{DateTime, IndexWriter, TantivyDocument, schema::Facet};
use tracing::warn;

use crate::{AppState, fossil::WritingMeta, parental_mode, root_url};

mod embed;
mod sitemap;
pub use embed::{Statics, Templates};
pub use sitemap::{UrlEntry, render_sitemap};

pub async fn reindex(s: &AppState, mut writer: IndexWriter) -> color_eyre::Result<()> {
    // Clear index before we re-index
    writer.delete_all_documents()?;
    writer.garbage_collect_files().await?;

    // Work on a clone so we don't block the writing cache for too long
    let cache = s.writing_cache.read().await.clone();

    let sc = s.schema.clone();
    let title = sc.get_field("title")?;
    let description = sc.get_field("description")?;
    let content = sc.get_field("content")?;
    let tags = sc.get_field("tags")?;
    let tag = sc.get_field("tag")?;
    let date = sc.get_field("date")?;
    let nsfw = sc.get_field("nsfw")?;
    let hidden = sc.get_field("hidden")?;
    let slug = sc.get_field("slug")?;
    let word_count = sc.get_field("word_count")?;

    for writing in cache.writings.iter() {
        if writing.is_hidden {
            continue; // skip :3
        }
        if parental_mode() && writing.is_nsfw {
            continue; // skip :3
        }
        let mut doc = TantivyDocument::default();

        doc.add_text(title, &writing.title);

        if let Some(description_text) = &writing.description {
            doc.add_text(description, description_text);
        }

        doc.add_text(content, &writing.content);
        doc.add_text(tags, writing.tags.join(" "));

        let facets: Vec<_> = writing
            .tags
            .iter()
            .map(|tag| Facet::from(&format!("/tag/{tag}")))
            .collect();

        for facet in facets {
            doc.add_facet(tag, facet);
        }

        doc.add_date(
            date,
            DateTime::from_primitive(writing.date_authored.into_real_datetime()?),
        );
        doc.add_bool(nsfw, writing.is_nsfw);
        doc.add_bool(hidden, writing.is_hidden);

        doc.add_text(slug, slugify_path(&writing.rel_path));
        doc.add_u64(
            word_count,
            writing.content.split_whitespace().count().try_into()?,
        );

        writer.add_document(doc)?;
    }

    writer.commit()?;

    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
pub fn tag_url_for(name: &str) -> Result<String, Error> {
    Ok(format!("{}/tag/{name}", root_url()))
}

pub fn slugify_path(path: &Path) -> String {
    let mut out = String::new();

    let mut comps = 0;
    for comp in path.components() {
        // debug!(?comp, "the component");
        comps += 1;

        if let Component::Normal(str) = comp {
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
        } else {
            // Skip RootDir/CurDir/ParentDir
        }
    }

    out
}

#[allow(clippy::unnecessary_wraps, clippy::needless_pass_by_value)]
pub fn writing_url_for_jinja(writing: ViaDeserialize<WritingMeta>) -> Result<String, Error> {
    let out = format!("{}/writing/{}", root_url(), slugify_path(&writing.rel_path));

    Ok(out)
}

pub fn writing_url_for(writing: &WritingMeta) -> String {
    format!("{}/writing/{}", root_url(), slugify_path(&writing.rel_path))
}

#[allow(clippy::unnecessary_wraps)]
pub fn writing_url_from(slugified: &str) -> Result<String, Error> {
    Ok(format!("{}/writing/{}", root_url(), slugified))
}

#[tracing::instrument(skip(input))]
fn preprocess(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            if line.starts_with('>') {
                format!("{line}\n>")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tracing::instrument(skip(input))]
pub fn to_markdown(input: &str) -> Result<String, Error> {
    let input = preprocess(input);
    let input = input.as_str();

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

    let mut output = String::new();

    let root = comrak::parse_document(&arena, input, &options);

    comrak::format_html_with_plugins(root, &options, &mut output, &plugins).map_err(|e| {
        Error::new(
            ErrorKind::SyntaxError,
            format!("Unable to htmlify the markdown. {e}"),
        )
    })?;

    Ok(output)
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

pub fn prerender_css() -> Result<String, crate::err::Error> {
    let main_scss = Statics::get("main.scss").ok_or_eyre("this should never fail.")?;
    let string = String::from_utf8(main_scss.data.to_vec()).map_err(Report::from)?;
    let rendered = grass::from_string(string, &grass::Options::default())?;

    Ok(rendered)
}
