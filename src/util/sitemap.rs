use time::{PrimitiveDateTime, format_description::well_known::Rfc3339};
use tracing::debug;

pub struct UrlEntry {
    loc: String,
    /// RFC3339 Date
    lastmod: Option<String>,
}

impl UrlEntry {
    pub fn new(loc: String, dt: Option<PrimitiveDateTime>) -> Self {
        let lastmod = dt.map(|d| d.assume_utc().format(&Rfc3339)).transpose();

        debug!(?lastmod);

        Self {
            loc,
            lastmod: lastmod.ok().flatten(),
        }
    }
}

pub fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace(' ', "%20")
}

pub fn render_sitemap(entries: Vec<UrlEntry>) -> String {
    let mut out = String::with_capacity(8192);

    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push_str(r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">"#);

    for e in entries {
        out.push_str("<url>");
        out.push_str("<loc>");
        out.push_str(&xml_escape(&e.loc));
        out.push_str("</loc>");
        if let Some(last) = e.lastmod {
            out.push_str("<lastmod>");
            out.push_str(&last);
            out.push_str("</lastmod>");
        }
        out.push_str("</url>");
    }
    out.push_str("</urlset>");
    out
}
