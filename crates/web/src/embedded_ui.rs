#[cfg(feature = "embedded-ui")]
use include_dir::{include_dir, Dir};

#[cfg(feature = "embedded-ui")]
static DIST_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../../apps/dist");

#[cfg(feature = "embedded-ui")]
pub fn has_embedded_ui() -> bool {
    // apps/dist 至少应包含 index.html
    DIST_DIR.get_file("index.html").is_some()
}

#[cfg(feature = "embedded-ui")]
pub fn read_asset_bytes(path: &str) -> Option<&'static [u8]> {
    let path = path.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let file = DIST_DIR.get_file(path)?;
    Some(file.contents())
}

#[cfg(feature = "embedded-ui")]
pub fn guess_mime(path: &str) -> String {
    let path = path.trim_start_matches('/');
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

#[cfg(not(feature = "embedded-ui"))]
pub fn has_embedded_ui() -> bool {
    false
}

#[cfg(not(feature = "embedded-ui"))]
pub fn read_asset_bytes(_path: &str) -> Option<&'static [u8]> {
    None
}

#[cfg(not(feature = "embedded-ui"))]
pub fn guess_mime(_path: &str) -> String {
    "application/octet-stream".to_string()
}

