//! Files under `~/Library/Application Support/claude-companion`.

use std::{fs, io, path::PathBuf};

use serde::{Serialize, de::DeserializeOwned};

pub fn support_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join("Library/Application Support/claude-companion")
}

/// Fixed working directory for `claude`, which stores sessions per project
/// directory: `--resume` only finds sessions started from the same place.
pub fn work_dir() -> io::Result<PathBuf> {
    let dir = support_dir().join("workdir");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Reads `name` from the support directory, falling back to the default if
/// the file is missing or unreadable.
pub fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    fs::read(support_dir().join(name))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Writes to a temporary file and renames it so a crash never leaves a
/// half-written file behind.
pub fn save<T: Serialize>(name: &str, value: &T) -> io::Result<()> {
    let dir = support_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(name);
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    fs::rename(temporary, path)
}
