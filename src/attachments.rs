//! Images attached to messages. Each is copied into the app's support
//! folder when attached, so it outlives the original (or the clipboard),
//! still shows after a relaunch, and can be re-sent by Retry. Files are
//! deleted with the conversation that uses them.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::Engine;

use crate::storage;

/// Claude's limit for a single image.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// File extensions Claude accepts, and their media types.
const TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

fn media_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, media)| *media)
}

fn dir() -> io::Result<PathBuf> {
    let dir = storage::support_dir().join("attachments");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A fresh file name in the attachments folder.
fn new_path(extension: &str) -> io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(dir()?.join(format!("{millis}-{count}.{extension}")))
}

/// Copies an image file in. Errors are user-facing.
pub fn import_file(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if media_type(path).is_none() {
        return Err(format!("{name} isn't a PNG, JPEG, GIF or WebP image."));
    }
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    check_size(&name, size)?;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png");
    let copy = new_path(&extension.to_ascii_lowercase()).map_err(|error| error.to_string())?;
    fs::copy(path, &copy).map_err(|error| error.to_string())?;
    Ok(copy)
}

/// Saves pasted image data. `extension` must be one of [`TYPES`].
pub fn import_bytes(bytes: &[u8], extension: &str) -> Result<PathBuf, String> {
    check_size("The pasted image", bytes.len() as u64)?;
    let path = new_path(extension).map_err(|error| error.to_string())?;
    fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

fn check_size(name: &str, size: u64) -> Result<(), String> {
    if size > MAX_BYTES {
        return Err(format!(
            "{name} is {:.1} MB; images can be at most 5 MB.",
            size as f64 / 1024. / 1024.
        ));
    }
    Ok(())
}

/// The image as a Claude content block.
pub fn content_block(path: &Path) -> io::Result<serde_json::Value> {
    let media_type = media_type(path).unwrap_or("image/png");
    let data = base64::engine::general_purpose::STANDARD.encode(fs::read(path)?);
    Ok(serde_json::json!({
        "type": "image",
        "source": { "type": "base64", "media_type": media_type, "data": data },
    }))
}

/// Deletes attachment files that are no longer used.
pub fn remove(paths: impl IntoIterator<Item = PathBuf>) {
    for path in paths {
        fs::remove_file(path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_image_types() {
        assert_eq!(media_type(Path::new("a/photo.JPG")), Some("image/jpeg"));
        assert_eq!(media_type(Path::new("shot.png")), Some("image/png"));
        assert_eq!(media_type(Path::new("notes.txt")), None);
        assert_eq!(media_type(Path::new("no-extension")), None);
    }

    #[test]
    fn rejects_large_images() {
        assert!(check_size("x", MAX_BYTES).is_ok());
        assert!(check_size("x", MAX_BYTES + 1).is_err());
    }
}
