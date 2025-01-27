use rust_embed::RustEmbed;

#[derive(Debug, RustEmbed)]
#[folder = "templates/"]
pub struct Templates;

#[derive(Debug, RustEmbed)]
#[folder = "static/"]
pub struct Statics;
