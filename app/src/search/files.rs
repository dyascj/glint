//! File search through Spotlight's index (`mdfind`), so there's no index of
//! our own to build or keep fresh. A worker thread runs only the latest query;
//! anything typed meanwhile replaces the pending one.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

use crate::paths;
use crate::settings::FileScope;

/// Lines read from mdfind before ranking (it streams in index order).
const SCAN: usize = 1500;

pub struct FileSearch {
    shared: Arc<(Mutex<State>, Condvar)>,
}

#[derive(Default)]
struct State {
    pending: Option<(u64, String, FileScope, usize)>,
    done: Option<(u64, Vec<PathBuf>)>,
}

impl FileSearch {
    /// `notify` runs after each finished search (to repaint the UI).
    pub fn new(notify: impl Fn() + Send + 'static) -> FileSearch {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let s = shared.clone();
        std::thread::spawn(move || {
            loop {
                let (id, q, scope, limit) = {
                    let (lock, cv) = &*s;
                    let mut st = lock.lock().unwrap();
                    loop {
                        if let Some(p) = st.pending.take() {
                            break p;
                        }
                        st = cv.wait(st).unwrap();
                    }
                };
                let found = search(&q, scope, limit, || s.0.lock().unwrap().pending.is_some());
                if let Some(found) = found {
                    s.0.lock().unwrap().done = Some((id, found));
                    notify();
                }
            }
        });
        FileSearch { shared }
    }

    pub fn request(&self, id: u64, query: &str, scope: FileScope, limit: usize) {
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap().pending = Some((id, query.to_string(), scope, limit));
        cv.notify_one();
    }

    /// The finished result for search `id`, if it's ready.
    pub fn take(&self, id: u64) -> Option<Vec<PathBuf>> {
        let mut st = self.shared.0.lock().unwrap();
        if st.done.as_ref().is_some_and(|(d, _)| *d == id) { st.done.take().map(|(_, v)| v) } else { None }
    }
}

/// Runs mdfind and ranks what it returns. None if `superseded` says a newer
/// query arrived while reading.
fn search(q: &str, scope: FileScope, limit: usize, superseded: impl Fn() -> bool) -> Option<Vec<PathBuf>> {
    let q = q.trim();
    if q.chars().count() < 2 {
        return Some(Vec::new());
    }
    let home = paths::home();
    let mut cmd = Command::new("/usr/bin/mdfind");
    if scope == FileScope::Home {
        cmd.arg("-onlyin").arg(&home);
    }
    cmd.arg("-name").arg(q).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut hits = Vec::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok).take(SCAN) {
            if hits.len() % 100 == 0 && superseded() {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            let p = PathBuf::from(line);
            if !noise(&p, &home) {
                hits.push(p);
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    Some(rank(q, hits, &home, limit))
}

/// Paths nobody means when they search for a file.
fn noise(p: &Path, home: &Path) -> bool {
    let s = p.to_string_lossy();
    let rel = p.strip_prefix(home).map(|r| r.to_string_lossy().into_owned());
    s.contains("/.")
        || s.contains("/node_modules/")
        || s.contains("/target/")
        || s.contains(".app/")
        || s.contains(".photoslibrary/")
        || s.contains("/DerivedData/")
        || rel.is_ok_and(|r| r.starts_with("Library/"))
}

fn rank(q: &str, hits: Vec<PathBuf>, home: &Path, limit: usize) -> Vec<PathBuf> {
    let q = q.to_lowercase();
    let mut scored: Vec<(i32, PathBuf)> = hits
        .into_iter()
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let stem = p.file_stem().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let mut s = if stem == q || name == q {
                1000
            } else if name.starts_with(&q) {
                800
            } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q)) {
                600
            } else {
                400
            };
            s -= p.components().count() as i32 * 8;
            s -= name.len().min(80) as i32;
            if p.starts_with(home) {
                s += 40;
            }
            if p.is_dir() {
                s += 10;
            }
            (s, p)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_prefers_exact_shallow_names() {
        let home = PathBuf::from("/Users/me");
        let hits = vec![
            PathBuf::from("/Users/me/a/b/c/d/old-report-final.pdf"),
            PathBuf::from("/Users/me/Documents/report.pdf"),
            PathBuf::from("/Users/me/Documents/reports-2024.xlsx"),
        ];
        let r = rank("report", hits, &home, 10);
        assert_eq!(r[0], PathBuf::from("/Users/me/Documents/report.pdf"));
        assert_eq!(r[2], PathBuf::from("/Users/me/a/b/c/d/old-report-final.pdf"));
        assert!(noise(Path::new("/Users/me/Library/Caches/x"), &home));
        assert!(noise(Path::new("/Users/me/code/node_modules/x"), &home));
        assert!(!noise(Path::new("/Users/me/Desktop/x.png"), &home));
    }
}
