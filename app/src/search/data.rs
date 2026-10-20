//! What Glint remembers: how often you pick things (ranking), quicklinks,
//! snippets, clipboard history, saved colors and calculator history. Each is
//! one JSON file in the support folder.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::color::Rgb;
use crate::prefs;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// ---- frecency ----

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Use {
    pub count: u32,
    pub last: u64,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Frecency(pub HashMap<String, Use>);

impl Frecency {
    pub const FILE: &str = "frecency.json";

    pub fn load() -> Frecency {
        prefs::load(Self::FILE)
    }

    pub fn record(&mut self, id: &str) {
        let u = self.0.entry(id.to_string()).or_default();
        u.count = u.count.saturating_add(1);
        u.last = now();
        let _ = prefs::save(Self::FILE, self);
    }

    pub fn forget(&mut self, id: &str) {
        self.0.remove(id);
        let _ = prefs::save(Self::FILE, self);
    }

    /// Ranking bonus: frequent and recent picks float up, fading over weeks.
    pub fn bonus(&self, id: &str) -> i32 {
        self.0.get(id).map_or(0, |u| bonus(u, now()))
    }
}

fn bonus(u: &Use, now: u64) -> i32 {
    let days = now.saturating_sub(u.last) as f32 / 86_400.0;
    let recency = 1.0 / (1.0 + days / 7.0);
    ((u.count as f32).ln_1p() * 90.0 * recency).min(400.0) as i32
}

// ---- quicklinks ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quicklink {
    pub name: String,
    /// `{query}` is replaced with the (URL-encoded) argument.
    pub url: String,
    /// An SF Symbol name (or an image path).
    #[serde(default)]
    pub icon: String,
}

impl Quicklink {
    pub fn takes_query(&self) -> bool {
        self.url.contains("{query}")
    }

    pub fn id(&self) -> String {
        format!("quicklink:{}", self.name)
    }

    pub fn url_for(&self, query: &str) -> String {
        self.url.replace("{query}", &encode(query))
    }
}

pub const QUICKLINKS: &str = "quicklinks.json";

pub fn load_quicklinks() -> Vec<Quicklink> {
    let stored: Option<Vec<Quicklink>> = std::fs::read_to_string(crate::paths::data(QUICKLINKS)).ok().and_then(|t| serde_json::from_str(&t).ok());
    stored.unwrap_or_else(default_quicklinks)
}

pub fn save_quicklinks(q: &[Quicklink]) {
    let _ = prefs::save(QUICKLINKS, &q);
}

fn default_quicklinks() -> Vec<Quicklink> {
    [
        ("Search Google", "https://www.google.com/search?q={query}", "globe"),
        ("Search DuckDuckGo", "https://duckduckgo.com/?q={query}", "magnifyingglass.circle"),
        ("Search GitHub", "https://github.com/search?q={query}", "chevron.left.forwardslash.chevron.right"),
        ("Search YouTube", "https://www.youtube.com/results?search_query={query}", "play.rectangle"),
        ("Search Wikipedia", "https://en.wikipedia.org/w/index.php?search={query}", "book.closed"),
        ("Search Maps", "maps://?q={query}", "map"),
        ("Define Word", "dict://{query}", "character.book.closed"),
    ]
    .iter()
    .map(|(n, u, i)| Quicklink { name: n.to_string(), url: u.to_string(), icon: i.to_string() })
    .collect()
}

/// Percent-encodes for a URL query component.
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ---- snippets ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snippet {
    pub name: String,
    /// `{clipboard}`, `{date}`, `{time}` and `{datetime}` are filled in when pasted.
    pub text: String,
    #[serde(default)]
    pub keyword: String,
}

pub const SNIPPETS: &str = "snippets.json";

pub fn load_snippets() -> Vec<Snippet> {
    prefs::load(SNIPPETS)
}

pub fn save_snippets(s: &[Snippet]) {
    let _ = prefs::save(SNIPPETS, &s);
}

/// Fills placeholders. `clipboard` is the current clipboard text.
pub fn expand(text: &str, clipboard: &str, date: &str, time: &str) -> String {
    text.replace("{clipboard}", clipboard).replace("{datetime}", &format!("{date} {time}")).replace("{date}", date).replace("{time}", time)
}

// ---- clipboard history ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub text: String,
    /// Copied from Finder: `text` is the file path.
    #[serde(default)]
    pub file: bool,
    pub at: u64,
    #[serde(default)]
    pub pinned: bool,
    /// Frontmost app when it was copied.
    #[serde(default)]
    pub app: String,
}

pub const CLIPBOARD: &str = "clipboard.json";

pub fn load_clips() -> Vec<Clip> {
    prefs::load(CLIPBOARD)
}

/// Adds `c` at the top (moving an identical older entry up) and trims
/// unpinned items beyond `limit`.
pub fn push_clip(list: &mut Vec<Clip>, c: Clip, limit: usize) {
    let pinned = list.iter().find(|x| x.text == c.text).is_some_and(|x| x.pinned);
    list.retain(|x| x.text != c.text);
    list.insert(0, Clip { pinned, ..c });
    let mut kept = 0;
    list.retain(|x| {
        if x.pinned {
            return true;
        }
        kept += 1;
        kept <= limit
    });
}

// ---- colors ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedColor {
    pub color: Rgb,
    pub at: u64,
}

pub const COLORS: &str = "colors.json";

pub fn load_colors() -> Vec<SavedColor> {
    prefs::load(COLORS)
}

/// Newest first, no duplicates, at most 200.
pub fn push_color(list: &mut Vec<SavedColor>, color: Rgb) {
    let hex = color.hex();
    list.retain(|c| c.color.hex() != hex);
    list.insert(0, SavedColor { color, at: now() });
    list.truncate(200);
}

// ---- calculator history ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Calculation {
    pub input: String,
    pub result: String,
    pub at: u64,
}

pub const CALCULATIONS: &str = "calculations.json";

pub fn load_calculations() -> Vec<Calculation> {
    prefs::load(CALCULATIONS)
}

pub fn push_calculation(input: &str, result: &str) {
    let mut list = load_calculations();
    list.retain(|c| c.input != input);
    list.insert(0, Calculation { input: input.to_string(), result: result.to_string(), at: now() });
    list.truncate(100);
    let _ = prefs::save(CALCULATIONS, &list);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frecency_favours_frequent_and_recent() {
        let t = 1_000_000_000;
        let often = Use { count: 20, last: t };
        let once = Use { count: 1, last: t };
        let stale = Use { count: 20, last: t - 90 * 86_400 };
        assert!(bonus(&often, t) > bonus(&once, t));
        assert!(bonus(&often, t) > bonus(&stale, t));
        assert!(bonus(&often, t) <= 400);
    }

    #[test]
    fn clipboard_history_dedupes_and_keeps_pins() {
        let clip = |s: &str| Clip { text: s.into(), file: false, at: 0, pinned: false, app: String::new() };
        let mut l = vec![];
        push_clip(&mut l, clip("a"), 2);
        l[0].pinned = true;
        push_clip(&mut l, clip("b"), 2);
        push_clip(&mut l, clip("c"), 2);
        push_clip(&mut l, clip("d"), 2);
        let texts: Vec<_> = l.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(texts, ["d", "c", "a"], "pinned a survives the limit");
        push_clip(&mut l, clip("a"), 2);
        assert_eq!(l[0].text, "a");
        assert!(l[0].pinned, "re-copying keeps the pin");
    }

    #[test]
    fn placeholders_and_urls() {
        let q = Quicklink { name: "G".into(), url: "https://g.com/?q={query}".into(), icon: String::new() };
        assert_eq!(q.url_for("rust egui & co"), "https://g.com/?q=rust%20egui%20%26%20co");
        assert_eq!(expand("Hi {clipboard} on {date}", "CJ", "2026-10-19", "09:00"), "Hi CJ on 2026-10-19");
    }
}
