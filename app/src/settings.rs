//! User settings (settings.json). The launcher watches the file, so a save
//! from the Settings window applies right away.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::prefs;

pub const FILE: &str = "settings.json";
pub const DEFAULT_HOTKEY: &str = "Alt+Space";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileScope {
    /// Your home folder, minus Library and other noise.
    #[default]
    Home,
    /// Every volume Spotlight indexes.
    Everywhere,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorFormat {
    #[default]
    Hex,
    Rgb,
    Hsl,
    Oklch,
    Swift,
}

impl ColorFormat {
    pub const ALL: [ColorFormat; 5] = [ColorFormat::Hex, ColorFormat::Rgb, ColorFormat::Hsl, ColorFormat::Oklch, ColorFormat::Swift];
    pub fn label(self) -> &'static str {
        match self {
            ColorFormat::Hex => "HEX",
            ColorFormat::Rgb => "RGB",
            ColorFormat::Hsl => "HSL",
            ColorFormat::Oklch => "OKLCH",
            ColorFormat::Swift => "SwiftUI",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Opens the launcher, in global-hotkey syntax ("Alt+Space", "Cmd+Shift+K").
    pub hotkey: String,
    pub start_at_login: bool,
    pub show_menu_bar_icon: bool,
    /// Root search shows matching files under the apps and commands.
    pub files_in_root: bool,
    pub file_scope: FileScope,
    pub clipboard_history: bool,
    /// Items kept in clipboard history.
    pub clipboard_limit: usize,
    /// Choosing a clipboard item or snippet pastes it into the front app
    /// (needs Accessibility); otherwise it is only copied.
    pub paste_on_select: bool,
    /// What Pick Color copies.
    pub color_format: ColorFormat,
    /// Command id -> alias typed in the search bar to jump straight to it.
    pub aliases: BTreeMap<String, String>,
    /// Command id -> its own global hotkey.
    pub hotkeys: BTreeMap<String, String>,
    /// Command ids hidden from search.
    pub disabled: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: DEFAULT_HOTKEY.into(),
            start_at_login: true,
            show_menu_bar_icon: true,
            files_in_root: true,
            file_scope: FileScope::Home,
            clipboard_history: true,
            clipboard_limit: 300,
            paste_on_select: true,
            color_format: ColorFormat::Hex,
            aliases: BTreeMap::new(),
            hotkeys: BTreeMap::new(),
            disabled: BTreeSet::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        prefs::load(FILE)
    }

    pub fn save(&self) -> std::io::Result<()> {
        prefs::save(FILE, self)
    }
}

/// "Cmd+Shift+K" -> "⌘ ⇧ K", for keycaps and menus.
pub fn hotkey_label(hotkey: &str) -> String {
    hotkey_parts(hotkey).join(" ")
}

/// Each keycap of a hotkey: modifier symbols in macOS order, then the key.
pub fn hotkey_parts(hotkey: &str) -> Vec<String> {
    let mut mods = [false; 4]; // ⌃ ⌥ ⇧ ⌘
    let mut key = String::new();
    for part in hotkey.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods[0] = true,
            "alt" | "option" => mods[1] = true,
            "shift" => mods[2] = true,
            "cmd" | "command" | "super" => mods[3] = true,
            _ => key = key_label(part),
        }
    }
    let mut out: Vec<String> = ["⌃", "⌥", "⇧", "⌘"].iter().zip(mods).filter(|(_, on)| *on).map(|(s, _)| s.to_string()).collect();
    if !key.is_empty() {
        out.push(key);
    }
    out
}

fn key_label(key: &str) -> String {
    let k = key.strip_prefix("Key").or_else(|| key.strip_prefix("Digit")).unwrap_or(key);
    match k {
        "Space" => "Space".into(),
        "ArrowUp" => "↑".into(),
        "ArrowDown" => "↓".into(),
        "ArrowLeft" => "←".into(),
        "ArrowRight" => "→".into(),
        "Enter" => "↵".into(),
        "Backspace" => "⌫".into(),
        "Escape" => "⎋".into(),
        "Tab" => "⇥".into(),
        "Comma" => ",".into(),
        "Period" => ".".into(),
        "Slash" => "/".into(),
        "Semicolon" => ";".into(),
        "Quote" => "'".into(),
        "Backquote" => "`".into(),
        "Minus" => "-".into(),
        "Equal" => "=".into(),
        "BracketLeft" => "[".into(),
        "BracketRight" => "]".into(),
        "Backslash" => "\\".into(),
        other => other.to_uppercase(),
    }
}

/// Builds a hotkey string from a pressed key, or None when it can't be a
/// global hotkey (no modifier, or a key the hotkey API doesn't know).
pub fn hotkey_from(key: &str, ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Option<String> {
    if !(ctrl || alt || cmd) {
        return None;
    }
    let code = match key {
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphabetic()) => format!("Key{}", k.to_ascii_uppercase()),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_digit()) => format!("Digit{k}"),
        "Space" | "Enter" | "Tab" | "Backspace" | "Escape" | "Comma" | "Period" | "Slash" | "Semicolon" | "Quote" | "Backquote"
        | "Minus" | "Equal" | "BracketLeft" | "BracketRight" | "Backslash" | "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" => {
            key.to_string()
        }
        k if k.starts_with('F') && k[1..].parse::<u8>().is_ok_and(|n| (1..=20).contains(&n)) => k.to_string(),
        _ => return None,
    };
    let mut parts = Vec::new();
    for (on, name) in [(ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift"), (cmd, "Cmd")] {
        if on {
            parts.push(name);
        }
    }
    parts.push(&code);
    Some(parts.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_round_trip_to_labels() {
        assert_eq!(hotkey_label("Alt+Space"), "⌥ Space");
        assert_eq!(hotkey_label("Cmd+Shift+KeyK"), "⇧ ⌘ K");
        assert_eq!(hotkey_from("K", false, true, true, false).as_deref(), Some("Alt+Shift+KeyK"));
        assert_eq!(hotkey_from("Space", false, false, false, true).as_deref(), Some("Cmd+Space"));
        assert_eq!(hotkey_from("K", false, false, true, false), None, "shift alone is not a hotkey");
        let parsed: global_hotkey::hotkey::HotKey = "Alt+Shift+KeyK".parse().unwrap();
        assert!(parsed.mods.contains(global_hotkey::hotkey::Modifiers::ALT));
        assert!(DEFAULT_HOTKEY.parse::<global_hotkey::hotkey::HotKey>().is_ok());
    }

    #[test]
    fn settings_survive_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"hotkey":"Cmd+Space"}"#).unwrap();
        assert_eq!(s.hotkey, "Cmd+Space");
        assert!(s.files_in_root);
        assert_eq!(s.clipboard_limit, 300);
    }
}
