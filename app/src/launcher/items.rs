//! Rows in the launcher and what they do. Root search ranks every source
//! into sections; each command's view builds its own rows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::Key;

use crate::commands::{self, Cmd};
use crate::macos::{Img, Running};
use crate::paths;
use crate::search::apps::{self, App};
use crate::search::ax::{Elem, MenuItem, Window};
use crate::search::calc;
use crate::search::color::{self, Rgb};
use crate::search::data::{Clip, Frecency, Quicklink, Snippet};
use crate::search::fuzzy;
use crate::search::scripts::{ListItem, Mode, Script};
use crate::settings::{ColorFormat, Settings};

#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    Img(Img),
    Swatch(Rgb),
    Mark,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Row,
    /// The calculator's two-sided answer card.
    Calc { input: String, answer: calc::Answer },
    /// A color typed into the search bar.
    Color(Rgb),
}

#[derive(Clone, Debug)]
pub struct Item {
    /// Ranking and alias key ("app:/Applications/Safari.app"); empty = untracked.
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub accessory: String,
    pub icon: Icon,
    pub kind: Kind,
    /// [0] runs on ↵, [1] on ⌘↵; the rest by shortcut or the Actions panel.
    pub actions: Vec<Action>,
    /// Extra lines for the detail pane (label, value).
    pub detail: Vec<(String, String)>,
    /// Long preview text (clipboard).
    pub preview: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub cmd: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub key: Key,
}

impl Shortcut {
    pub const fn cmd(key: Key) -> Shortcut {
        Shortcut { cmd: true, shift: false, ctrl: false, key }
    }
    pub const fn cmd_shift(key: Key) -> Shortcut {
        Shortcut { cmd: true, shift: true, ctrl: false, key }
    }
    pub const fn ctrl(key: Key) -> Shortcut {
        Shortcut { cmd: false, shift: false, ctrl: true, key }
    }

    pub fn caps(&self) -> Vec<String> {
        let mut v = Vec::new();
        if self.ctrl {
            v.push("⌃".into());
        }
        if self.shift {
            v.push("⇧".into());
        }
        if self.cmd {
            v.push("⌘".into());
        }
        v.push(match self.key {
            Key::Enter => "↵".into(),
            Key::Backspace => "⌫".into(),
            Key::Comma => ",".into(),
            Key::Period => ".".into(),
            k => k.name().to_string(),
        });
        v
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    OpenPath(PathBuf),
    OpenUrl(String),
    Reveal(PathBuf),
    QuickLook(PathBuf),
    Trash(PathBuf),
    OpenInTerminal(PathBuf),
    EditFile(PathBuf),
    Copy(String),
    CopyFile(PathBuf),
    Paste(String),
    Cmd(Cmd),
    /// Script by index, with arguments.
    Script(usize, Vec<String>),
    /// Asks for the script's arguments first.
    ScriptArgs(usize),
    /// Quicklink by index; None asks for the query.
    Quicklink(usize, Option<String>),
    PutInSearch(String),
    QuitApp(i32, bool),
    PinClip(usize),
    DeleteClip(usize),
    DeleteColor(usize),
    Ramp(Rgb),
    DeleteQuicklink(usize),
    DeleteSnippet(usize),
    Snippet(usize, bool),
    Forget(String),
    OpenSettings(&'static str),
    Calculation(String, String),
    SubmitForm,
    SubmitArgs,
    /// Re-runs a full-output extension in place.
    Rerun(PathBuf, Vec<String>),
    /// Copies a file itself and pastes it into the front app.
    PasteFile(PathBuf),
    /// Clipboard history entry by index: paste it, or copy it back.
    PasteClip(usize),
    CopyClip(usize),
    /// Clicks an item in the front app's menu bar.
    PressMenu(Elem),
    /// Brings a window (of the app with this pid) to the front.
    RaiseWindow(i32, Elem),
}

#[derive(Clone, Debug)]
pub struct Action {
    pub title: String,
    pub symbol: &'static str,
    pub shortcut: Option<Shortcut>,
    pub run: Run,
}

fn act(title: impl Into<String>, symbol: &'static str, shortcut: Option<Shortcut>, run: Run) -> Action {
    Action { title: title.into(), symbol, shortcut, run }
}

pub struct Section {
    pub title: String,
    pub items: Vec<Item>,
}

impl Item {
    fn new(id: impl Into<String>, title: impl Into<String>, icon: Icon, actions: Vec<Action>) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: String::new(),
            accessory: String::new(),
            icon,
            kind: Kind::Row,
            actions,
            detail: Vec::new(),
            preview: None,
        }
    }
    fn sub(mut self, s: impl Into<String>) -> Item {
        self.subtitle = s.into();
        self
    }
    fn acc(mut self, s: impl Into<String>) -> Item {
        self.accessory = s.into();
        self
    }
}

/// Everything root search reads, borrowed from the launcher.
pub struct Sources<'a> {
    pub apps: &'a [App],
    pub scripts: &'a [Script],
    pub inline: &'a HashMap<PathBuf, String>,
    pub quicklinks: &'a [Quicklink],
    pub files: &'a [PathBuf],
    pub running: &'a [Running],
    pub settings: &'a Settings,
    pub frecency: &'a Frecency,
}

// ---- per-source items ----

fn ranking_actions(id: &str) -> [Action; 2] {
    [
        act("Add Alias or Hotkey", "keyboard", None, Run::OpenSettings("commands")),
        act("Reset Ranking", "arrow.uturn.backward", None, Run::Forget(id.to_string())),
    ]
}

pub fn app_item(a: &App, running: &[Running]) -> Item {
    let id = format!("app:{}", a.path.display());
    let mut actions = vec![
        act("Open Application", "arrow.up.forward.app", None, Run::OpenPath(a.path.clone())),
        act("Show in Finder", "folder", Some(Shortcut::cmd(Key::Enter)), Run::Reveal(a.path.clone())),
        act("Copy Path", "doc.on.doc", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(a.path.display().to_string())),
        act("Show Package Contents", "shippingbox", None, Run::OpenPath(a.path.join("Contents"))),
    ];
    let live = running.iter().find(|r| r.path.as_deref() == Some(a.path.as_path()));
    if let Some(r) = live {
        actions.push(act("Quit Application", "xmark.app", Some(Shortcut::cmd_shift(Key::Q)), Run::QuitApp(r.pid, false)));
        actions.push(act("Force Quit Application", "exclamationmark.octagon", None, Run::QuitApp(r.pid, true)));
    }
    actions.extend(ranking_actions(&id));
    let mut it = Item::new(&id, &a.name, Icon::Img(Img::File(a.path.clone())), actions).acc("Application");
    if live.is_some() {
        it.subtitle = "Running".into();
    }
    it
}

pub fn file_item(p: &Path) -> Item {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    let parent = p.parent().map(paths::tilde).unwrap_or_default();
    let is_dir = p.is_dir();
    let mut actions = vec![
        act(if is_dir { "Open Folder" } else { "Open" }, "arrow.up.forward.square", None, Run::OpenPath(p.to_path_buf())),
        act("Show in Finder", "folder", Some(Shortcut::cmd(Key::Enter)), Run::Reveal(p.to_path_buf())),
        act("Quick Look", "eye", Some(Shortcut::cmd(Key::Y)), Run::QuickLook(p.to_path_buf())),
        act("Copy File", "doc.on.doc", Some(Shortcut::cmd_shift(Key::F)), Run::CopyFile(p.to_path_buf())),
        act("Copy Path", "link", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(p.display().to_string())),
        act("Copy Name", "textformat", Some(Shortcut::cmd_shift(Key::N)), Run::Copy(name.clone())),
    ];
    if is_dir {
        actions.push(act("Open in Terminal", "terminal", Some(Shortcut::cmd_shift(Key::T)), Run::OpenInTerminal(p.to_path_buf())));
    }
    actions.push(act("Move to Trash", "trash", Some(Shortcut::ctrl(Key::X)), Run::Trash(p.to_path_buf())));
    let mut it = Item::new(format!("file:{}", p.display()), name, Icon::Img(Img::File(p.to_path_buf())), actions).sub(parent);
    it.accessory = if is_dir { "Folder".into() } else { p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_else(|| "File".into()) };
    it.detail = file_detail(p);
    it
}

fn file_detail(p: &Path) -> Vec<(String, String)> {
    let mut d = vec![("Where".to_string(), p.parent().map(paths::tilde).unwrap_or_default())];
    if let Ok(m) = std::fs::metadata(p) {
        if m.is_file() {
            d.push(("Size".into(), human_size(m.len())));
        }
        if let Ok(t) = m.modified() {
            d.push(("Modified".into(), ago(t)));
        }
    }
    d
}

pub fn human_size(b: u64) -> String {
    const U: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < U.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 { format!("{b} bytes") } else { format!("{v:.1} {}", U[i]) }
}

pub fn ago(t: std::time::SystemTime) -> String {
    let secs = std::time::SystemTime::now().duration_since(t).map_or(0, |d| d.as_secs());
    ago_secs(secs)
}

pub fn ago_secs(secs: u64) -> String {
    match secs {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=172_799 => "Yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

pub fn command_item(def: &commands::Def) -> Item {
    let id = def.id();
    let title = if matches!(def.cmd, Cmd::Restart | Cmd::ShutDown | Cmd::LogOut | Cmd::EmptyTrash | Cmd::QuitAllApps) { "Run Command" } else { "Open Command" };
    let mut actions = vec![act(title, "return", None, Run::Cmd(def.cmd))];
    actions.extend(ranking_actions(&id));
    Item::new(id, def.title, Icon::Img(Img::Symbol(def.symbol)), actions).sub(def.subtitle).acc("Command")
}

fn named_icon(icon: Option<&str>, fallback: &'static str) -> Icon {
    Icon::Img(crate::macos::icon_named(icon, fallback))
}

pub fn script_item(i: usize, s: &Script, inline: &HashMap<PathBuf, String>) -> Item {
    let id = s.id();
    // List scripts take the search text as their argument instead.
    let run = if !s.arguments.is_empty() && s.mode != Mode::List { Run::ScriptArgs(i) } else { Run::Script(i, vec![]) };
    let mut actions = vec![
        act("Run Extension", "play", None, run),
        act("Show in Finder", "folder", Some(Shortcut::cmd(Key::Enter)), Run::Reveal(s.path.clone())),
        act("Edit Script", "pencil", Some(Shortcut::cmd(Key::E)), Run::EditFile(s.path.clone())),
        act("Copy Path", "link", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(s.path.display().to_string())),
    ];
    actions.extend(ranking_actions(&id));
    let sub = if s.mode == Mode::Inline {
        inline.get(&s.path).cloned().unwrap_or_default()
    } else {
        s.package.clone().or_else(|| s.description.clone()).unwrap_or_default()
    };
    Item::new(id, &s.title, named_icon(s.icon.as_deref(), "terminal"), actions).sub(sub).acc("Extension")
}

pub fn quicklink_item(i: usize, q: &Quicklink, query: Option<&str>) -> Item {
    let id = q.id();
    let open = if q.takes_query() { Run::Quicklink(i, query.map(String::from)) } else { Run::Quicklink(i, Some(String::new())) };
    let mut actions = vec![
        act("Open Quicklink", "arrow.up.forward", None, open),
        act("Copy Link", "link", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(q.url.clone())),
        act("Delete Quicklink", "trash", Some(Shortcut::ctrl(Key::X)), Run::DeleteQuicklink(i)),
    ];
    actions.extend(ranking_actions(&id));
    let icon = named_icon(Some(&q.icon), "link");
    let host = match q.url.split_once("://") {
        Some(("http" | "https", rest)) => rest.split(['/', '?']).next().unwrap_or(rest).to_string(),
        Some((scheme, _)) => format!("{scheme}:"),
        None => q.url.clone(),
    };
    Item::new(id, &q.name, icon, actions).sub(host).acc("Quicklink")
}

fn pane_item(title: &str, url_id: &str) -> Item {
    let id = format!("pane:{url_id}");
    let mut actions = vec![act("Open Settings Pane", "gearshape", None, Run::OpenUrl(apps::settings_url(url_id)))];
    actions.extend(ranking_actions(&id));
    Item::new(id, title, Icon::Img(Img::File(PathBuf::from(apps::SETTINGS_APP))), actions).sub("System Settings").acc("Settings")
}

pub fn calc_item(input: &str, a: calc::Answer) -> Item {
    let actions = vec![
        act("Copy Answer", "doc.on.doc", None, Run::Calculation(input.to_string(), a.raw.clone())),
        act("Paste Answer", "doc.on.clipboard", Some(Shortcut::cmd(Key::Enter)), Run::Paste(a.raw.clone())),
        act("Put Answer in Search Bar", "magnifyingglass", Some(Shortcut::cmd_shift(Key::Enter)), Run::PutInSearch(a.raw.clone())),
        act("Copy Calculation", "equal", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(format!("{input} = {}", a.text))),
    ];
    let mut it = Item::new("", a.text.clone(), Icon::Img(Img::Symbol("plus.forwardslash.minus")), actions).acc("Calculator");
    it.kind = Kind::Calc { input: input.to_string(), answer: a };
    it
}

pub fn color_actions(c: Rgb, primary: ColorFormat) -> Vec<Action> {
    let mut formats: Vec<ColorFormat> = vec![primary];
    formats.extend(ColorFormat::ALL.into_iter().filter(|f| *f != primary));
    let mut actions: Vec<Action> = formats
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let sc = match i {
                1 => Some(Shortcut::cmd(Key::Enter)),
                n @ 2..=5 => Some(Shortcut::cmd([Key::Num1, Key::Num2, Key::Num3, Key::Num4][n - 2])),
                _ => None,
            };
            act(format!("Copy {}", f.label()), "doc.on.doc", sc, Run::Copy(c.format(*f)))
        })
        .collect();
    actions.push(act("Shades and Tints", "square.stack.3d.down.right", Some(Shortcut::cmd(Key::T)), Run::Ramp(c)));
    actions
}

pub fn color_item(c: Rgb, primary: ColorFormat) -> Item {
    let mut it = Item::new("", c.format(primary), Icon::Swatch(c), color_actions(c, primary)).acc("Color");
    it.kind = Kind::Color(c);
    it
}

// ---- root search ----

/// Commands shown before you type anything (after your most used).
const STARTERS: [Cmd; 6] = [Cmd::SearchFiles, Cmd::ClipboardHistory, Cmd::PickColor, Cmd::SavedColors, Cmd::SearchEmoji, Cmd::Extensions];

/// Something root search can show. Scoring only needs its id, title and
/// keywords; the full row (actions, icon) is built for the winners alone.
#[derive(Clone, Copy)]
enum Src {
    App(usize),
    Cmd(usize),
    Script(usize),
    Quicklink(usize),
    Pane(usize),
}

impl Src {
    fn all(s: &Sources) -> impl Iterator<Item = Src> {
        (0..s.apps.len())
            .map(Src::App)
            .chain((0..commands::ALL.len()).map(Src::Cmd))
            .chain((0..s.scripts.len()).map(Src::Script))
            .chain((0..s.quicklinks.len()).map(Src::Quicklink))
            .chain((0..apps::SETTINGS_PANES.len()).map(Src::Pane))
    }

    fn id(self, s: &Sources) -> String {
        match self {
            Src::App(i) => format!("app:{}", s.apps[i].path.display()),
            Src::Cmd(i) => commands::ALL[i].id(),
            Src::Script(i) => s.scripts[i].id(),
            Src::Quicklink(i) => s.quicklinks[i].id(),
            Src::Pane(i) => format!("pane:{}", apps::SETTINGS_PANES[i].1),
        }
    }

    fn title<'a>(self, s: &'a Sources) -> &'a str {
        match self {
            Src::App(i) => &s.apps[i].name,
            Src::Cmd(i) => commands::ALL[i].title,
            Src::Script(i) => &s.scripts[i].title,
            Src::Quicklink(i) => &s.quicklinks[i].name,
            Src::Pane(i) => apps::SETTINGS_PANES[i].0,
        }
    }

    fn keywords<'a>(self, s: &'a Sources) -> Vec<&'a str> {
        match self {
            Src::Cmd(i) => commands::ALL[i].keywords.to_vec(),
            Src::Script(i) => s.scripts[i].keywords.iter().map(String::as_str).collect(),
            Src::Pane(i) => apps::SETTINGS_PANES[i].2.split(' ').collect(),
            Src::App(_) | Src::Quicklink(_) => Vec::new(),
        }
    }

    fn item(self, s: &Sources) -> Item {
        match self {
            Src::App(i) => app_item(&s.apps[i], s.running),
            Src::Cmd(i) => command_item(&commands::ALL[i]),
            Src::Script(i) => script_item(i, &s.scripts[i], s.inline),
            Src::Quicklink(i) => quicklink_item(i, &s.quicklinks[i], None),
            Src::Pane(i) => pane_item(apps::SETTINGS_PANES[i].0, apps::SETTINGS_PANES[i].1),
        }
    }
}

pub fn root(query: &str, s: &Sources) -> Vec<Section> {
    let q = query.trim().to_lowercase();
    let candidates: Vec<(Src, String)> = Src::all(s).map(|c| (c, c.id(s))).filter(|(_, id)| !s.settings.disabled.contains(id)).collect();

    let mut sections = Vec::new();
    if q.is_empty() {
        let mut used: Vec<(i32, Src)> = candidates.iter().filter_map(|(c, id)| {
            let b = s.frecency.bonus(id);
            (b > 0).then_some((b, *c))
        }).collect();
        used.sort_by(|a, b| b.0.cmp(&a.0));
        let mut suggestions: Vec<Item> = used.into_iter().take(6).map(|(_, c)| c.item(s)).collect();
        for cmd in STARTERS {
            let id = commands::def(cmd).id();
            if suggestions.len() < 6 && !suggestions.iter().any(|i| i.id == id) && !s.settings.disabled.contains(&id) {
                suggestions.push(command_item(commands::def(cmd)));
            }
        }
        sections.push(Section { title: "Suggestions".into(), items: suggestions });
        let glint: Vec<Item> = [Cmd::Settings, Cmd::CreateExtension, Cmd::CreateQuicklink, Cmd::QuitApplications]
            .iter()
            .map(|c| command_item(commands::def(*c)))
            .filter(|it| !sections[0].items.iter().any(|x| x.id == it.id))
            .collect();
        sections.push(Section { title: "Commands".into(), items: glint });
        return sections;
    }

    let mut top: Vec<Item> = Vec::new();
    if let Some(a) = calc::evaluate(query) {
        top.push(calc_item(query.trim(), a));
    }
    if let Some(c) = color::parse(query) {
        top.push(color_item(c, s.settings.color_format));
    }

    let aliases: HashMap<&str, &str> = s.settings.aliases.iter().map(|(id, a)| (id.as_str(), a.as_str())).collect();
    let mut scored: Vec<(i32, Src, Option<String>)> = candidates
        .into_iter()
        .filter_map(|(c, id)| {
            let alias = aliases.get(id.as_str()).map(|a| a.to_lowercase());
            let alias_score = alias.as_deref().and_then(|a| if a == q { Some(5000) } else if a.starts_with(&q) { Some(1100) } else { None });
            let base = fuzzy::score(&q, c.title(s), &c.keywords(s)).max(alias_score)?;
            Some((base + s.frecency.bonus(&id), c, alias))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.title(s).cmp(b.1.title(s))));
    top.extend(scored.into_iter().take(14).map(|(_, c, alias)| {
        let mut it = c.item(s);
        if let Some(a) = alias {
            it.accessory = format!("{} · {a}", it.accessory);
        }
        it
    }));
    if !top.is_empty() {
        sections.push(Section { title: "Results".into(), items: top });
    }
    if s.settings.files_in_root && !s.files.is_empty() {
        let files: Vec<Item> = s.files.iter().take(6).map(|p| file_item(p)).collect();
        sections.push(Section { title: "Files".into(), items: files });
    }
    sections.push(Section { title: format!("Use “{}” with…", query.trim()), items: fallbacks(query.trim(), s) });
    sections
}

/// Fallbacks: what the text can always be used for.
fn fallbacks(query: &str, s: &Sources) -> Vec<Item> {
    let mut out = Vec::new();
    for (i, ql) in s.quicklinks.iter().enumerate().filter(|(_, q)| q.takes_query()).take(2) {
        let mut it = quicklink_item(i, ql, Some(query));
        it.subtitle = String::new();
        out.push(it);
    }
    let files = commands::def(Cmd::SearchFiles);
    out.push(Item::new("", "Search Files", Icon::Img(Img::Symbol(files.symbol)), vec![act("Search Files", "magnifyingglass", None, Run::Cmd(Cmd::SearchFiles))]).acc("Fallback"));
    let emoji = commands::def(Cmd::SearchEmoji);
    out.push(Item::new("", "Search Emoji & Symbols", Icon::Img(Img::Symbol(emoji.symbol)), vec![act("Search Emoji", "face.smiling", None, Run::Cmd(Cmd::SearchEmoji))]).acc("Fallback"));
    out
}

// ---- command views ----

pub fn files_view(files: &[PathBuf]) -> Vec<Section> {
    vec![Section { title: "Files".into(), items: files.iter().map(|p| file_item(p)).collect() }]
}

pub fn clipboard_view(clips: &[Clip], q: &str) -> Vec<Section> {
    let q = q.trim().to_lowercase();
    let now = crate::search::data::now();
    let row = |(i, c): (usize, &Clip)| {
        let first = c.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
        let title: String = first.chars().take(120).collect();
        // By index: a long clip isn't copied into every action, every frame.
        let mut actions = vec![
            act("Paste to Active App", "doc.on.clipboard", None, Run::PasteClip(i)),
            act("Copy to Clipboard", "doc.on.doc", Some(Shortcut::cmd(Key::Enter)), Run::CopyClip(i)),
            act(if c.pinned { "Unpin" } else { "Pin" }, "pin", Some(Shortcut::cmd_shift(Key::P)), Run::PinClip(i)),
            act("Delete Entry", "trash", Some(Shortcut::ctrl(Key::X)), Run::DeleteClip(i)),
            act("Clear All History", "trash.slash", None, Run::Cmd(Cmd::ClearClipboardHistory)),
        ];
        let icon = if c.file {
            actions.insert(2, act("Show in Finder", "folder", Some(Shortcut::cmd(Key::O)), Run::Reveal(PathBuf::from(&c.text))));
            Icon::Img(Img::File(PathBuf::from(&c.text)))
        } else if let Some(col) = (c.text.len() < 64).then(|| color::parse(&c.text)).flatten() {
            Icon::Swatch(col)
        } else if c.text.starts_with("http://") || c.text.starts_with("https://") {
            actions.insert(2, act("Open Link", "safari", Some(Shortcut::cmd(Key::O)), Run::OpenUrl(c.text.trim().to_string())));
            Icon::Img(Img::Symbol("link"))
        } else {
            Icon::Img(Img::Symbol("text.alignleft"))
        };
        let mut it = Item::new("", title, icon, actions);
        it.accessory = if c.pinned { "Pinned".into() } else { String::new() };
        // The pane shows the first 4,000 characters; don't carry the rest around.
        it.preview = Some(c.text.chars().take(4000).collect());
        let mut d = vec![];
        if !c.app.is_empty() {
            d.push(("Application".to_string(), c.app.clone()));
        }
        d.push(("Copied".into(), ago_secs(now.saturating_sub(c.at))));
        d.push(("Characters".into(), c.text.chars().count().to_string()));
        d.push(("Words".into(), c.text.split_whitespace().count().to_string()));
        it.detail = d;
        it
    };
    let matches = |c: &&Clip| q.is_empty() || c.text.to_lowercase().contains(&q);
    let pinned: Vec<Item> = clips.iter().enumerate().filter(|(_, c)| c.pinned).filter(|(_, c)| matches(c)).map(row).collect();
    let rest: Vec<Item> = clips.iter().enumerate().filter(|(_, c)| !c.pinned).filter(|(_, c)| matches(c)).map(row).collect();
    let mut s = Vec::new();
    if !pinned.is_empty() {
        s.push(Section { title: "Pinned".into(), items: pinned });
    }
    s.push(Section { title: "Recent".into(), items: rest });
    s
}

fn filter(q: &str, title: &str, extra: &[&str]) -> Option<i32> {
    fuzzy::score(&q.trim().to_lowercase(), title, extra)
}

fn ranked(mut v: Vec<(i32, Item)>) -> Vec<Item> {
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, i)| i).collect()
}

pub fn quicklinks_view(links: &[Quicklink], q: &str) -> Vec<Section> {
    let items = ranked(links.iter().enumerate().filter_map(|(i, l)| Some((filter(q, &l.name, &[&l.url])?, quicklink_item(i, l, None)))).collect());
    vec![Section { title: "Quicklinks".into(), items }]
}

pub fn snippets_view(snippets: &[Snippet], q: &str) -> Vec<Section> {
    let items = ranked(
        snippets
            .iter()
            .enumerate()
            .filter_map(|(i, sn)| {
                let score = filter(q, &sn.name, &[&sn.keyword, &sn.text])?;
                let actions = vec![
                    act("Paste Snippet", "doc.on.clipboard", None, Run::Snippet(i, true)),
                    act("Copy Snippet", "doc.on.doc", Some(Shortcut::cmd(Key::Enter)), Run::Snippet(i, false)),
                    act("Delete Snippet", "trash", Some(Shortcut::ctrl(Key::X)), Run::DeleteSnippet(i)),
                ];
                let mut it = Item::new("", &sn.name, Icon::Img(Img::Symbol("text.quote")), actions).acc(&sn.keyword);
                it.preview = Some(sn.text.clone());
                Some((score, it))
            })
            .collect(),
    );
    vec![Section { title: "Snippets".into(), items }]
}

pub fn extensions_view(scripts: &[Script], inline: &HashMap<PathBuf, String>, q: &str) -> Vec<Section> {
    let items = ranked(scripts.iter().enumerate().filter_map(|(i, s)| Some((filter(q, &s.title, &[])?, script_item(i, s, inline)))).collect());
    let mut out = vec![Section { title: format!("Extensions · {}", scripts.len()), items }];
    let tools = [Cmd::CreateExtension, Cmd::OpenExtensionsFolder, Cmd::ReloadExtensions];
    out.push(Section { title: "Manage".into(), items: tools.iter().map(|c| command_item(commands::def(*c))).filter(|it| filter(q, &it.title, &[]).is_some()).collect() });
    out
}

pub fn processes_view(running: &[Running], q: &str) -> Vec<Section> {
    let items = ranked(
        running
            .iter()
            .filter_map(|r| {
                let score = filter(q, &r.name, &[])?;
                let actions = vec![
                    act("Quit Application", "xmark.app", None, Run::QuitApp(r.pid, false)),
                    act("Force Quit Application", "exclamationmark.octagon", Some(Shortcut::cmd(Key::Enter)), Run::QuitApp(r.pid, true)),
                    act("Copy Bundle Identifier", "doc.on.doc", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(r.bundle_id.clone())),
                ];
                let icon = r.path.clone().map_or(Icon::Img(Img::Symbol("app")), |p| Icon::Img(Img::File(p)));
                let it = Item::new("", &r.name, icon, actions).sub(if r.hidden { "Hidden" } else { "" }).acc(format!("pid {}", r.pid));
                Some((score, it))
            })
            .collect(),
    );
    vec![Section { title: "Running Applications".into(), items }]
}

/// The row that asks for Accessibility when a view can't work without it.
fn needs_access(what: &str) -> Vec<Section> {
    let actions = vec![act("Open Accessibility Settings", "lock.open", None, Run::OpenUrl(crate::macos::ACCESSIBILITY_URL.into()))];
    let it = Item::new("", "Allow Accessibility Access", Icon::Img(Img::Symbol("hand.raised")), actions).sub(format!("Glint needs it to {what}"));
    vec![Section { title: "Accessibility".into(), items: vec![it] }]
}

pub fn menus_view(menus: Option<&[MenuItem]>, error: Option<&str>, trusted: bool, q: &str) -> Vec<Section> {
    if !trusted {
        return needs_access("read the menus of the app you're using");
    }
    if error.is_some() {
        return Vec::new();
    }
    let items = ranked(
        menus
            .unwrap_or_default()
            .iter()
            .filter_map(|m| {
                let parents = m.path.join(" › ");
                let score = filter(q, &m.title, &[&parents])?;
                let actions = vec![
                    act("Run Menu Item", "filemenu.and.selection", None, Run::PressMenu(m.elem.clone())),
                    act("Copy Name", "doc.on.doc", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(m.title.clone())),
                ];
                Some((score, Item::new("", &m.title, Icon::Img(Img::Symbol("filemenu.and.selection")), actions).sub(parents).acc(&m.shortcut)))
            })
            .collect(),
    );
    vec![Section { title: "Menu Items".into(), items }]
}

pub fn windows_view(windows: Option<&[Window]>, trusted: bool, q: &str) -> Vec<Section> {
    if !trusted {
        return needs_access("see and switch between your windows");
    }
    let items = ranked(
        windows
            .unwrap_or_default()
            .iter()
            .filter_map(|w| {
                let score = filter(q, &w.title, &[&w.app])?;
                let actions = vec![
                    act("Switch to Window", "macwindow", None, Run::RaiseWindow(w.pid, w.elem.clone())),
                    act("Quit Application", "xmark.app", Some(Shortcut::cmd_shift(Key::Q)), Run::QuitApp(w.pid, false)),
                    act("Copy Title", "doc.on.doc", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(w.title.clone())),
                ];
                let icon = w.app_path.clone().map_or(Icon::Img(Img::Symbol("macwindow")), |p| Icon::Img(Img::File(p)));
                let sub = if w.minimized { format!("{} · Minimized", w.app) } else { w.app.clone() };
                Some((score, Item::new("", &w.title, icon, actions).sub(sub)))
            })
            .collect(),
    );
    vec![Section { title: "Windows".into(), items }]
}

pub fn calculations_view(list: &[crate::search::data::Calculation], q: &str) -> Vec<Section> {
    let q = q.trim().to_lowercase();
    let items = list
        .iter()
        .filter(|c| q.is_empty() || c.input.to_lowercase().contains(&q) || c.result.contains(&q))
        .map(|c| {
            let actions = vec![
                act("Copy Answer", "doc.on.doc", None, Run::Copy(c.result.clone())),
                act("Paste Answer", "doc.on.clipboard", Some(Shortcut::cmd(Key::Enter)), Run::Paste(c.result.clone())),
                act("Put Calculation in Search Bar", "magnifyingglass", Some(Shortcut::cmd_shift(Key::Enter)), Run::PutInSearch(c.input.clone())),
            ];
            Item::new("", &c.result, Icon::Img(Img::Symbol("equal")), actions).sub(&c.input).acc(ago_secs(crate::search::data::now().saturating_sub(c.at)))
        })
        .collect();
    vec![Section { title: "History".into(), items }]
}

/// Eleven steps from light to dark, each copyable, plus the whole palette.
pub fn ramp_view(base: Rgb, primary: ColorFormat) -> Vec<Section> {
    let ramp = base.ramp();
    let css: String = ramp.iter().map(|(n, c)| format!("  --color-{n}: {};\n", c.format(primary))).collect();
    let json: String = format!("{{\n{}\n}}", ramp.iter().map(|(n, c)| format!("  \"{n}\": \"{}\"", c.hex())).collect::<Vec<_>>().join(",\n"));
    let items = ramp
        .iter()
        .map(|(n, c)| {
            let mut actions = color_actions(*c, primary);
            actions.retain(|a| !matches!(a.run, Run::Ramp(_)));
            actions.push(act("Copy Palette as CSS Variables", "curlybraces", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(format!(":root {{\n{css}}}"))));
            actions.push(act("Copy Palette as JSON", "doc.plaintext", Some(Shortcut::cmd_shift(Key::J)), Run::Copy(json.clone())));
            Item::new("", n.to_string(), Icon::Swatch(*c), actions).sub(c.format(primary)).acc(c.format(ColorFormat::Oklch))
        })
        .collect();
    vec![Section { title: format!("Shades of {}", base.hex()), items }]
}

pub fn list_view(items: &[ListItem], q: &str) -> Vec<Section> {
    let rows = items
        .iter()
        .filter(|li| filter(q, &li.title, &[&li.subtitle]).is_some() || q.trim().is_empty())
        .map(|li| {
            let mut actions = Vec::new();
            if let Some(p) = &li.paste {
                actions.push(act("Paste", "doc.on.clipboard", None, Run::Paste(p.clone())));
            }
            if let Some(u) = &li.open {
                actions.push(act("Open", "arrow.up.forward", None, Run::OpenUrl(u.clone())));
            }
            if let Some(c) = &li.copy {
                actions.push(act("Copy to Clipboard", "doc.on.doc", None, Run::Copy(c.clone())));
            }
            if actions.is_empty() {
                actions.push(act("Copy Title", "doc.on.doc", None, Run::Copy(li.title.clone())));
            }
            for (i, a) in actions.iter_mut().enumerate().skip(1) {
                a.shortcut = [Some(Shortcut::cmd(Key::Enter)), Some(Shortcut::cmd_shift(Key::Enter))].get(i - 1).copied().flatten();
            }
            let fallback = match (&li.open, &li.paste) {
                (Some(u), _) if u.starts_with("file://") => "doc",
                (Some(_), _) => "link",
                (_, Some(_)) => "doc.on.clipboard",
                _ => "text.alignleft",
            };
            let icon = named_icon(Some(&li.icon), fallback);
            Item::new("", &li.title, icon, actions).sub(&li.subtitle).acc(&li.accessory)
        })
        .collect();
    vec![Section { title: "Results".into(), items: rows }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources<'a>(apps: &'a [App], s: &'a Settings, f: &'a Frecency, inline: &'a HashMap<PathBuf, String>) -> Sources<'a> {
        Sources { apps, scripts: &[], inline, quicklinks: &[], files: &[], running: &[], settings: s, frecency: f }
    }

    #[test]
    fn root_ranks_apps_calc_colors_and_aliases() {
        let apps = vec![
            App { name: "Safari".into(), path: "/Applications/Safari.app".into() },
            App { name: "Slack".into(), path: "/Applications/Slack.app".into() },
        ];
        let mut s = Settings::default();
        let f = Frecency::default();
        let inline = HashMap::new();
        let r = root("saf", &sources(&apps, &s, &f, &inline));
        assert_eq!(r[0].items[0].title, "Safari");
        let r = root("2+2", &sources(&apps, &s, &f, &inline));
        assert!(matches!(r[0].items[0].kind, Kind::Calc { .. }));
        let r = root("#A6C8F4", &sources(&apps, &s, &f, &inline));
        assert!(matches!(r[0].items[0].kind, Kind::Color(_)));
        s.aliases.insert("glint:pick-color".into(), "cp".into());
        let r = root("cp", &sources(&apps, &s, &f, &inline));
        assert_eq!(r[0].items[0].title, "Pick Color", "an exact alias wins");
        s.disabled.insert("app:/Applications/Slack.app".into());
        let r = root("slack", &sources(&apps, &s, &f, &inline));
        assert!(r[0].items.iter().all(|i| i.title != "Slack"), "disabled items are hidden");
        assert!(r.last().unwrap().title.starts_with("Use “slack”"), "fallbacks come last");
        let empty = root("", &sources(&apps, &s, &f, &inline));
        assert_eq!(empty[0].title, "Suggestions");
        assert!(!empty[0].items.is_empty());
    }
}
