//! Built-in commands: every one shows in root search and can get an alias
//! or its own hotkey in Settings.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    // Views
    SearchFiles,
    ClipboardHistory,
    SearchMenuItems,
    SwitchWindows,
    SearchEmoji,
    SavedColors,
    CalculatorHistory,
    SearchQuicklinks,
    CreateQuicklink,
    SearchSnippets,
    CreateSnippet,
    Extensions,
    CreateExtension,
    QuitApplications,
    // Actions
    PickColor,
    OpenExtensionsFolder,
    ReloadExtensions,
    ClearClipboardHistory,
    LockScreen,
    Sleep,
    SleepDisplays,
    Restart,
    ShutDown,
    LogOut,
    EmptyTrash,
    EjectAllDisks,
    ToggleAppearance,
    ToggleHiddenFiles,
    HideOthers,
    UnhideAll,
    QuitAllApps,
    ToggleMute,
    VolumeUp,
    VolumeDown,
    ScreenSaver,
    Settings,
    ShowOnboarding,
    CheckForUpdates,
    About,
    QuitGlint,
}

pub struct Def {
    pub cmd: Cmd,
    pub slug: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    /// SF Symbol name.
    pub symbol: &'static str,
    pub keywords: &'static [&'static str],
    /// Asks before running.
    pub confirm: Option<&'static str>,
}

const fn d(cmd: Cmd, slug: &'static str, title: &'static str, subtitle: &'static str, symbol: &'static str, keywords: &'static [&'static str]) -> Def {
    Def { cmd, slug, title, subtitle, symbol, keywords, confirm: None }
}

const fn c(cmd: Cmd, slug: &'static str, title: &'static str, subtitle: &'static str, symbol: &'static str, keywords: &'static [&'static str], confirm: &'static str) -> Def {
    Def { cmd, slug, title, subtitle, symbol, keywords, confirm: Some(confirm) }
}

pub const ALL: &[Def] = &[
    d(Cmd::SearchFiles, "search-files", "Search Files", "Files", "doc.text.magnifyingglass", &["find", "finder", "documents", "spotlight"]),
    d(Cmd::ClipboardHistory, "clipboard-history", "Clipboard History", "Clipboard", "doc.on.clipboard", &["copy", "paste", "pasteboard"]),
    d(Cmd::SearchMenuItems, "search-menu-items", "Search Menu Items", "Menus", "filemenu.and.selection", &["menu bar", "commands", "actions", "menubar"]),
    d(Cmd::SwitchWindows, "switch-windows", "Switch Windows", "Windows", "macwindow.on.rectangle", &["window switcher", "alt tab", "windows", "focus"]),
    d(Cmd::PickColor, "pick-color", "Pick Color", "Colors", "eyedropper", &["eyedropper", "color picker", "colour", "sample"]),
    d(Cmd::SavedColors, "saved-colors", "Saved Colors", "Colors", "swatchpalette", &["color history", "swatches", "palette", "colour"]),
    d(Cmd::SearchEmoji, "search-emoji", "Search Emoji & Symbols", "Emoji", "face.smiling", &["emoji", "symbols", "smiley"]),
    d(Cmd::CalculatorHistory, "calculator-history", "Calculator History", "Calculator", "plus.forwardslash.minus", &["math", "calc"]),
    d(Cmd::SearchQuicklinks, "search-quicklinks", "Search Quicklinks", "Quicklinks", "link", &["bookmarks", "urls", "web"]),
    d(Cmd::CreateQuicklink, "create-quicklink", "Create Quicklink", "Quicklinks", "link.badge.plus", &["bookmark", "url", "web search"]),
    d(Cmd::SearchSnippets, "search-snippets", "Search Snippets", "Snippets", "text.quote", &["text", "templates"]),
    d(Cmd::CreateSnippet, "create-snippet", "Create Snippet", "Snippets", "text.badge.plus", &["text", "template"]),
    d(Cmd::Extensions, "extensions", "Extensions", "Extensions", "puzzlepiece.extension", &["script commands", "scripts", "plugins"]),
    d(Cmd::CreateExtension, "create-extension", "Create Extension", "Extensions", "hammer", &["script command", "new script", "plugin"]),
    d(Cmd::OpenExtensionsFolder, "open-extensions-folder", "Open Extensions Folder", "Extensions", "folder", &["script commands"]),
    d(Cmd::ReloadExtensions, "reload-extensions", "Reload Extensions", "Extensions", "arrow.clockwise", &["script commands", "refresh"]),
    d(Cmd::QuitApplications, "quit-applications", "Quit Applications", "Applications", "xmark.app", &["force quit", "kill", "process", "close"]),
    d(Cmd::ClearClipboardHistory, "clear-clipboard-history", "Clear Clipboard History", "Clipboard", "trash", &["delete", "wipe"]),
    d(Cmd::LockScreen, "lock-screen", "Lock Screen", "System", "lock", &["lock"]),
    d(Cmd::Sleep, "sleep", "Sleep", "System", "moon", &["suspend"]),
    d(Cmd::SleepDisplays, "sleep-displays", "Sleep Displays", "System", "display", &["screen off", "monitor"]),
    c(Cmd::Restart, "restart", "Restart", "System", "arrow.counterclockwise", &["reboot"], "Restart your Mac?"),
    c(Cmd::ShutDown, "shut-down", "Shut Down", "System", "power", &["power off", "turn off"], "Shut down your Mac?"),
    c(Cmd::LogOut, "log-out", "Log Out", "System", "rectangle.portrait.and.arrow.right", &["sign out", "logout"], "Log out of your Mac?"),
    c(Cmd::EmptyTrash, "empty-trash", "Empty Trash", "System", "trash", &["bin", "delete"], "Empty the Trash? This can't be undone."),
    d(Cmd::EjectAllDisks, "eject-all-disks", "Eject All Disks", "System", "eject", &["unmount", "drives", "usb"]),
    d(Cmd::ToggleAppearance, "toggle-appearance", "Toggle System Appearance", "System", "circle.lefthalf.filled", &["dark mode", "light mode", "theme"]),
    d(Cmd::ToggleHiddenFiles, "toggle-hidden-files", "Toggle Hidden Files", "System", "eye", &["dotfiles", "finder", "show all files"]),
    d(Cmd::HideOthers, "hide-others", "Hide All Apps Except Frontmost", "System", "rectangle.on.rectangle.slash", &["hide others", "focus"]),
    d(Cmd::UnhideAll, "unhide-all", "Unhide All Hidden Apps", "System", "rectangle.on.rectangle", &["show all"]),
    c(Cmd::QuitAllApps, "quit-all-apps", "Quit All Apps", "System", "xmark.circle", &["close everything"], "Quit every open app?"),
    d(Cmd::ToggleMute, "toggle-mute", "Toggle Mute", "System", "speaker.slash", &["sound", "volume", "silence"]),
    d(Cmd::VolumeUp, "volume-up", "Turn Volume Up", "System", "speaker.wave.3", &["sound", "louder"]),
    d(Cmd::VolumeDown, "volume-down", "Turn Volume Down", "System", "speaker.wave.1", &["sound", "quieter"]),
    d(Cmd::ScreenSaver, "screen-saver", "Start Screen Saver", "System", "sparkles.tv", &["screensaver"]),
    d(Cmd::Settings, "settings", "Glint Settings", "Glint", "gearshape", &["preferences", "config", "hotkey"]),
    d(Cmd::ShowOnboarding, "onboarding", "Show Onboarding", "Glint", "hand.wave", &["welcome", "tour", "intro"]),
    d(Cmd::CheckForUpdates, "check-for-updates", "Check for Updates", "Glint", "arrow.down.circle", &["upgrade", "version"]),
    d(Cmd::About, "about", "About Glint", "Glint", "info.circle", &["version"]),
    d(Cmd::QuitGlint, "quit", "Quit Glint", "Glint", "power", &["exit", "close"]),
];

pub fn def(cmd: Cmd) -> &'static Def {
    ALL.iter().find(|d| d.cmd == cmd).expect("every command has a definition")
}

impl Def {
    pub fn id(&self) -> String {
        format!("glint:{}", self.slug)
    }
}

pub fn by_id(id: &str) -> Option<&'static Def> {
    let slug = id.strip_prefix("glint:")?;
    ALL.iter().find(|d| d.slug == slug)
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs_are_unique() {
        let mut s: Vec<_> = super::ALL.iter().map(|d| d.slug).collect();
        let n = s.len();
        s.sort();
        s.dedup();
        assert_eq!(n, s.len());
        assert!(super::by_id("glint:pick-color").is_some());
    }
}
