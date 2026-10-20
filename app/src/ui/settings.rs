//! Settings window: sidebar plus one page per section. Every change is saved
//! to settings.json as soon as it happens; the launcher picks it up live.

use eframe::egui::{self, Align, Layout, Margin, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};

use super::about::About;
use super::hotkey::recorder;
use super::theme::{self, Kind, Mark, card, hairline, row, section};
use super::{art, chrome};
use crate::commands;
use crate::icons::Icons;
use crate::login::{self, LOGIN_ITEMS_URL, LoginStatus};
use crate::macos::{self, Img};
use crate::paths::{self, APP_NAME};
use crate::prefs::{self, Prefs};
use crate::search::apps;
use crate::search::data::{self, Clip, Frecency, SavedColor};
use crate::search::scripts::{self, Mode};
use crate::settings::{ColorFormat, DEFAULT_HOTKEY, FileScope, Settings, hotkey_label};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Search,
    Commands,
    Clipboard,
    Colors,
    Extensions,
    About,
}

const PAGES: [(Page, &str, &str, &str); 7] = [
    (Page::General, "general", "General", "Your hotkey, startup and the menu bar."),
    (Page::Search, "search", "Search", "What root search looks through."),
    (Page::Commands, "commands", "Commands", "Aliases, hotkeys and which commands show up."),
    (Page::Clipboard, "clipboard", "Clipboard", "History of what you copy."),
    (Page::Colors, "colors", "Colors", "Pick Color and saved colors."),
    (Page::Extensions, "extensions", "Extensions", "Script commands that add your own features."),
    (Page::About, "about", "About", "Version and updates."),
];

/// One row of the Commands page.
struct Entry {
    id: String,
    title: String,
    kind: &'static str,
    icon: Img,
}

pub struct SettingsApp {
    s: Settings,
    saved: Settings,
    page: Page,
    login: LoginStatus,
    login_err: Option<String>,
    next_poll: f64,
    hotkey_error: Option<String>,
    ax: bool,
    spotlight_on: bool,
    entries: Vec<Entry>,
    filter: String,
    icons: Icons,
    about: About,
    note: Option<String>,
}

fn entries() -> Vec<Entry> {
    let mut v: Vec<Entry> = commands::ALL.iter().map(|d| Entry { id: d.id(), title: d.title.into(), kind: "Command", icon: Img::Symbol(d.symbol) }).collect();
    for s in scripts::scan() {
        let icon = match &s.icon {
            Some(i) if !i.is_ascii() => Img::Emoji(i.clone()),
            _ => Img::Symbol("terminal"),
        };
        v.push(Entry { id: s.id(), title: s.title.clone(), kind: "Extension", icon });
    }
    for q in data::load_quicklinks() {
        let icon = if q.icon.is_empty() { Img::Symbol("link") } else { Img::Emoji(q.icon.clone()) };
        v.push(Entry { id: q.id(), title: q.name.clone(), kind: "Quicklink", icon });
    }
    for a in apps::scan() {
        v.push(Entry { id: format!("app:{}", a.path.display()), title: a.name.clone(), kind: "Application", icon: Img::File(a.path.clone()) });
    }
    v
}

impl SettingsApp {
    pub fn new(ctx: &egui::Context, section: Option<&str>) -> SettingsApp {
        let s = Settings::load();
        let page = section.and_then(|id| PAGES.iter().find(|p| p.1 == id)).map_or(Page::General, |p| p.0);
        SettingsApp {
            saved: s.clone(),
            s,
            page,
            login: login::status(),
            login_err: None,
            next_poll: 0.0,
            hotkey_error: None,
            ax: macos::ax_trusted(),
            spotlight_on: macos::spotlight_shortcut_on(),
            entries: entries(),
            filter: String::new(),
            icons: Icons::new(ctx),
            about: About::default(),
            note: None,
        }
    }
}

impl eframe::App for SettingsApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::GLASS
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        chrome(ui);
        let ctx = ui.ctx().clone();
        self.icons.pump(&ctx);
        let now = ctx.input(|i| i.time);
        if now >= self.next_poll {
            self.login = login::status();
            self.hotkey_error = super::runtime().hotkey_error;
            self.ax = macos::ax_trusted();
            self.next_poll = now + 1.5;
        }
        ctx.request_repaint_after_secs(1.5);

        let p = theme::pal();
        egui::Panel::left("sidebar")
            .exact_size(196.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(theme::glass(Margin::symmetric(10, 0)))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default().frame(theme::glass(Margin::ZERO)).show(ui, |ui| {
            let sep = ui.max_rect();
            ui.painter().vline(sep.left(), sep.y_range(), Stroke::new(1.0, p.line));
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                egui::Frame::new().inner_margin(Margin { left: 32, right: 32, top: 40, bottom: 32 }).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(660.0));
                    let (_, _, title, sub) = PAGES.iter().find(|x| x.0 == self.page).copied().unwrap_or(PAGES[0]);
                    theme::page_header(ui, title, sub);
                    match self.page {
                        Page::General => self.general(ui),
                        Page::Search => self.search(ui),
                        Page::Commands => self.commands(ui),
                        Page::Clipboard => self.clipboard(ui),
                        Page::Colors => self.colors(ui),
                        Page::Extensions => self.extensions(ui),
                        Page::About => {
                            ui.add_space(12.0);
                            card(ui, |ui| {
                                ui.add_space(28.0);
                                self.about.ui(ui);
                                ui.add_space(24.0);
                            });
                        }
                    }
                });
            });
        });

        if self.s != self.saved && ctx.dragged_id().is_none() {
            if let Err(e) = self.s.save() {
                eprintln!("{APP_NAME}: couldn't save settings: {e}");
            }
            self.saved = self.s.clone();
        }
    }
}

impl SettingsApp {
    fn sidebar(&mut self, ui: &mut Ui) {
        let p = theme::pal();
        ui.add_space(50.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 15.0), Sense::hover());
            art::mark(ui.painter(), r, &p);
            ui.label(theme::text(APP_NAME, 15.0, theme::BOLD).extra_letter_spacing(-0.3));
        });
        ui.add_space(14.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for (page, _, title, _) in PAGES {
            let selected = self.page == page;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let fill = if selected { p.knob } else if resp.hovered() { p.chip } else { egui::Color32::TRANSPARENT };
            ui.painter().rect_filled(rect, 6, fill);
            let g = theme::galley(ui, theme::text(title.to_lowercase(), 13.0, theme::REGULAR), f32::INFINITY);
            let color = if selected { p.bright } else if resp.hovered() { p.ink } else { p.muted };
            ui.painter().galley(pos2(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, color);
            if page == Page::General && self.hotkey_error.is_some() {
                ui.painter().circle_filled(pos2(rect.right() - 14.0, rect.center().y), 3.5, p.warning);
            }
            if resp.clicked() {
                self.page = page;
                self.note = None;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(theme::mono(format!("v{}", env!("CARGO_PKG_VERSION")), 11.0).weak());
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                if theme::small_button(ui, &format!("Open {APP_NAME}  {}", hotkey_label(&self.s.hotkey)), Kind::Secondary).clicked() {
                    super::open_launcher();
                }
            });
        });
    }

    fn general(&mut self, ui: &mut Ui) {
        section(ui, "Hotkey");
        let mut take_cmd_space = false;
        let mut undo_cmd_space = false;
        let on_cmd_space = self.s.hotkey == "Cmd+Space";
        card(ui, |ui| {
            let caption = self.hotkey_error.clone().unwrap_or_else(|| format!("Opens {APP_NAME} from anywhere"));
            let mut hk = Some(self.s.hotkey.clone());
            row(ui, &format!("{APP_NAME} hotkey"), Some(&caption), None, |ui| {
                if recorder(ui, "main", &mut hk, false).changed()
                    && let Some(h) = hk.clone()
                {
                    self.s.hotkey = h;
                }
            });
            hairline(ui);
            let detail = if on_cmd_space {
                if self.spotlight_on { "Spotlight's ⌘ Space is still on. Undo, or turn it off in Keyboard Shortcuts." } else { "Spotlight's shortcut is off, so ⌘ Space opens Glint." }
            } else {
                "Turns off Spotlight's ⌘ Space shortcut and gives it to Glint. You can undo this."
            };
            row(ui, "Replace Spotlight", Some(detail), None, |ui| {
                if on_cmd_space {
                    undo_cmd_space = theme::small_button(ui, "Undo", Kind::Secondary).clicked();
                } else {
                    take_cmd_space = theme::small_button(ui, "Use ⌘ Space", Kind::Primary).clicked();
                }
            });
            if on_cmd_space && self.hotkey_error.is_some() {
                theme::callout(ui, "⌘ Space is taken", "Another app already uses ⌘ Space. Change its hotkey, then Glint takes over.", None);
                ui.add_space(14.0);
            }
        });
        if take_cmd_space {
            take_spotlight(&mut self.s);
            self.spotlight_on = macos::spotlight_shortcut_on();
        }
        if undo_cmd_space {
            give_back_spotlight(&mut self.s);
            self.spotlight_on = macos::spotlight_shortcut_on();
        }

        section(ui, "Startup");
        card(ui, |ui| {
            let caption = self.login_err.clone().unwrap_or_else(|| {
                if !self.s.start_at_login {
                    return "Off".into();
                }
                match self.login {
                    LoginStatus::Enabled => format!("{APP_NAME} opens when you log in"),
                    LoginStatus::RequiresApproval => "Waiting for approval in System Settings".into(),
                    LoginStatus::NotFound => format!("Available once {APP_NAME} is in Applications"),
                    LoginStatus::NotRegistered => "Off".into(),
                }
            });
            let mut on = self.s.start_at_login;
            row(ui, "Open at login", Some(&caption), None, |ui| {
                if theme::toggle(ui, &mut on).changed() {
                    self.s.start_at_login = on;
                    self.login_err = login::apply(on).err().map(|e| format!("Couldn't update: {e}"));
                    self.login = login::status();
                }
            });
            if self.s.start_at_login && self.login == LoginStatus::RequiresApproval {
                if theme::callout(ui, "Approval needed", &format!("Allow {APP_NAME} in System Settings › General › Login Items."), Some("Open Login Items")) {
                    macos::open(LOGIN_ITEMS_URL);
                }
                ui.add_space(14.0);
            }
            hairline(ui);
            row(ui, "Show in menu bar", Some("The mark in the menu bar, with quick commands"), None, |ui| {
                theme::toggle(ui, &mut self.s.show_menu_bar_icon);
            });
        });

        section(ui, "Permissions");
        card(ui, |ui| {
            let (mark, label) = if self.ax { (Mark::On, "Granted") } else { (Mark::Off, "Not granted") };
            row(ui, "Accessibility", Some("Lets Glint paste clipboard items, snippets and emoji into the app you're using"), None, |ui| {
                if !self.ax && theme::small_button(ui, "Grant Access", Kind::Primary).clicked() {
                    macos::ax_prompt();
                    macos::open(macos::ACCESSIBILITY_URL);
                }
                theme::pill(ui, mark, label);
            });
        });
    }

    fn search(&mut self, ui: &mut Ui) {
        section(ui, "Root search");
        card(ui, |ui| {
            row(ui, "Include files", Some("Matching files and folders show under apps and commands"), None, |ui| {
                theme::toggle(ui, &mut self.s.files_in_root);
            });
            hairline(ui);
            row(ui, "Search in", Some("Glint uses the Spotlight index, so there's nothing to build"), None, |ui| {
                theme::segmented(ui, &mut self.s.file_scope, &[(FileScope::Home, "Home folder"), (FileScope::Everywhere, "Everywhere")]);
            });
        });
        section(ui, "Ranking");
        card(ui, |ui| {
            row(ui, "Learned ranking", Some("Things you pick often and recently float to the top"), None, |ui| {
                if theme::small_button(ui, "Reset", Kind::Secondary).clicked() {
                    let _ = prefs::save(Frecency::FILE, &Frecency::default());
                    self.note = Some("Ranking reset. It takes effect the next time Glint starts.".into());
                }
            });
        });
        if let Some(n) = &self.note {
            ui.add_space(8.0);
            ui.label(theme::caption(n));
        }
    }

    fn commands(&mut self, ui: &mut Ui) {
        let p = theme::pal();
        ui.add_space(14.0);
        egui::Frame::new().fill(p.raised).stroke(Stroke::new(1.0, p.line)).corner_radius(8).inner_margin(Margin::symmetric(10, 7)).show(ui, |ui| {
            ui.add(TextEdit::singleline(&mut self.filter).frame(egui::Frame::NONE).desired_width(f32::INFINITY).hint_text(theme::text("Filter commands, extensions and apps…", 13.0, theme::REGULAR).color(p.dim)));
        });
        ui.add_space(4.0);
        ui.label(theme::caption("Type an alias in the search bar to jump straight to a command. Hotkeys work from anywhere."));
        let q = self.filter.trim().to_lowercase();
        for kind in ["Command", "Extension", "Quicklink", "Application"] {
            let list: Vec<usize> = (0..self.entries.len()).filter(|&i| self.entries[i].kind == kind && (q.is_empty() || self.entries[i].title.to_lowercase().contains(&q))).collect();
            if list.is_empty() {
                continue;
            }
            section(ui, &format!("{kind}s"));
            card(ui, |ui| {
                for (n, &i) in list.iter().enumerate() {
                    if n > 0 {
                        hairline(ui);
                    }
                    let (id, title, icon) = (self.entries[i].id.clone(), self.entries[i].title.clone(), self.entries[i].icon.clone());
                    ui.horizontal(|ui| {
                        ui.set_height(44.0);
                        let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                        if matches!(icon, Img::Symbol(_)) {
                            ui.painter().rect_filled(r, 6, p.knob);
                            self.icons.paint(ui.painter(), &icon, r.shrink(4.5), p.ink);
                        } else {
                            self.icons.paint(ui.painter(), &icon, r, p.ink);
                        }
                        ui.add_space(4.0);
                        let enabled = !self.s.disabled.contains(&id);
                        ui.add(egui::Label::new(theme::text(&title, 13.0, theme::REGULAR).color(if enabled { p.ink } else { p.dim })).truncate());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let mut on = enabled;
                            if theme::toggle(ui, &mut on).changed() {
                                if on {
                                    self.s.disabled.remove(&id);
                                } else {
                                    self.s.disabled.insert(id.clone());
                                }
                            }
                            let mut hk = self.s.hotkeys.get(&id).cloned();
                            if recorder(ui, ("cmd", &id), &mut hk, true).changed() {
                                match hk {
                                    Some(h) => self.s.hotkeys.insert(id.clone(), h),
                                    None => self.s.hotkeys.remove(&id),
                                };
                            }
                            let mut alias = self.s.aliases.get(&id).cloned().unwrap_or_default();
                            let resp = ui.add_sized(
                                vec2(84.0, 26.0),
                                TextEdit::singleline(&mut alias).font(egui::FontId::monospace(12.0)).margin(Margin::symmetric(8, 4)).hint_text(theme::text("alias", 12.0, theme::REGULAR).color(p.faint)),
                            );
                            if resp.changed() {
                                let a = alias.trim().to_lowercase();
                                if a.is_empty() {
                                    self.s.aliases.remove(&id);
                                } else {
                                    self.s.aliases.insert(id.clone(), a);
                                }
                            }
                        });
                    });
                }
            });
        }
    }

    fn clipboard(&mut self, ui: &mut Ui) {
        section(ui, "History");
        card(ui, |ui| {
            row(ui, "Keep clipboard history", Some("Text and files you copy, searchable from Clipboard History"), None, |ui| {
                theme::toggle(ui, &mut self.s.clipboard_history);
            });
            hairline(ui);
            row(ui, "Keep", Some("Pinned items are always kept"), None, |ui| {
                theme::segmented(ui, &mut self.s.clipboard_limit, &[(100, "100"), (300, "300"), (1000, "1,000")]);
            });
            hairline(ui);
            row(ui, "Paste when chosen", Some(if self.ax { "Return pastes into the app you came from; ⌘ Return copies" } else { "Needs Accessibility; until then Return copies" }), None, |ui| {
                theme::toggle(ui, &mut self.s.paste_on_select);
            });
        });
        ui.add_space(8.0);
        ui.label(theme::caption("Password managers mark what they copy as private; Glint never saves those."));
        section(ui, "Clear");
        card(ui, |ui| {
            let n = data::load_clips().len();
            row(ui, &format!("{n} items saved"), Some("Removes everything except pinned items"), None, |ui| {
                if theme::small_button(ui, "Clear History", Kind::Secondary).clicked() {
                    let kept: Vec<Clip> = data::load_clips().into_iter().filter(|c| c.pinned).collect();
                    let _ = prefs::save(data::CLIPBOARD, &kept);
                }
            });
        });
    }

    fn colors(&mut self, ui: &mut Ui) {
        let p = theme::pal();
        section(ui, "Pick Color");
        card(ui, |ui| {
            row(ui, "Copy as", Some("What Pick Color puts on the clipboard"), None, |ui| {
                let opts: Vec<(ColorFormat, &str)> = ColorFormat::ALL.iter().map(|f| (*f, f.label())).collect();
                theme::segmented(ui, &mut self.s.color_format, &opts);
            });
        });
        let saved: Vec<SavedColor> = data::load_colors();
        section(ui, &format!("Saved colors · {}", saved.len()));
        card(ui, |ui| {
            ui.add_space(12.0);
            if saved.is_empty() {
                ui.label(theme::caption("Colors you pick show up here and in Saved Colors."));
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    for c in saved.iter().take(36) {
                        let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                        let [rr, gg, bb, aa] = c.color.u8();
                        ui.painter().rect_filled(r, 7, egui::Color32::from_rgba_unmultiplied(rr, gg, bb, aa));
                        ui.painter().rect_stroke(r, 7, Stroke::new(1.0, p.line), StrokeKind::Inside);
                        resp.on_hover_text(c.color.format(self.s.color_format));
                    }
                });
            }
            ui.add_space(12.0);
            hairline(ui);
            row(ui, "Clear saved colors", None, None, |ui| {
                if theme::small_button(ui, "Clear", Kind::Secondary).clicked() {
                    let _ = prefs::save(data::COLORS, &Vec::<SavedColor>::new());
                }
            });
        });
        ui.add_space(8.0);
        ui.label(theme::caption("Type #hex, rgb(), hsl() or oklch() in the search bar to convert a color. ⌘T on any color shows its shades."));
    }

    fn extensions(&mut self, ui: &mut Ui) {
        let p = theme::pal();
        let dir = paths::extensions_dir();
        section(ui, "Folder");
        card(ui, |ui| {
            row(ui, &paths::tilde(&dir), Some("Every script with a title in it becomes a command"), None, |ui| {
                if theme::small_button(ui, "Reveal", Kind::Secondary).clicked() {
                    paths::ensure_dirs();
                    macos::open_path(&dir);
                }
                if theme::small_button(ui, "Add Samples", Kind::Secondary).clicked() {
                    self.note = Some(match scripts::install_samples() {
                        Ok(0) => "The samples are already there.".into(),
                        Ok(n) => format!("Added {n} sample extensions to Samples."),
                        Err(e) => format!("Couldn't add samples: {e}"),
                    });
                    self.entries = entries();
                }
            });
        });
        if let Some(n) = &self.note {
            ui.add_space(6.0);
            ui.label(theme::caption(n));
        }
        let list = scripts::scan();
        section(ui, &format!("Installed · {}", list.len()));
        card(ui, |ui| {
            if list.is_empty() {
                ui.add_space(12.0);
                ui.label(theme::caption("No extensions yet. Add the samples, or use Create Extension in Glint."));
                ui.add_space(12.0);
            }
            for (i, s) in list.iter().enumerate() {
                if i > 0 {
                    hairline(ui);
                }
                let mode = match s.mode {
                    Mode::Silent => "silent",
                    Mode::Compact => "compact",
                    Mode::FullOutput => "full output",
                    Mode::Inline => "inline",
                    Mode::List => "list",
                };
                let caption = paths::tilde(&s.path);
                row(ui, &s.title, Some(&caption), Some(mode), |ui| {
                    if theme::small_button(ui, "Edit", Kind::Secondary).clicked() {
                        let _ = std::process::Command::new("/usr/bin/open").arg("-t").arg(&s.path).status();
                    }
                });
            }
        });
        section(ui, "Write your own");
        card(ui, |ui| {
            ui.add_space(12.0);
            theme::para(ui, theme::body("Any language works. Put metadata in comments at the top, make the file executable, and Glint picks it up within a second.").weak());
            ui.add_space(8.0);
            let sample = "#!/bin/bash\n# @glint.title Say Hello\n# @glint.mode fullOutput   # silent · compact · inline · list\n# @glint.argument1 { \"type\": \"text\", \"placeholder\": \"Name\" }\necho \"Hello $1\"";
            egui::Frame::new().fill(egui::Color32::from_black_alpha(90)).stroke(Stroke::new(1.0, p.line)).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(theme::mono(sample, 12.0).color(p.ink)).selectable(true));
            });
            ui.add_space(8.0);
            theme::para(ui, theme::caption("List mode is Glint's own: the script gets the search text as $1 and prints JSON like [{\"title\": \"…\", \"subtitle\": \"…\", \"icon\": \"🔥\", \"copy\": \"…\", \"open\": \"https://…\"}]."));
            ui.add_space(6.0);
            ui.add_space(6.0);
        });

    }
}

/// Turns off Spotlight's ⌘Space and puts Glint there.
pub fn take_spotlight(s: &mut Settings) {
    if macos::spotlight_shortcut_on() && macos::set_spotlight_shortcut(false) {
        Prefs::update(|p| p.spotlight_taken = true);
    }
    s.hotkey = "Cmd+Space".into();
}

/// Undo: Glint goes back to its own hotkey; Spotlight gets ⌘Space back if we took it.
pub fn give_back_spotlight(s: &mut Settings) {
    if Prefs::load().spotlight_taken {
        macos::set_spotlight_shortcut(true);
        Prefs::update(|p| p.spotlight_taken = false);
    }
    s.hotkey = DEFAULT_HOTKEY.into();
}
