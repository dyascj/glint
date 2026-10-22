//! Small app state that is not a user setting, plus the JSON load/save every
//! data file uses. Shared by the launcher and window processes.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use crate::paths;

/// Reads a JSON file from the support folder; missing or broken files give the default.
pub fn load<T: DeserializeOwned + Default>(name: &str) -> T {
    fs::read_to_string(paths::data(name)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Writes atomically (temp file + rename) so readers never see half a file.
pub fn save<T: Serialize>(name: &str, value: &T) -> std::io::Result<()> {
    paths::ensure_dirs();
    let file = paths::data(name);
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(value).unwrap_or_default())?;
    fs::rename(&tmp, &file)?;
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

    /// Read-modify-write so the launcher and windows don't clobber each other.
    pub fn update(f: impl FnOnce(&mut Prefs)) {
        let mut p = Prefs::load();
        f(&mut p);
        let _ = save(FILE, &p);
    }
}
