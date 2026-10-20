//! Installed applications and System Settings panes. The app list is a plain
//! directory scan (a few ms), redone when the launcher opens if it's stale.

use std::path::{Path, PathBuf};

use crate::paths;

#[derive(Clone, Debug)]
pub struct App {
    pub name: String,
    pub path: PathBuf,
}

fn roots() -> Vec<PathBuf> {
    let mut r: Vec<PathBuf> = [
        "/Applications",
        "/System/Applications",
        "/System/Applications/Utilities",
        "/System/Library/CoreServices/Applications",
        "/Applications/Utilities",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    r.push(paths::home().join("Applications"));
    r
}

/// Every .app in the usual folders, one level of subfolders deep (Adobe-style
/// suites keep apps in a folder), plus Finder. Deduplicated by name.
pub fn scan() -> Vec<App> {
    let mut out: Vec<App> = Vec::new();
    let mut push = |p: PathBuf| {
        if let Some(name) = p.file_stem().and_then(|s| s.to_str()) {
            if !out.iter().any(|a| a.name == name) {
                out.push(App { name: name.to_string(), path: p.clone() });
            }
        }
    };
    for root in roots() {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if is_app(&p) {
                push(p);
            } else if p.is_dir() && !hidden(&p) && root.ends_with("Applications") {
                for sub in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                    if is_app(&sub.path()) {
                        push(sub.path());
                    }
                }
            }
        }
    }
    push(PathBuf::from("/System/Library/CoreServices/Finder.app"));
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn is_app(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "app") && !hidden(p)
}

fn hidden(p: &Path) -> bool {
    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'))
}

/// (title, settings URL id, keywords)
pub const SETTINGS_PANES: &[(&str, &str, &str)] = &[
    ("Wi-Fi", "com.apple.wifi-settings-extension", "wireless network internet"),
    ("Bluetooth", "com.apple.BluetoothSettings", "devices headphones"),
    ("Network", "com.apple.Network-Settings.extension", "ethernet vpn proxy"),
    ("Notifications", "com.apple.Notifications-Settings.extension", "alerts banners"),
    ("Sound", "com.apple.Sound-Settings.extension", "audio volume output input"),
    ("Focus", "com.apple.Focus-Settings.extension", "do not disturb"),
    ("Screen Time", "com.apple.Screen-Time-Settings.extension", "limits"),
    ("General", "com.apple.systempreferences.GeneralSettings", "about"),
    ("Appearance", "com.apple.Appearance-Settings.extension", "dark mode light accent"),
    ("Accessibility", "com.apple.Accessibility-Settings.extension", "zoom voiceover"),
    ("Control Center", "com.apple.ControlCenter-Settings.extension", "menu bar"),
    ("Siri", "com.apple.Siri-Settings.extension", "apple intelligence"),
    ("Privacy & Security", "com.apple.settings.PrivacySecurity.extension", "permissions accessibility full disk access"),
    ("Desktop & Dock", "com.apple.Desktop-Settings.extension", "stage manager mission control hot corners"),
    ("Displays", "com.apple.Displays-Settings.extension", "monitor resolution night shift"),
    ("Wallpaper", "com.apple.Wallpaper-Settings.extension", "background"),
    ("Battery", "com.apple.Battery-Settings.extension", "energy power"),
    ("Lock Screen", "com.apple.Lock-Screen-Settings.extension", "screen saver password"),
    ("Users & Groups", "com.apple.Users-Groups-Settings.extension", "accounts"),
    ("Passwords", "com.apple.Passwords-Settings.extension", "keychain"),
    ("Keyboard", "com.apple.Keyboard-Settings.extension", "shortcuts input sources dictation"),
    ("Trackpad", "com.apple.Trackpad-Settings.extension", "gestures click"),
    ("Mouse", "com.apple.Mouse-Settings.extension", "scroll"),
    ("Printers & Scanners", "com.apple.Print-Scan-Settings.extension", "print"),
    ("Software Update", "com.apple.Software-Update-Settings.extension", "upgrade macos"),
    ("Storage", "com.apple.settings.Storage", "disk space"),
    ("Login Items", "com.apple.LoginItems-Settings.extension", "startup extensions"),
    ("Date & Time", "com.apple.Date-Time-Settings.extension", "clock timezone"),
    ("Sharing", "com.apple.Sharing-Settings.extension", "airdrop file sharing remote"),
    ("Time Machine", "com.apple.Time-Machine-Settings.extension", "backup"),
];

pub fn settings_url(id: &str) -> String {
    format!("x-apple.systempreferences:{id}")
}

pub const SETTINGS_APP: &str = "/System/Applications/System Settings.app";

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_system_apps() {
        let apps = super::scan();
        assert!(apps.iter().any(|a| a.name == "Finder"));
        assert!(apps.iter().any(|a| a.name == "System Settings"));
        let mut names: Vec<_> = apps.iter().map(|a| &a.name).collect();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len(), "no duplicates");
    }
}
