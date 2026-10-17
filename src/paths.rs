//! Where the utility keeps its files: the per-user config directory of the
//! system, as claudestatus does.

use std::path::PathBuf;

const APP: &str = "sentrypanel";

/// macOS `~/Library/Application Support/sentrypanel`, Linux
/// `~/.config/sentrypanel`.
pub fn dir() -> Result<PathBuf, String> {
    let base = dirs::config_dir().ok_or("the system has no config directory for this user")?;
    Ok(base.join(APP))
}

pub fn config() -> Result<PathBuf, String> {
    Ok(dir()?.join("config.toml"))
}

/// A file of ours next to the config, its directory created on the way.
pub fn file(name: &str) -> Result<PathBuf, String> {
    let dir = dir()?;
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("could not create {}: {err}", dir.display()))?;
    Ok(dir.join(name))
}
