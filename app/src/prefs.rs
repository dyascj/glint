//! Small app state that is not a user setting, plus the JSON load/save every
//! data file uses. Shared by the launcher and window processes.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use crate::paths;

/// Reads a JSON file from the support folder; a missing file gives the default.
/// A file that doesn't parse (a hand edit gone wrong) is moved aside to
/// `name.bad.json` first, so the next save can't overwrite the user's data.
pub fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    load_from(&paths::data(name))
}

fn load_from<T: DeserializeOwned + Default>(file: &std::path::Path) -> T {
    let Ok(text) = fs::read_to_string(file) else { return T::default() };
    match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) if !text.trim().is_empty() => {
            let bad = file.with_extension("bad.json");
            eprintln!("glint: {} doesn't parse ({e}); moved it to {}", file.display(), bad.display());
            let _ = fs::rename(file, &bad);
            T::default()
        }
        Err(_) => T::default(),
    }
}

/// Writes atomically (temp file + fsync + rename) so readers never see half a
/// file. The temp name is unique per process and write, since the launcher and
/// the Settings window can save the same file at the same moment.
pub fn save<T: Serialize>(name: &str, value: &T) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    paths::ensure_dirs();
    let file = paths::data(name);
    let tmp = file.with_extension(format!("json.{}.{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let write = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
        fs::rename(&tmp, &file)
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    own_writes().insert(name.to_string(), mtime(&file));
    Ok(())
}

/// The mtime each file had after this process last saved it.
fn own_writes() -> std::sync::MutexGuard<'static, HashMap<String, Option<SystemTime>>> {
    static OWN: LazyLock<Mutex<HashMap<String, Option<SystemTime>>>> = LazyLock::new(Mutex::default);
    OWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether another process (a Settings window) wrote `name` after we last did.
pub fn changed_elsewhere(name: &str) -> bool {
    own_writes().get(name).is_none_or(|m| *m != mtime(&paths::data(name)))
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Prefs {
    pub onboarding_completed: bool,
    /// Unix seconds of the last successful update check.
    pub updates_last_checked_at: Option<u64>,
    pub updates_latest_version: Option<String>,
    /// Spotlight's ⌘Space before Glint took it over, so it can be given back.
    pub spotlight_taken: bool,
}

const FILE: &str = "prefs.json";

impl Prefs {
    pub fn load() -> Prefs {
        load(FILE)
    }

    /// Read-modify-write under an exclusive file lock, so the launcher and the
    /// windows (or two threads) can't drop each other's changes.
    pub fn update(f: impl FnOnce(&mut Prefs)) {
        paths::ensure_dirs();
        let lock = fs::File::create(paths::data("prefs.lock")).ok();
        if let Some(l) = &lock {
            // SAFETY: plain syscall on an fd we own; released when `lock` drops.
            unsafe { libc::flock(l.as_raw_fd(), libc::LOCK_EX) };
        }
        let mut p = Prefs::load();
        f(&mut p);
        let _ = save(FILE, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_broken_file_is_kept_aside_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("glint-prefs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        fs::write(&file, "{\"onboardingCompleted\": true,}").unwrap();
        let p: Prefs = load_from(&file);
        assert_eq!(p, Prefs::default());
        assert!(!file.exists());
        assert!(fs::read_to_string(dir.join("settings.bad.json")).unwrap().contains("onboardingCompleted"));
        fs::write(&file, "{\"onboardingCompleted\": true}").unwrap();
        assert!(load_from::<Prefs>(&file).onboarding_completed);
        let _ = fs::remove_dir_all(dir);
    }
}
