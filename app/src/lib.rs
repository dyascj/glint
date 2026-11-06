//! Glint: a launcher for macOS. Everything, one keystroke away.

#[cfg(not(target_os = "macos"))]
compile_error!("Glint is a macOS app: it is built on AppKit, Spotlight and the Accessibility API, and only builds on macOS.");

#[cfg(target_os = "macos")]
pub mod commands;
#[cfg(target_os = "macos")]
pub mod icons;
#[cfg(target_os = "macos")]
pub mod launcher;
#[cfg(target_os = "macos")]
pub mod login;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod paths;
#[cfg(target_os = "macos")]
pub mod prefs;
#[cfg(target_os = "macos")]
pub mod search;
#[cfg(target_os = "macos")]
pub mod settings;
#[cfg(target_os = "macos")]
pub mod ui;
