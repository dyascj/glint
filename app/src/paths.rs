//! Every file the app touches lives under these roots.

use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "Glint";
pub const BUNDLE_ID: &str = "io.github.dyascj.glint";
pub const REPO_URL: &str = "https://github.com/dyascj/glint";

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

pub fn support_dir() -> PathBuf {
    home().join("Library/Application Support").join(APP_NAME)
}
pub fn logs_dir() -> PathBuf {
    home().join("Library/Logs").join(APP_NAME)
}
pub fn settings_file() -> PathBuf {
    support_dir().join("settings.json")
}
/// A JSON data file (history, colors, quicklinks...) in the support folder.
pub fn data(name: &str) -> PathBuf {
    support_dir().join(name)
}
/// Script commands: Glint's extensions.
pub fn extensions_dir() -> PathBuf {
    support_dir().join("Extensions")
}

pub fn ensure_dirs() {
    for d in [support_dir(), logs_dir(), extensions_dir()] {
        let _ = std::fs::create_dir_all(d);
    }
}

/// `~`-abbreviated path for display.
pub fn tilde(p: &Path) -> String {
    let s = p.display().to_string();
    let h = home().display().to_string();
    s.strip_prefix(&h).map_or(s.clone(), |rest| format!("~{rest}"))
}
