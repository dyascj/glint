//! The launcher process: menu bar item, global hotkeys, clipboard watcher
//! and the search window itself. The window stays loaded while hidden, so
//! the hotkey shows it instantly.
//!
//! Everything lives in [`Launcher`] on the main thread. Background threads
//! (hotkeys, file search, scripts, watchers) talk to it with [`send`].

mod items;
mod tray;
pub mod updates;
mod view;
mod window;

pub use window::blur;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use objc2::MainThreadMarker;
use serde::{Deserialize, Serialize};

use crate::commands::{self, Cmd};
use crate::icons::Icons;
use crate::macos::{self, Img, Running};
use crate::paths::{self, APP_NAME};
use crate::prefs::{self, Prefs};
use crate::search::apps::{self, App};
use crate::search::ax::{self, MenuItem, Window};
use crate::search::color::Rgb;
use crate::search::data::{self, Calculation, Clip, Frecency, Quicklink, SavedColor, Snippet};
use crate::search::files::FileSearch;
use crate::search::scripts::{self, ListItem, Mode, Output, Script};
use crate::settings::Settings;
use items::{Run, Section};

pub enum Msg {
    Toggle,
    Show,
    /// Glint.app was opened (Finder, Dock) while running.
    Activated,
    Hotkey(u32),
    Menu(String),
    Clip(Clip),
    Hud(String),
    PickedColor(Option<Rgb>),
    ScriptDone(u64, Output),
    ListDone(u64, Result<Vec<ListItem>, String>),
    InlineDone(PathBuf, String),
    MenusDone(u64, i32, Result<Vec<MenuItem>, String>),
    WindowsDone(u64, Vec<Window>),
    UpdateFound(updates::Release),
    UpdateDone(updates::Release, Result<(), String>),
    SettingsChanged,
    ExtensionsChanged,
    DataChanged,
    Apps(Vec<App>),
}

static TX: OnceLock<Sender<Msg>> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();
static CLIPBOARD_ON: AtomicBool = AtomicBool::new(true);

/// From any thread: hand a message to the launcher and wake it.
pub fn send(m: Msg) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(m);
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgsFor {
    Script(usize),
    Quicklink(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    Quicklink,
    Snippet,
    Extension,
}

#[derive(Clone, Debug)]
pub struct Form {
    pub kind: FormKind,
    /// (label, value, placeholder, multiline)
    pub fields: Vec<(&'static str, String, &'static str, bool)>,
    /// Extension mode chip.
    pub choice: usize,
    pub focus: usize,
    pub error: Option<String>,
}

pub const EXTENSION_MODES: [(&str, &str); 4] = [("fullOutput", "Full output"), ("list", "List"), ("compact", "Compact"), ("silent", "Silent")];

#[derive(Clone, Debug)]
pub enum View {
    Root,
    Files,
    Clipboard,
    Emoji,
    Colors,
    Calculations,
    Quicklinks,
    Snippets,
    Extensions,
    Processes,
    Ramp(Rgb),
    /// Extension views hold the script's path; indices shift when the folder reloads.
    Output { title: String, text: Option<String>, ok: bool, rerun: (PathBuf, Vec<String>), token: u64 },
    List { script: PathBuf, items: Vec<ListItem>, error: Option<String>, token: u64, asked: Option<String>, due: Option<f64> },
    /// The front app's menu bar; None while it's being read.
    Menus { app: String, items: Option<Vec<MenuItem>>, error: Option<String>, token: u64 },
    Windows { items: Option<Vec<Window>>, token: u64 },
    Args { title: String, target: ArgsFor, values: Vec<String>, placeholders: Vec<String>, secure: Vec<bool>, focus: usize },
    Form(Form),
}

impl View {
    pub fn title(&self) -> String {
        let def = |c| commands::def(c).title.to_string();
        match self {
            View::Root => APP_NAME.into(),
            View::Files => def(Cmd::SearchFiles),
            View::Clipboard => def(Cmd::ClipboardHistory),
            View::Emoji => def(Cmd::SearchEmoji),
            View::Colors => def(Cmd::SavedColors),
            View::Calculations => def(Cmd::CalculatorHistory),
            View::Quicklinks => def(Cmd::SearchQuicklinks),
            View::Snippets => def(Cmd::SearchSnippets),
            View::Extensions => def(Cmd::Extensions),
            View::Processes => def(Cmd::QuitApplications),
            View::Menus { .. } => def(Cmd::SearchMenuItems),
            View::Windows { .. } => def(Cmd::SwitchWindows),
            View::Ramp(c) => format!("Shades of {}", c.hex()),
            View::Output { title, .. } | View::Args { title, .. } => title.clone(),
            View::List { .. } => "Extension".into(),
            View::Form(f) => match f.kind {
                FormKind::Quicklink => def(Cmd::CreateQuicklink),
                FormKind::Snippet => def(Cmd::CreateSnippet),
                FormKind::Extension => def(Cmd::CreateExtension),
            },
        }
    }
}

pub struct Page {
    pub view: View,
    pub query: String,
    pub selected: usize,
}

impl Page {
    fn new(view: View) -> Page {
        Page { view, query: String::new(), selected: 0 }
    }
}

/// What the onboarding window watches to tick off "try it" steps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Runtime {
    pub pid: i32,
    pub tried: BTreeSet<String>,
    pub hotkey_error: Option<String>,
    pub update_available: Option<String>,
    pub update_status: Option<String>,
}

/// Signals the windows send the launcher (Slat's scheme).
pub const SIG_SHOW: i32 = libc::SIGUSR1;
pub const SIG_CHECK_UPDATES: i32 = libc::SIGHUP;
pub const SIG_INSTALL_UPDATE: i32 = libc::SIGINFO;

pub const RUNTIME: &str = "runtime.json";

pub struct ActionsPanel {
    pub query: String,
    pub selected: usize,
}

pub struct Launcher {
    rx: Receiver<Msg>,
    pub stack: Vec<Page>,
    pub settings: Settings,
    settings_mtime: Option<SystemTime>,
    pub apps: Vec<App>,
    apps_at: Instant,
    pub scripts: Vec<Script>,
    pub inline: HashMap<PathBuf, String>,
    pub quicklinks: Vec<Quicklink>,
    pub snippets: Vec<Snippet>,
    pub clips: Vec<Clip>,
    pub colors: Vec<SavedColor>,
    pub calculations: Vec<Calculation>,
    pub frecency: Frecency,
    pub favorites: Vec<String>,
    pub running: Vec<Running>,
    /// The last menus read, shown at once next time while they refresh.
    menu_cache: Option<(i32, Vec<MenuItem>)>,
    files: FileSearch,
    file_token: u64,
    pub file_results: Vec<PathBuf>,
    file_for: String,
    /// How many results `file_results` was asked for.
    file_limit: usize,
    pub file_due: Option<f64>,
    pub file_loading: bool,
    pub sections: Vec<Section>,
    /// The emoji grid's sections, rebuilt with `sections`.
    pub emoji: Vec<(String, Vec<view::Emoji>)>,
    sections_key: (usize, String, u64),
    pub epoch: u64,
    pub icons: Icons,
    pub actions: Option<ActionsPanel>,
    pub confirm: Option<Cmd>,
    pub toast: Option<(String, f64, bool)>,
    hotkeys: Option<GlobalHotKeyManager>,
    registered: Vec<(HotKey, String)>,
    tray: Option<tray_icon::TrayIcon>,
    pub update: updates::Status,
    windows: HashMap<String, Child>,
    pub shown: bool,
    pub shown_at: f64,
    was_focused: bool,
    prev_app: Option<(String, i32)>,
    pub focus_query: bool,
    pub scroll_to_selected: bool,
    token: u64,
    runtime: Runtime,
    started: bool,
    parked: bool,
}

pub fn run() {
    paths::ensure_dirs();
    // One launcher at a time: a second launch shows the running one.
    let rt: Runtime = prefs::load(RUNTIME);
    // SAFETY: kill with signal 0 only probes the pid.
    if rt.pid > 0 && rt.pid != std::process::id() as i32 && unsafe { libc::kill(rt.pid, 0) } == 0 && is_glint(rt.pid) {
        // SAFETY: as above, now asking it to show.
        unsafe { libc::kill(rt.pid, SIG_SHOW) };
        return;
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_inner_size(window::SIZE)
        .with_resizable(false)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_titlebar_buttons_shown(false)
        .with_transparent(true)
        .with_visible(false);
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        event_loop_builder: Some(Box::new(|b| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            b.with_activation_policy(ActivationPolicy::Accessory);
        })),
        ..Default::default()
    };
    let result = eframe::run_native(
        APP_NAME,
        options,
        Box::new(|cc| {
            crate::ui::theme::install(&cc.egui_ctx);
            Ok(Box::new(Launcher::new(&cc.egui_ctx)))
        }),
    );
    if let Err(e) = result {
        eprintln!("{APP_NAME}: launcher failed: {e}");
    }
}

fn is_glint(pid: i32) -> bool {
    Command::new("/bin/ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("Glint"))
}

impl Launcher {
    fn new(ctx: &egui::Context) -> Launcher {
        let (tx, rx) = channel();
        let _ = TX.set(tx);
        let _ = CTX.set(ctx.clone());
        let settings = Settings::load();
        CLIPBOARD_ON.store(settings.clipboard_history, Ordering::Relaxed);
        let notify_ctx = ctx.clone();
        let mut l = Launcher {
            rx,
            stack: vec![Page::new(View::Root)],
            settings_mtime: mtime(&paths::settings_file()),
            settings,
            apps: apps::scan(),
            apps_at: Instant::now(),
            scripts: scripts::scan(),
            inline: HashMap::new(),
            quicklinks: data::load_quicklinks(),
            snippets: data::load_snippets(),
            clips: data::load_clips(),
            colors: data::load_colors(),
            calculations: Vec::new(),
            frecency: Frecency::load(),
            favorites: data::load_favorites(),
            running: Vec::new(),
            menu_cache: None,
            files: FileSearch::new(move || notify_ctx.request_repaint()),
            file_token: 0,
            file_results: Vec::new(),
            file_for: String::new(),
            file_limit: 0,
            file_due: None,
            file_loading: false,
            sections: Vec::new(),
            emoji: Vec::new(),
            sections_key: (usize::MAX, String::new(), 0),
            epoch: 0,
            icons: Icons::new(ctx),
            actions: None,
            confirm: None,
            toast: None,
            hotkeys: None,
            registered: Vec::new(),
            tray: None,
            update: updates::Status::Idle,
            windows: HashMap::new(),
            shown: false,
            shown_at: 0.0,
            was_focused: false,
            prev_app: None,
            focus_query: true,
            scroll_to_selected: false,
            token: 0,
            runtime: Runtime { pid: std::process::id() as i32, ..Runtime::default() },
            started: false,
            parked: false,
        };
        // App icons render in the background before you ever open the window.
        let warm: Vec<Img> = l.apps.iter().map(|a| Img::File(a.path.clone())).collect();
        l.icons.warm(warm);
        l.icons.warm(commands::ALL.iter().map(|d| Img::Symbol(d.symbol)));
        l
    }

    /// Runs once the event loop is up: things that need a running NSApp.
    fn start(&mut self, ctx: &egui::Context) {
        self.started = true;
        if let Some(mtm) = MainThreadMarker::new() {
            window::configure(mtm);
        }
        match GlobalHotKeyManager::new() {
            Ok(m) => self.hotkeys = Some(m),
            Err(e) => eprintln!("{APP_NAME}: hotkeys unavailable: {e}"),
        }
        GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
            if e.state == HotKeyState::Pressed {
                send(Msg::Hotkey(e.id));
            }
        }));
        tray_icon::menu::MenuEvent::set_event_handler(Some(|e: tray_icon::menu::MenuEvent| send(Msg::Menu(e.id.0))));
        self.register_hotkeys();
        self.sync_tray();
        self.write_runtime();
        updates::start();
        spawn_watchers();
        listen_for_signals();
        observe_activation();
        if cfg!(debug_assertions) && std::env::var_os("GLINT_HUD_SNAPSHOT").is_some() {
            self.hud("You're on the latest version");
        }
        if snapshotting() {
            // Dev snapshots: open straight onto a query or command (GLINT_QUERY, GLINT_VIEW).
            self.show(ctx.input(|i| i.time));
            if let Some(view) = std::env::var("GLINT_VIEW").ok().and_then(|v| commands::by_id(&format!("glint:{v}"))) {
                self.run_cmd(view.cmd, 0.0);
            }
            self.page_mut().query = std::env::var("GLINT_QUERY").unwrap_or_default();
            if let Ok(n) = std::env::var("GLINT_SELECT") {
                self.page_mut().selected = n.parse().unwrap_or(0);
            }
            if std::env::var_os("GLINT_ACTIONS").is_some() {
                self.actions = Some(ActionsPanel { query: String::new(), selected: 0 });
            }
        } else if !Prefs::load().onboarding_completed {
            self.open_window("onboarding", None);
        }
    }

    // ---- show / hide ----

    pub fn show(&mut self, now: f64) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if self.shown && window::is_shown(mtm) {
            return;
        }
        // Remember who had focus so "Hide others" and pasting go back to them.
        self.prev_app = macos::frontmost().filter(|(_, pid)| *pid != std::process::id() as i32);
        // Every open starts fresh: root page, empty search.
        self.stack.truncate(1);
        self.stack[0].query.clear();
        self.stack[0].selected = 0;
        if self.apps_at.elapsed() > Duration::from_secs(30) {
            // Rescan in the background; the window shouldn't wait on the disk.
            self.apps_at = Instant::now();
            std::thread::spawn(|| send(Msg::Apps(apps::scan())));
        }
        self.running = macos::running_apps();
        self.refresh_inline();
        self.actions = None;
        self.confirm = None;
        self.focus_query = true;
        self.shown = true;
        self.shown_at = now;
        self.was_focused = false;
        self.epoch += 1;
        window::show(mtm);
        self.tried("open");
    }

    pub fn hide(&mut self, _now: f64) {
        if !self.shown {
            return;
        }
        self.shown = false;
        self.actions = None;
        self.confirm = None;
        if let Some(mtm) = MainThreadMarker::new() {
            window::hide(mtm);
        }
    }

    fn toggle(&mut self, now: f64) {
        let visible = MainThreadMarker::new().is_some_and(window::is_shown);
        if self.shown && visible { self.hide(now) } else { self.show(now) }
    }

    pub fn hud(&self, text: &str) {
        if let Some(mtm) = MainThreadMarker::new() {
            window::hud(mtm, text);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, ok: bool, now: f64) {
        self.toast = Some((text.into(), now, ok));
    }

    fn tried(&mut self, what: &str) {
        // Only the first time matters (onboarding ticks it off).
        if self.runtime.tried.insert(what.to_string()) {
            self.write_runtime();
        }
    }

    fn write_runtime(&self) {
        let _ = prefs::save(RUNTIME, &self.runtime);
    }

    // ---- navigation ----

    pub fn page(&self) -> &Page {
        self.stack.last().expect("root page")
    }

    pub fn page_mut(&mut self) -> &mut Page {
        self.stack.last_mut().expect("root page")
    }

    pub fn push(&mut self, view: View, query: String) {
        if matches!(view, View::Calculations) {
            self.calculations = data::load_calculations();
        }
        if matches!(view, View::Processes) {
            self.running = macos::running_apps();
        }
        self.stack.push(Page { view, query, selected: 0 });
        self.actions = None;
        self.focus_query = true;
        self.epoch += 1;
    }

    pub fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
            self.actions = None;
            self.focus_query = true;
            self.epoch += 1;
        }
    }

    /// Rebuilds the visible rows when the page, query or data changed.
    pub fn refresh_sections(&mut self, now: f64) {
        let key = (self.stack.len(), self.page().query.clone(), self.epoch);
        self.schedule_file_search(now);
        if key == self.sections_key {
            return;
        }
        let q = self.page().query.clone();
        let sections = match &self.page().view {
            View::Root => items::root(
                &q,
                &items::Sources {
                    apps: &self.apps,
                    scripts: &self.scripts,
                    inline: &self.inline,
                    quicklinks: &self.quicklinks,
                    // The last results stay up while the next search runs, so rows don't flash.
                    files: &self.file_results,
                    running: &self.running,
                    settings: &self.settings,
                    frecency: &self.frecency,
                    favorites: &self.favorites,
                },
            ),
            View::Files => items::files_view(&self.file_results),
            View::Clipboard => items::clipboard_view(&self.clips, &q),
            View::Quicklinks => items::quicklinks_view(&self.quicklinks, &q),
            View::Snippets => items::snippets_view(&self.snippets, &q),
            View::Extensions => items::extensions_view(&self.scripts, &self.inline, &q),
            View::Processes => items::processes_view(&self.running, &q),
            View::Calculations => items::calculations_view(&self.calculations, &q),
            View::List { items: li, .. } => items::list_view(li, &q),
            View::Menus { items: m, error, .. } => items::menus_view(m.as_deref(), error.as_deref(), macos::ax_trusted(), &q),
            View::Windows { items: w, .. } => items::windows_view(w.as_deref(), macos::ax_trusted(), &q),
            View::Ramp(c) => items::ramp_view(*c, self.settings.color_format),
            _ => Vec::new(),
        };
        let same_page = self.sections_key.0 == key.0 && self.sections_key.1 == key.1;
        // New data for the same text (files landing, a new clip) keeps the
        // selection on the same row instead of whatever slid under it.
        let keep = if same_page { self.selected_item().map(|it| (it.id.clone(), it.title.clone())) } else { None };
        self.sections = sections;
        self.sections_key = key;
        self.emoji = if matches!(self.page().view, View::Emoji) { self.emoji_sections() } else { Vec::new() };
        let found = keep.and_then(|(id, title)| self.sections.iter().flat_map(|s| &s.items).position(|it| it.id == id && it.title == title));
        let n = self.sections.iter().map(|s| s.items.len()).sum::<usize>();
        // Grids (emoji, colors) have no sections; they clamp themselves.
        let grid = matches!(self.page().view, View::Emoji | View::Colors);
        let page = self.page_mut();
        if !same_page {
            // New text: the best match is on top.
            page.selected = 0;
        }
        if let Some(i) = found {
            page.selected = i;
        }
        if !grid {
            page.selected = page.selected.min(n.saturating_sub(1));
        }
    }

    /// File search runs a beat after typing stops, for root and Search Files.
    fn schedule_file_search(&mut self, now: f64) {
        let wants = match &self.page().view {
            View::Root => self.settings.files_in_root,
            View::Files => true,
            _ => false,
        };
        let q = self.page().query.trim().to_string();
        if let Some(found) = self.files.take(self.file_token) {
            self.file_results = found;
            self.file_loading = false;
            self.epoch += 1;
        }
        if !wants || q.chars().count() < 2 {
            self.file_due = None;
            if !self.file_results.is_empty() && q.chars().count() < 2 {
                self.file_results.clear();
                self.file_for.clear();
                self.epoch += 1;
            }
            return;
        }
        let limit = if matches!(self.page().view, View::Files) { 60 } else { 12 };
        if q != self.file_for || limit > self.file_limit {
            if self.file_due.is_none() {
                self.file_due = Some(now + 0.12);
            }
            if self.file_due.is_some_and(|d| now >= d) {
                self.file_due = None;
                self.file_token += 1;
                self.file_for = q.clone();
                self.file_limit = limit;
                self.file_loading = true;
                self.files.request(self.file_token, &q, self.settings.file_scope, limit);
                if matches!(self.page().view, View::Files) {
                    self.tried("file");
                }
            } else if let Some(ctx) = CTX.get() {
                ctx.request_repaint_after(Duration::from_millis(40));
            }
        }
    }

    pub fn item_count(&self) -> usize {
        self.sections.iter().map(|s| s.items.len()).sum()
    }

    pub fn selected_item(&self) -> Option<&items::Item> {
        let mut i = self.page().selected;
        for s in &self.sections {
            if i < s.items.len() {
                return s.items.get(i);
            }
            i -= s.items.len();
        }
        None
    }

    // ---- running things ----

    pub fn run(&mut self, run: Run, id: &str, now: f64) {
        if !id.is_empty() {
            self.frecency.record(id);
        }
        if id.starts_with("app:") {
            self.tried("app");
        }
        match run {
            Run::OpenPath(p) => {
                self.hide(now);
                open_path_fast(&p);
            }
            Run::OpenUrl(u) => {
                self.hide(now);
                macos::open(&u);
            }
            Run::Reveal(p) => {
                self.hide(now);
                macos::reveal(&p);
            }
            Run::QuickLook(p) => {
                self.hide(now);
                macos::quick_look(&p);
            }
            Run::Trash(p) => {
                if macos::trash(&p) {
                    self.toast("Moved to Trash", true, now);
                    self.file_for.clear();
                    self.file_results.retain(|f| f != &p);
                    self.epoch += 1;
                } else {
                    self.toast("Couldn't move it to the Trash", false, now);
                }
            }
            Run::OpenInTerminal(p) => {
                self.hide(now);
                macos::open_in("Terminal", &p);
            }
            Run::EditFile(p) => {
                self.hide(now);
                macos::open_with(&["-t".as_ref(), p.as_os_str()]);
            }
            Run::Copy(s) => {
                macos::copy(&s);
                self.hide(now);
                self.hud("Copied to Clipboard");
            }
            Run::PasteFile(p) => {
                macos::copy_file(&p);
                self.hide(now);
                if self.settings.paste_on_select && macos::ax_trusted() {
                    std::thread::spawn(|| {
                        std::thread::sleep(Duration::from_millis(160));
                        macos::paste();
                    });
                } else {
                    self.hud("Copied File");
                }
            }
            Run::CopyFile(p) => {
                macos::copy_file(&p);
                self.hide(now);
                self.hud("Copied File");
            }
            Run::Paste(s) => self.paste(&s, now),
            Run::PasteClip(i) | Run::CopyClip(i) => {
                let Some(c) = self.clips.get(i) else { return };
                // A copied file goes back on the clipboard as the file, not its path.
                let next = match (c.file, matches!(run, Run::PasteClip(_))) {
                    (true, true) => Run::PasteFile(PathBuf::from(&c.text)),
                    (true, false) => Run::CopyFile(PathBuf::from(&c.text)),
                    (false, true) => Run::Paste(c.text.clone()),
                    (false, false) => Run::Copy(c.text.clone()),
                };
                self.run(next, "", now);
            }
            Run::Cmd(c) => self.run_cmd(c, now),
            Run::Script(i, args) => self.run_script(i, args, now),
            Run::ScriptArgs(i) => {
                if let Some(s) = self.scripts.get(i) {
                    let view = View::Args {
                        title: s.title.clone(),
                        target: ArgsFor::Script(i),
                        values: vec![String::new(); s.arguments.len()],
                        placeholders: s.arguments.iter().map(|a| if a.optional { format!("{} (optional)", a.placeholder) } else { a.placeholder.clone() }).collect(),
                        secure: s.arguments.iter().map(|a| a.secure).collect(),
                        focus: 0,
                    };
                    self.push(view, String::new());
                }
            }
            Run::Quicklink(i, q) => {
                let Some(link) = self.quicklinks.get(i).cloned() else { return };
                match q {
                    Some(q) if !(link.takes_query() && q.trim().is_empty()) => {
                        self.hide(now);
                        macos::open(&link.url_for(q.trim()));
                    }
                    _ => {
                        let view = View::Args { title: link.name.clone(), target: ArgsFor::Quicklink(i), values: vec![String::new()], placeholders: vec!["Query".into()], secure: vec![false], focus: 0 };
                        self.push(view, String::new());
                    }
                }
            }
            Run::PutInSearch(s) => {
                self.page_mut().query = s;
                self.focus_query = true;
            }
            Run::QuitApp(pid, force) => {
                let name = self.running.iter().find(|r| r.pid == pid).map(|r| r.name.clone()).unwrap_or_default();
                if macos::quit_app(pid, force) {
                    self.toast(format!("{} {name}", if force { "Force quit" } else { "Quit" }), true, now);
                    self.running.retain(|r| r.pid != pid);
                    self.epoch += 1;
                }
            }
            Run::PinClip(i) => {
                if let Some(c) = self.clips.get_mut(i) {
                    c.pinned = !c.pinned;
                    let _ = prefs::save(data::CLIPBOARD, &self.clips);
                    self.epoch += 1;
                }
            }
            Run::DeleteClip(i) => {
                if i < self.clips.len() {
                    self.clips.remove(i);
                    let _ = prefs::save(data::CLIPBOARD, &self.clips);
                    self.epoch += 1;
                }
            }
            Run::DeleteColor(i) => {
                if i < self.colors.len() {
                    self.colors.remove(i);
                    let _ = prefs::save(data::COLORS, &self.colors);
                    self.epoch += 1;
                    self.toast("Color removed", true, now);
                }
            }
            Run::Ramp(c) => self.push(View::Ramp(c), String::new()),
            Run::DeleteQuicklink(i) => {
                if i < self.quicklinks.len() {
                    let gone = self.quicklinks.remove(i);
                    data::save_quicklinks(&self.quicklinks);
                    self.epoch += 1;
                    self.toast(format!("Deleted {}", gone.name), true, now);
                }
            }
            Run::DeleteSnippet(i) => {
                if i < self.snippets.len() {
                    let gone = self.snippets.remove(i);
                    data::save_snippets(&self.snippets);
                    self.epoch += 1;
                    self.toast(format!("Deleted {}", gone.name), true, now);
                }
            }
            Run::Snippet(i, paste) => {
                let Some(sn) = self.snippets.get(i) else { return };
                let clip = macos::read_clipboard().map(|(t, _)| t).unwrap_or_default();
                let (date, time) = local_date_time();
                let text = data::expand(&sn.text, &clip, &date, &time);
                if paste { self.paste(&text, now) } else { self.run(Run::Copy(text), "", now) }
            }
            Run::Favorite(id, on) => {
                self.favorites.retain(|f| *f != id);
                if on {
                    self.favorites.push(id);
                }
                let _ = prefs::save(data::FAVORITES, &self.favorites);
                self.epoch += 1;
                self.toast(if on { "Added to Favorites" } else { "Removed from Favorites" }, true, now);
            }
            Run::MoveFavorite(id, by) => {
                data::move_favorite(&mut self.favorites, &id, by);
                let _ = prefs::save(data::FAVORITES, &self.favorites);
                self.epoch += 1;
            }
            Run::Forget(id) => {
                self.frecency.forget(&id);
                self.epoch += 1;
                self.toast("Ranking reset", true, now);
            }
            Run::OpenSettings(section) => {
                self.hide(now);
                self.open_window("settings", Some(section));
            }
            Run::SubmitForm => self.submit_form(now),
            Run::SubmitArgs => self.submit_args(now),
            Run::Rerun(path, args) => {
                if let Some(i) = self.scripts.iter().position(|s| s.path == path) {
                    self.run_script(i, args, now);
                }
            }
            Run::PressMenu(e) => {
                self.hide(now);
                std::thread::spawn(move || {
                    // Let the app come back to the front first.
                    std::thread::sleep(Duration::from_millis(120));
                    if !ax::press(&e) {
                        send(Msg::Hud("That menu item is no longer there".into()));
                    }
                });
            }
            Run::RaiseWindow(pid, e) => {
                self.hide(now);
                std::thread::spawn(move || {
                    ax::raise(pid, &e);
                });
            }
            Run::Calculation(input, raw) => {
                data::push_calculation(&input, &raw);
                self.tried("calc");
                self.run(Run::Copy(raw), "", now);
            }
        }
    }

    /// Copies, hides and (with Accessibility) presses ⌘V in the app you came from.
    fn paste(&mut self, text: &str, now: f64) {
        macos::copy(text);
        self.hide(now);
        if self.settings.paste_on_select && macos::ax_trusted() {
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(160));
                macos::paste();
            });
        } else {
            self.hud("Copied to Clipboard");
        }
    }

    pub fn run_cmd(&mut self, c: Cmd, now: f64) {
        let carry = || String::new();
        let root_query = if self.stack.len() == 1 { self.stack[0].query.trim().to_string() } else { String::new() };
        if let Some(prompt) = commands::def(c).confirm
            && self.confirm != Some(c)
        {
            let _ = prompt;
            self.confirm = Some(c);
            return;
        }
        self.confirm = None;
        match c {
            Cmd::SearchFiles => {
                let q = if commands::def(Cmd::SearchFiles).title.to_lowercase().starts_with(&root_query.to_lowercase()) { carry() } else { root_query };
                self.push(View::Files, q);
            }
            Cmd::ClipboardHistory => self.push(View::Clipboard, String::new()),
            Cmd::SearchMenuItems => {
                self.token += 1;
                let token = self.token;
                let front = self.prev_app.clone();
                let app = front.as_ref().map(|(n, _)| n.clone()).unwrap_or_default();
                let error = front.is_none().then(|| "There's no app in front to search".to_string());
                let trusted = macos::ax_trusted();
                if let Some((_, pid)) = front.clone()
                    && trusted
                {
                    std::thread::spawn(move || send(Msg::MenusDone(token, pid, ax::menu_items(pid))));
                }
                let cached = self.menu_cache.as_ref().filter(|(pid, _)| front.as_ref().is_some_and(|(_, p)| p == pid)).map(|(_, v)| v.clone());
                // Without Accessibility there's nothing to wait for.
                let items = if trusted { cached } else { Some(Vec::new()) };
                self.push(View::Menus { app, items, error, token }, String::new());
            }
            Cmd::SwitchWindows => {
                self.token += 1;
                let token = self.token;
                let trusted = macos::ax_trusted();
                if trusted {
                    let apps = macos::running_apps();
                    std::thread::spawn(move || send(Msg::WindowsDone(token, ax::windows(&apps))));
                }
                self.push(View::Windows { items: (!trusted).then(Vec::new), token }, String::new());
            }
            Cmd::SearchEmoji => {
                let q = if "search emoji & symbols".starts_with(&root_query.to_lowercase()) || "emoji".starts_with(&root_query.to_lowercase()) { carry() } else { root_query };
                self.push(View::Emoji, q);
            }
            Cmd::SavedColors => self.push(View::Colors, String::new()),
            Cmd::CalculatorHistory => self.push(View::Calculations, String::new()),
            Cmd::SearchQuicklinks => self.push(View::Quicklinks, String::new()),
            Cmd::SearchSnippets => self.push(View::Snippets, String::new()),
            Cmd::Extensions => self.push(View::Extensions, String::new()),
            Cmd::QuitApplications => self.push(View::Processes, String::new()),
            Cmd::CreateQuicklink => self.push(View::Form(form(FormKind::Quicklink)), String::new()),
            Cmd::CreateSnippet => {
                let mut f = form(FormKind::Snippet);
                if let Some((t, false)) = macos::read_clipboard() {
                    f.fields[1].1 = t;
                }
                self.push(View::Form(f), String::new());
            }
            Cmd::CreateExtension => self.push(View::Form(form(FormKind::Extension)), String::new()),
            Cmd::PickColor => {
                self.hide(now);
                if let Some(mtm) = MainThreadMarker::new() {
                    macos::pick_color(mtm, |c| send(Msg::PickedColor(c)));
                }
            }
            Cmd::OpenExtensionsFolder => {
                self.hide(now);
                macos::open_path(&paths::extensions_dir());
            }
            Cmd::ReloadExtensions => {
                self.reload_scripts();
                let n = self.scripts.len();
                self.toast(format!("Loaded {n} extension{}", if n == 1 { "" } else { "s" }), true, now);
            }
            Cmd::ClearClipboardHistory => {
                self.clips.retain(|c| c.pinned);
                let _ = prefs::save(data::CLIPBOARD, &self.clips);
                self.epoch += 1;
                self.toast("Clipboard history cleared", true, now);
            }
            Cmd::LockScreen => {
                self.hide(now);
                macos::lock_screen();
            }
            Cmd::Sleep => {
                self.hide(now);
                macos::pmset("sleepnow");
            }
            Cmd::SleepDisplays => {
                self.hide(now);
                macos::pmset("displaysleepnow");
            }
            Cmd::Restart | Cmd::ShutDown | Cmd::LogOut => {
                self.hide(now);
                let ev = match c {
                    Cmd::Restart => "rrst",
                    Cmd::ShutDown => "shut",
                    _ => "rlgo",
                };
                std::thread::spawn(move || macos::power(ev));
            }
            Cmd::EmptyTrash => {
                self.hide(now);
                background(|| macos::osascript(r#"tell application "Finder" to empty trash"#).map(|_| "Trash emptied".into()));
            }
            Cmd::EjectAllDisks => {
                self.hide(now);
                background(|| macos::osascript(r#"tell application "Finder" to eject (every disk whose ejectable is true)"#).map(|_| "Ejected all disks".into()));
            }
            Cmd::ToggleAppearance => {
                self.hide(now);
                background(|| {
                    macos::osascript(r#"tell application "System Events" to tell appearance preferences to set dark mode to not dark mode
tell application "System Events" to tell appearance preferences to get dark mode"#)
                    .map(|dark| if dark == "true" { "Dark appearance".into() } else { "Light appearance".into() })
                });
            }
            Cmd::ToggleHiddenFiles => {
                self.hide(now);
                background(|| Ok(if macos::toggle_hidden_files() { "Showing hidden files" } else { "Hiding hidden files" }.into()));
            }
            Cmd::HideOthers => {
                self.hide(now);
                macos::hide_others(self.prev_app.as_ref().map(|(_, pid)| *pid));
            }
            Cmd::UnhideAll => {
                self.hide(now);
                macos::unhide_all();
            }
            Cmd::QuitAllApps => {
                self.hide(now);
                macos::quit_all();
            }
            Cmd::ToggleMute => {
                self.hide(now);
                background(|| {
                    macos::osascript("set m to not (output muted of (get volume settings))\nset volume output muted m\nreturn m")
                        .map(|m| if m == "true" { "Muted".into() } else { "Unmuted".into() })
                });
            }
            Cmd::VolumeUp | Cmd::VolumeDown => {
                self.hide(now);
                let step = if c == Cmd::VolumeUp { "+ 10" } else { "- 10" };
                background(move || {
                    macos::osascript(&format!("set v to (output volume of (get volume settings)) {step}\nset volume output volume v\nreturn output volume of (get volume settings)"))
                        .map(|v| format!("Volume {v}%"))
                });
            }
            Cmd::ScreenSaver => {
                self.hide(now);
                let _ = Command::new("/usr/bin/open").args(["-a", "ScreenSaverEngine"]).spawn();
            }
            Cmd::Settings => {
                self.hide(now);
                self.open_window("settings", None);
            }
            Cmd::ShowOnboarding => {
                self.hide(now);
                self.open_window("onboarding", None);
            }
            Cmd::CheckForUpdates => {
                self.hide(now);
                if let updates::Status::Available(_) = self.update {
                    self.install_update();
                } else {
                    updates::check_in_background();
                }
            }
            Cmd::About => {
                self.hide(now);
                self.open_window("about", None);
            }
            Cmd::QuitGlint => quit(),
        }
    }

    fn run_script(&mut self, i: usize, args: Vec<String>, now: f64) {
        let Some(s) = self.scripts.get(i).cloned() else { return };
        self.tried("extension");
        self.token += 1;
        let token = self.token;
        match s.mode {
            Mode::Silent | Mode::Compact => {
                self.hide(now);
                let compact = s.mode == Mode::Compact;
                std::thread::spawn(move || {
                    let out = scripts::run(&s, &args, Duration::from_secs(60));
                    let last = out.text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
                    if compact || !out.ok {
                        send(Msg::Hud(if out.ok { if last.is_empty() { format!("{} finished", s.title) } else { last } } else { format!("{} failed", s.title) }));
                    }
                });
            }
            Mode::FullOutput => {
                let view = View::Output { title: s.title.clone(), text: None, ok: true, rerun: (s.path.clone(), args.clone()), token };
                if matches!(self.page().view, View::Output { .. }) {
                    self.page_mut().view = view;
                } else {
                    self.push(view, String::new());
                }
                std::thread::spawn(move || send(Msg::ScriptDone(token, scripts::run(&s, &args, Duration::from_secs(120)))));
            }
            Mode::Inline => {
                let path = s.path.clone();
                std::thread::spawn(move || {
                    let out = scripts::run(&s, &args, Duration::from_secs(15));
                    send(Msg::InlineDone(path, out.text.lines().next().unwrap_or("").trim().to_string()));
                });
                self.toast(format!("Refreshing {}", self.scripts[i].title), true, now);
            }
            Mode::List => {
                let q = if self.stack.len() == 1 { String::new() } else { self.page().query.clone() };
                self.push(View::List { script: s.path.clone(), items: Vec::new(), error: None, token, asked: None, due: Some(now) }, q);
            }
        }
    }

    /// Runs a list extension again once typing pauses.
    pub fn tick_list(&mut self, now: f64) {
        let q = self.page().query.clone();
        let next = self.token + 1;
        let Some(page) = self.stack.last_mut() else { return };
        let View::List { script, token, asked, due, .. } = &mut page.view else { return };
        if asked.as_deref() != Some(q.as_str()) && due.is_none() {
            *due = Some(now + 0.2);
        }
        if due.is_some_and(|d| now >= d) {
            *due = None;
            *asked = Some(q.clone());
            *token = next;
            self.token = next;
            if let Some(s) = self.scripts.iter().find(|s| &s.path == script).cloned() {
                std::thread::spawn(move || {
                    let out = scripts::run(&s, &[q], Duration::from_secs(15));
                    let parsed = if out.ok { scripts::parse_list(&out.text) } else { Err(out.text) };
                    send(Msg::ListDone(next, parsed));
                });
            }
        } else if due.is_some()
            && let Some(ctx) = CTX.get()
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    fn refresh_inline(&mut self) {
        for s in self.scripts.iter().filter(|s| s.mode == Mode::Inline && s.arguments.is_empty()).cloned() {
            std::thread::spawn(move || {
                let out = scripts::run(&s, &[], Duration::from_secs(10));
                send(Msg::InlineDone(s.path.clone(), out.text.lines().next().unwrap_or("").trim().to_string()));
            });
        }
    }

    fn reload_scripts(&mut self) {
        self.scripts = scripts::scan();
        self.inline.retain(|p, _| self.scripts.iter().any(|s| &s.path == p));
        self.epoch += 1;
        self.register_hotkeys();
    }

    /// Saves a Create Quicklink / Snippet / Extension form.
    pub fn submit_form(&mut self, now: f64) {
        let View::Form(f) = &self.page().view else { return };
        let f = f.clone();
        let val = |i: usize| f.fields[i].1.trim().to_string();
        let result: Result<String, String> = match f.kind {
            FormKind::Quicklink => {
                let (name, url, icon) = (val(0), val(1), val(2));
                if name.is_empty() || url.is_empty() {
                    Err("A quicklink needs a name and a link.".into())
                } else if !url.contains("://") {
                    Err("Links start with a scheme, like https://".into())
                } else {
                    self.quicklinks.retain(|q| q.name != name);
                    self.quicklinks.push(Quicklink { name: name.clone(), url, icon });
                    data::save_quicklinks(&self.quicklinks);
                    Ok(format!("Saved {name}"))
                }
            }
            FormKind::Snippet => {
                let (name, text, keyword) = (val(0), f.fields[1].1.clone(), val(2));
                if name.is_empty() || text.is_empty() {
                    Err("A snippet needs a name and some text.".into())
                } else {
                    self.snippets.retain(|s| s.name != name);
                    self.snippets.push(Snippet { name: name.clone(), text, keyword });
                    data::save_snippets(&self.snippets);
                    Ok(format!("Saved {name}"))
                }
            }
            FormKind::Extension => {
                let title = val(0);
                if title.is_empty() {
                    Err("Give your extension a title.".into())
                } else {
                    let slug: String = title.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
                    let slug = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
                    let path = paths::extensions_dir().join(format!("{slug}.sh"));
                    if path.exists() {
                        Err(format!("{} already exists.", paths::tilde(&path)))
                    } else {
                        let mode = EXTENSION_MODES[f.choice].0;
                        let body = scripts::template(&title, mode);
                        match std::fs::write(&path, body) {
                            Ok(()) => {
                                use std::os::unix::fs::PermissionsExt;
                                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
                                self.reload_scripts();
                                macos::open_with(&["-t".as_ref(), path.as_os_str()]);
                                Ok(format!("Created {}", paths::tilde(&path)))
                            }
                            Err(e) => Err(e.to_string()),
                        }
                    }
                }
            }
        };
        match result {
            Ok(msg) => {
                self.pop();
                self.epoch += 1;
                self.toast(msg, true, now);
            }
            Err(e) => {
                if let View::Form(f) = &mut self.page_mut().view {
                    f.error = Some(e);
                }
            }
        }
    }

    pub fn submit_args(&mut self, now: f64) {
        let View::Args { target, values, .. } = &self.page().view else { return };
        let (target, values) = (target.clone(), values.clone());
        match target {
            ArgsFor::Script(i) => {
                let required: Vec<bool> = self.scripts.get(i).map(|s| s.arguments.iter().map(|a| !a.optional).collect()).unwrap_or_default();
                if values.iter().zip(&required).any(|(v, r)| *r && v.trim().is_empty()) {
                    self.toast("Fill in the required fields", false, now);
                    return;
                }
                self.pop();
                self.run_script(i, values, now);
            }
            ArgsFor::Quicklink(i) => {
                if values[0].trim().is_empty() {
                    return;
                }
                self.run(Run::Quicklink(i, Some(values[0].clone())), "", now);
            }
        }
    }

    pub fn install_update(&mut self) {
        if let updates::Status::Available(r) | updates::Status::Failed(r, _) = &self.update {
            let r = r.clone();
            self.update = updates::Status::Installing(r.clone());
            self.sync_tray();
            updates::install_in_background(r);
        }
    }

    // ---- hotkeys, tray, windows ----

    fn register_hotkeys(&mut self) {
        let Some(m) = &self.hotkeys else { return };
        for (hk, _) in self.registered.drain(..) {
            let _ = m.unregister(hk);
        }
        self.runtime.hotkey_error = None;
        let mut wanted: Vec<(String, String)> = vec![(self.settings.hotkey.clone(), "toggle".into())];
        wanted.extend(self.settings.hotkeys.iter().map(|(id, hk)| (hk.clone(), id.clone())));
        for (text, target) in wanted {
            let Ok(hk) = text.parse::<HotKey>() else {
                if target == "toggle" {
                    self.runtime.hotkey_error = Some(format!("{text} isn't a hotkey Glint understands."));
                }
                continue;
            };
            match m.register(hk) {
                Ok(()) => self.registered.push((hk, target)),
                Err(e) => {
                    eprintln!("{APP_NAME}: couldn't register {text}: {e}");
                    if target == "toggle" {
                        self.runtime.hotkey_error = Some(format!("{} is already used by another app.", crate::settings::hotkey_label(&text)));
                    }
                }
            }
        }
        self.write_runtime();
    }

    fn hotkey(&mut self, id: u32, now: f64) {
        let Some(target) = self.registered.iter().find(|(hk, _)| hk.id() == id).map(|(_, t)| t.clone()) else { return };
        if target == "toggle" {
            return self.toggle(now);
        }
        self.run_id(&target, now);
    }

    /// Runs a command, app, extension or quicklink by its id (command hotkeys, menu).
    fn run_id(&mut self, id: &str, now: f64) {
        if let Some(def) = commands::by_id(id) {
            let shows = matches!(
                def.cmd,
                Cmd::SearchFiles | Cmd::ClipboardHistory | Cmd::SearchMenuItems | Cmd::SwitchWindows | Cmd::SearchEmoji | Cmd::SavedColors | Cmd::CalculatorHistory | Cmd::SearchQuicklinks
                    | Cmd::SearchSnippets | Cmd::Extensions | Cmd::QuitApplications | Cmd::CreateQuicklink | Cmd::CreateSnippet | Cmd::CreateExtension
            ) || def.confirm.is_some();
            if shows {
                self.show(now);
                self.stack.truncate(1);
                self.stack[0].query.clear();
            }
            self.frecency.record(id);
            self.run_cmd(def.cmd, now);
        } else if let Some(path) = id.strip_prefix("app:") {
            self.frecency.record(id);
            open_path_fast(&PathBuf::from(path));
        } else if let Some(i) = self.scripts.iter().position(|s| s.id() == id) {
            let needs_window = matches!(self.scripts[i].mode, Mode::FullOutput | Mode::List) || !self.scripts[i].arguments.is_empty();
            if needs_window {
                self.show(now);
                self.stack.truncate(1);
            }
            let run = if !self.scripts[i].arguments.is_empty() && self.scripts[i].mode != Mode::List { Run::ScriptArgs(i) } else { Run::Script(i, vec![]) };
            self.run(run, id, now);
        } else if let Some(i) = self.quicklinks.iter().position(|q| q.id() == id) {
            if self.quicklinks[i].takes_query() {
                self.show(now);
                self.stack.truncate(1);
            }
            // A link without {query} opens directly; one with it asks first.
            let q = (!self.quicklinks[i].takes_query()).then(String::new);
            self.run(Run::Quicklink(i, q), id, now);
        }
    }

    fn sync_tray(&mut self) {
        let (available, status) = match &self.update {
            updates::Status::Idle => (None, None),
            updates::Status::Available(r) => (Some(r.version.clone()), None),
            updates::Status::Installing(r) => (Some(r.version.clone()), Some("installing".to_string())),
            updates::Status::Failed(r, e) => (Some(r.version.clone()), Some(format!("failed: {e}"))),
        };
        if (&available, &status) != (&self.runtime.update_available, &self.runtime.update_status) {
            self.runtime.update_available = available;
            self.runtime.update_status = status;
            self.write_runtime();
        }
        if !self.settings.show_menu_bar_icon {
            self.tray = None;
            return;
        }
        let menu = tray::menu(self);
        match &self.tray {
            Some(t) => t.set_menu(Some(Box::new(menu))),
            None => self.tray = tray::build(menu),
        }
    }

    fn on_menu(&mut self, id: &str, now: f64) {
        match id {
            "open" => self.show(now),
            "settings" => self.open_window("settings", None),
            "onboarding" => self.open_window("onboarding", None),
            "about" => self.open_window("about", None),
            "update" => self.install_update(),
            "check" => updates::check_in_background(),
            "feedback" => {
                let _ = macos::open(&format!("{}/issues/new", paths::REPO_URL));
            }
            "quit" => quit(),
            other => self.run_id(other, now),
        }
    }

    /// `Glint ui <kind> [section]`: one process per kind. An open window is
    /// brought forward, or replaced when a section is requested.
    pub fn open_window(&mut self, kind: &str, section: Option<&str>) {
        if let Some(child) = self.windows.get_mut(kind)
            && child.try_wait().is_ok_and(|s| s.is_none())
        {
            if section.is_none() {
                if let Some(app) = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(child.id() as i32) {
                    #[allow(deprecated)]
                    app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps);
                }
                return;
            }
            let _ = child.kill();
        }
        let Ok(exe) = std::env::current_exe() else { return };
        match Command::new(exe).arg("ui").arg(kind).args(section).spawn() {
            Ok(child) => {
                self.windows.insert(kind.to_string(), child);
            }
            Err(e) => eprintln!("{APP_NAME}: couldn't open {kind}: {e}"),
        }
    }

    fn settings_changed(&mut self) {
        let m = mtime(&paths::settings_file());
        if m == self.settings_mtime {
            return;
        }
        self.settings_mtime = m;
        let old = std::mem::replace(&mut self.settings, Settings::load());
        CLIPBOARD_ON.store(self.settings.clipboard_history, Ordering::Relaxed);
        if old.hotkey != self.settings.hotkey || old.hotkeys != self.settings.hotkeys {
            self.register_hotkeys();
        }
        if old.show_menu_bar_icon != self.settings.show_menu_bar_icon || old.hotkey != self.settings.hotkey {
            self.sync_tray();
        }
        if old.clipboard_limit != self.settings.clipboard_limit && self.clips.len() > self.settings.clipboard_limit {
            let mut kept = 0;
            let limit = self.settings.clipboard_limit;
            self.clips.retain(|c| {
                kept += usize::from(!c.pinned);
                c.pinned || kept <= limit
            });
        }
        self.file_for.clear();
        self.epoch += 1;
    }

    fn handle(&mut self, m: Msg, now: f64) {
        match m {
            Msg::Toggle => self.toggle(now),
            Msg::Show => self.show(now),
            // The launch itself activates the app; only later activations mean "open".
            Msg::Activated if now > 2.0 && !self.shown => self.show(now),
            Msg::Activated => {}
            Msg::Hotkey(id) => self.hotkey(id, now),
            Msg::Menu(id) => self.on_menu(&id, now),
            Msg::Clip(c) => {
                if self.settings.clipboard_history {
                    data::push_clip(&mut self.clips, c, self.settings.clipboard_limit);
                    let _ = prefs::save(data::CLIPBOARD, &self.clips);
                    self.epoch += 1;
                }
            }
            Msg::Hud(s) => self.hud(&s),
            Msg::PickedColor(Some(c)) => {
                data::push_color(&mut self.colors, c);
                let _ = prefs::save(data::COLORS, &self.colors);
                let text = c.format(self.settings.color_format);
                macos::copy(&text);
                self.hud(&format!("Copied {text}"));
                self.tried("color");
                self.epoch += 1;
            }
            Msg::PickedColor(None) => {}
            Msg::ScriptDone(token, out) => {
                for p in &mut self.stack {
                    if let View::Output { token: t, text, ok, .. } = &mut p.view
                        && *t == token
                    {
                        *text = Some(out.text.clone());
                        *ok = out.ok;
                    }
                }
            }
            Msg::ListDone(token, result) => {
                for p in &mut self.stack {
                    if let View::List { token: t, items, error, .. } = &mut p.view
                        && *t == token
                    {
                        match &result {
                            Ok(v) => {
                                *items = v.clone();
                                *error = None;
                            }
                            Err(e) => *error = Some(e.clone()),
                        }
                    }
                }
                self.epoch += 1;
            }
            Msg::MenusDone(token, pid, result) => {
                if let Ok(v) = &result {
                    self.menu_cache = Some((pid, v.clone()));
                }
                for p in &mut self.stack {
                    if let View::Menus { token: t, items, error, .. } = &mut p.view
                        && *t == token
                    {
                        match &result {
                            Ok(v) => *items = Some(v.clone()),
                            Err(e) => *error = Some(e.clone()),
                        }
                    }
                }
                self.epoch += 1;
            }
            Msg::WindowsDone(token, found) => {
                for p in &mut self.stack {
                    if let View::Windows { token: t, items } = &mut p.view
                        && *t == token
                    {
                        *items = Some(found.clone());
                    }
                }
                self.epoch += 1;
            }
            Msg::InlineDone(path, line) => {
                self.inline.insert(path, line);
                self.epoch += 1;
            }
            Msg::UpdateFound(r) => {
                if matches!(self.update, updates::Status::Idle | updates::Status::Available(_) | updates::Status::Failed(..)) {
                    self.update = updates::Status::Available(r);
                    self.sync_tray();
                }
            }
            Msg::UpdateDone(r, result) => match result {
                Ok(()) => {
                    // Close Settings/Onboarding first (Slat does the same), so the
                    // relaunch starts the new launcher instead of focusing a leftover window.
                    for child in self.windows.values_mut() {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                    updates::relaunch();
                }
                Err(e) => {
                    self.update = updates::Status::Failed(r, e);
                    self.sync_tray();
                }
            },
            Msg::Apps(found) => {
                if found != self.apps {
                    self.icons.warm(found.iter().map(|a| Img::File(a.path.clone())));
                    self.apps = found;
                    self.epoch += 1;
                }
            }
            Msg::SettingsChanged => self.settings_changed(),
            Msg::ExtensionsChanged => self.reload_scripts(),
            // Only files a Settings window changed; our own saves aren't news.
            Msg::DataChanged => {
                if prefs::changed_elsewhere(data::QUICKLINKS) {
                    self.quicklinks = data::load_quicklinks();
                }
                if prefs::changed_elsewhere(data::SNIPPETS) {
                    self.snippets = data::load_snippets();
                }
                if prefs::changed_elsewhere(data::COLORS) {
                    self.colors = data::load_colors();
                }
                if prefs::changed_elsewhere(data::CLIPBOARD) {
                    self.clips = data::load_clips();
                }
                if prefs::changed_elsewhere(Frecency::FILE) {
                    self.frecency = Frecency::load();
                }
                self.epoch += 1;
            }
        }
    }
}

impl eframe::App for Launcher {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        crate::ui::theme::GLASS
    }

    /// Runs every pass, hidden or not.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        if !self.started {
            self.start(ctx);
        }
        if !self.parked && ctx.cumulative_frame_nr() >= 1 {
            self.parked = true;
            if !self.shown
                && let Some(mtm) = MainThreadMarker::new()
            {
                window::park(mtm);
            }
        }
        while let Ok(m) = self.rx.try_recv() {
            self.handle(m, now);
        }
        self.icons.pump(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        // Hide when focus moves elsewhere (Settings, Quick Look, the sampler...).
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        // Lost focus, or never got it (activation refused): hide.
        let gone = if self.was_focused { now - self.shown_at > 0.2 } else { now - self.shown_at > 1.5 };
        if self.shown && !self.was_focused {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
        if focused {
            self.was_focused = true;
        } else if self.shown && gone && !snapshotting() {
            self.hide(now);
            return;
        }
        if !self.shown {
            return;
        }
        self.refresh_sections(now);
        if matches!(self.page().view, View::List { .. }) {
            self.tick_list(now);
        }
        self.draw(ui, now);
        #[cfg(debug_assertions)]
        crate::ui::snapshot(&ctx);
    }
}

fn form(kind: FormKind) -> Form {
    let fields = match kind {
        FormKind::Quicklink => vec![
            ("Name", String::new(), "Search Figma", false),
            ("Link", String::new(), "https://www.figma.com/files/search?query={query}", false),
            ("Icon", String::new(), "An SF Symbol name, like paintbrush (optional)", false),
        ],
        FormKind::Snippet => vec![
            ("Name", String::new(), "Email signature", false),
            ("Snippet", String::new(), "Text to paste. {clipboard}, {date} and {time} are filled in.", true),
            ("Keyword", String::new(), "sig (optional, for search)", false),
        ],
        FormKind::Extension => vec![("Title", String::new(), "Say Hello", false)],
    };
    Form { kind, fields, choice: 0, focus: 0, error: None }
}

/// NSWorkspace opens apps and documents without spawning `open`.
fn open_path_fast(p: &std::path::Path) {
    let url = objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&p.to_string_lossy()));
    if !objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&url) {
        macos::open_path(p);
    }
}

fn background(f: impl FnOnce() -> Result<String, String> + Send + 'static) {
    std::thread::spawn(move || match f() {
        Ok(msg) => send(Msg::Hud(msg)),
        Err(e) => {
            let msg = if e.contains("-1743") || e.contains("Not authorized") { "Allow Glint under Privacy › Automation".to_string() } else { "That didn't work".to_string() };
            send(Msg::Hud(msg));
        }
    });
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Newest modification time in a folder and its subfolders (one level).
fn tree_mtime(dir: &std::path::Path) -> Option<SystemTime> {
    let mut newest = mtime(dir);
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let m = mtime(&e.path());
        newest = newest.max(m);
        if e.path().is_dir() {
            for f in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                newest = newest.max(mtime(&f.path()));
            }
        }
    }
    newest
}

/// Watches settings, data files and the Extensions folder; watches the clipboard.
fn spawn_watchers() {
    std::thread::spawn(|| {
        let data_files = [data::QUICKLINKS, data::SNIPPETS, data::COLORS, data::CLIPBOARD, Frecency::FILE];
        let stamp = |names: &[&str]| names.iter().map(|n| mtime(&paths::data(n))).collect::<Vec<_>>();
        let mut settings = mtime(&paths::settings_file());
        let mut ext = tree_mtime(&paths::extensions_dir());
        let mut files = stamp(&data_files);
        loop {
            std::thread::sleep(Duration::from_millis(800));
            let s = mtime(&paths::settings_file());
            if s != settings {
                settings = s;
                send(Msg::SettingsChanged);
            }
            let e = tree_mtime(&paths::extensions_dir());
            if e != ext {
                ext = e;
                send(Msg::ExtensionsChanged);
            }
            let f = stamp(&data_files);
            if f != files {
                files = f;
                send(Msg::DataChanged);
            }
        }
    });
    std::thread::spawn(|| {
        let mut last = macos::change_count();
        loop {
            std::thread::sleep(Duration::from_millis(450));
            // A long-lived thread: drain what AppKit autoreleases on every pass.
            objc2::rc::autoreleasepool(|_| {
                let n = macos::change_count();
                if n == last {
                    return;
                }
                last = n;
                if !CLIPBOARD_ON.load(Ordering::Relaxed) || macos::clipboard_concealed() {
                    return;
                }
                if let Some((text, file)) = macos::read_clipboard()
                    && !text.trim().is_empty()
                {
                    let app = macos::frontmost().map(|(n, _)| n).unwrap_or_default();
                    send(Msg::Clip(Clip { text, file, at: data::now(), pinned: false, app }));
                }
            });
        }
    });
}

/// SIGUSR1 shows the launcher (Onboarding's "Open Glint", a second launch).
fn listen_for_signals() {
    if let Ok(mut signals) = signal_hook::iterator::Signals::new([SIG_SHOW, SIG_CHECK_UPDATES, SIG_INSTALL_UPDATE, libc::SIGTERM, libc::SIGINT]) {
        std::thread::spawn(move || {
            for sig in signals.forever() {
                match sig {
                    SIG_SHOW => send(Msg::Show),
                    SIG_CHECK_UPDATES => send(Msg::Menu("check".into())),
                    SIG_INSTALL_UPDATE => send(Msg::Menu("update".into())),
                    _ => quit(),
                }
            }
        });
    }
}

/// Opening Glint.app while it runs (Finder, Dock, Spotlight) shows the launcher.
fn observe_activation() {
    use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    let block = block2::RcBlock::new(|_: std::ptr::NonNull<NSNotification>| {
        let shown = MainThreadMarker::new().is_some_and(window::is_shown);
        // An alert activating Glint isn't someone opening it.
        if !shown && !window::modal() {
            send(Msg::Activated);
        }
    });
    // SAFETY: the block only sends a message; it runs on the main thread.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(Some(NSApplicationDidBecomeActiveNotification), None, None, &block)
    };
    std::mem::forget(observer);
}

/// Dev builds rendering a snapshot keep the window up without focus.
fn snapshotting() -> bool {
    cfg!(debug_assertions) && std::env::var_os("GLINT_UI_SNAPSHOT").is_some()
}

fn quit() -> ! {
    let _ = std::fs::remove_file(paths::data(RUNTIME));
    std::process::exit(0)
}

/// Local "YYYY-MM-DD" and "HH:MM" from libc (the system's own time zone).
fn local_date_time() -> (String, String) {
    // SAFETY: localtime_r fills the zeroed tm we own.
    let tm = unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    };
    (format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday), format!("{:02}:{:02}", tm.tm_hour, tm.tm_min))
}
