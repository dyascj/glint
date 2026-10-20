//! First-run onboarding: seven pages from welcome to "Start using Glint".
//! The order: welcome, what's included, hotkey,
//! permissions), then a hands-on page that ticks off as you use Glint.

use eframe::egui::{self, Align, Layout, Margin, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::art::{self, smooth};
use super::hotkey::recorder;
use super::settings::{give_back_spotlight, take_spotlight};
use super::theme::{self, Kind, Mark, card, hairline};
use super::{chrome, open_launcher, runtime};
use crate::icons::Icons;
use crate::launcher::Runtime;
use crate::macos::{self, Img};
use crate::paths::{self, APP_NAME};
use crate::prefs::Prefs;
use crate::search::scripts;
use crate::settings::{FileScope, Settings, hotkey_label, hotkey_parts};

const PAGES: [(&str, &str); 7] = [
    ("Welcome", "Welcome"),
    ("What's included", "Out of the box"),
    ("Your hotkey", "Hotkey"),
    ("Permissions", "Permissions"),
    ("Try it", "Try it"),
    ("Extensions", "Extensions"),
    ("Done", "Done"),
];
const TRY: usize = 4;
const DONE: usize = 6;

/// (SF Symbol, title, detail)
const FEATURES: [(&str, &str, &str); 6] = [
    ("square.grid.2x2", "App Launcher", "Open any app in a few keystrokes. Glint learns what you use most."),
    ("doc.text.magnifyingglass", "File Search", "Files and folders from Spotlight's index, with Quick Look and actions."),
    ("plus.forwardslash.minus", "Calculator", "Math, percentages and units, like 24px to rem, as you type."),
    ("doc.on.clipboard", "Clipboard History", "Everything you copy, searchable, pinnable and ready to paste."),
    ("eyedropper", "Colors", "Pick any pixel, keep a palette, convert HEX, RGB, HSL and OKLCH."),
    ("power", "System", "Lock, sleep, empty the Trash, dark mode, quit apps and more."),
];

/// (title, how, runtime key)
const STEPS: [(&str, &str, &str); 5] = [
    ("Open Glint", "Press your hotkey from anywhere", "open"),
    ("Launch an app", "Type a few letters of its name, press Return", "app"),
    ("Do some math", "Type 24px to rem, then Return copies the answer", "calc"),
    ("Pick a color", "Run Pick Color and click any pixel", "color"),
    ("Find a file", "Run Search Files and type part of a name", "file"),
];

pub struct Onboarding {
    page: usize,
    entered: f64,
    s: Settings,
    saved: Settings,
    icons: Icons,
    ax: bool,
    ax_requested: bool,
    next_poll: f64,
    rt: Runtime,
    reduce_motion: bool,
    spotlight_on: bool,
    note: Option<String>,
}

impl Onboarding {
    pub fn new(ctx: &egui::Context) -> Onboarding {
        let s = Settings::load();
        // Dev builds: GLINT_UI_PAGE=<n> opens a page directly (for snapshots).
        let page = if cfg!(debug_assertions) { std::env::var("GLINT_UI_PAGE").ok().and_then(|p| p.parse().ok()).unwrap_or(0) } else { 0 };
        Onboarding {
            page,
            entered: 0.0,
            saved: s.clone(),
            s,
            icons: Icons::new(ctx),
            ax: macos::ax_trusted(),
            ax_requested: false,
            next_poll: 0.0,
            rt: runtime(),
            reduce_motion: super::reduce_motion(),
            spotlight_on: macos::spotlight_shortcut_on(),
            note: None,
        }
    }

    fn go(&mut self, page: usize, now: f64) {
        self.page = page.min(DONE);
        self.entered = now;
        self.note = None;
    }

    /// Completes onboarding and closes; `start` opens the launcher.
    fn finish(&mut self, ctx: &egui::Context, start: bool) {
        let _ = self.s.save();
        Prefs::update(|p| p.onboarding_completed = true);
        if start {
            let _ = crate::login::apply(self.s.start_at_login);
            open_launcher();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn fade(&self) -> f64 {
        if self.reduce_motion { f64::NEG_INFINITY } else { self.entered }
    }

    fn animate(&self, ui: &Ui) -> f64 {
        if self.reduce_motion {
            0.0
        } else {
            ui.ctx().request_repaint();
            ui.input(|i| i.time)
        }
    }

    fn done_mask(&self) -> Vec<bool> {
        STEPS.iter().map(|(_, _, k)| self.rt.tried.contains(*k)).collect()
    }
}

impl eframe::App for Onboarding {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::GLASS
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        chrome(ui);
        let ctx = ui.ctx().clone();
        self.icons.pump(&ctx);
        let now = ctx.input(|i| i.time);
        let p = theme::pal();
        if now >= self.next_poll {
            self.rt = runtime();
            self.ax = macos::ax_trusted();
            self.next_poll = now + 0.6;
        }
        ctx.request_repaint_after_secs(0.6);

        egui::Panel::top("top").exact_size(56.0).show_separator_line(false).frame(theme::glass(Margin::ZERO)).show(ui, |ui| {
            let r = ui.max_rect();
            let mark = Rect::from_center_size(pos2(r.left() + 92.0, r.center().y + 2.0), vec2(18.0, 15.0));
            art::mark(ui.painter(), mark, &p);
            let g = theme::galley(ui, theme::text(APP_NAME, 15.0, theme::BOLD).extra_letter_spacing(-0.3), f32::INFINITY);
            ui.painter().galley(pos2(mark.right() + 8.0, mark.center().y - g.size().y / 2.0), g, p.ink);
            let mut x = r.right() - 28.0;
            for i in (0..PAGES.len()).rev() {
                let w = if i == self.page { 26.0 } else { 14.0 };
                let w = ctx.animate_value_with_time(ui.scope_id().with(("progress", i)), w, 0.2);
                let seg = Rect::from_min_size(pos2(x - w, r.center().y), vec2(w, 4.0));
                ui.painter().rect_filled(seg, 2, if i <= self.page { p.ink } else { p.line });
                let resp = ui.interact(seg.expand(6.0), ui.scope_id().with(("seg", i)), Sense::hover());
                resp.on_hover_text(format!("{}, page {} of {}", PAGES[i].0, i + 1, PAGES.len()));
                x -= w + 5.0;
            }
        });

        egui::Panel::bottom("bottom").exact_size(68.0).show_separator_line(false).frame(theme::glass(Margin::symmetric(40, 0))).show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, p.line));
            ui.horizontal_centered(|ui| {
                if self.page < DONE {
                    let later = if self.page == TRY { "Skip for now" } else { "Set up later" };
                    if theme::button(ui, later, Kind::Plain).clicked() {
                        self.finish(&ctx, false);
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let primary = match self.page {
                        0 => "Get started".to_string(),
                        DONE => format!("Start using {APP_NAME}"),
                        _ => "Continue".to_string(),
                    };
                    if theme::button(ui, &format!("{primary}  →"), Kind::Primary).clicked() {
                        if self.page == DONE {
                            self.finish(&ctx, true);
                        } else {
                            self.go(self.page + 1, now);
                        }
                    }
                    if self.page > 0 && theme::button(ui, "←  Back", Kind::Secondary).clicked() {
                        self.go(self.page - 1, now);
                    }
                });
            });
        });

        egui::CentralPanel::default().frame(theme::glass(Margin { left: 48, right: 48, top: 16, bottom: 12 })).show(ui, |ui| {
            let f = self.fade();
            rise(ui, f, 0, |ui| {
                ui.label(theme::eyebrow(PAGES[self.page].1));
            });
            ui.add_space(2.0);
            match self.page {
                0 => self.welcome(ui),
                1 => self.features(ui),
                2 => self.hotkey(ui),
                3 => self.permissions(ui),
                TRY => self.try_it(ui),
                5 => self.extensions(ui),
                _ => self.done(ui),
            }
        });

        if self.s != self.saved && ctx.dragged_id().is_none() {
            let _ = self.s.save();
            self.saved = self.s.clone();
        }
    }
}

/// Fade-in for the i-th block of a page.
fn rise(ui: &mut Ui, start: f64, i: usize, add: impl FnOnce(&mut Ui)) {
    let now = ui.input(|inp| inp.time);
    let t = smooth(((now - start) as f32 - i as f32 * 0.07) / 0.35);
    if t < 1.0 {
        ui.ctx().request_repaint();
    }
    ui.scope(|ui| {
        ui.multiply_opacity(t);
        add(ui)
    });
}

fn heading(ui: &mut Ui, s: &str) {
    ui.add(egui::Label::new(theme::title(s, 20.0)).wrap());
}

fn big_keys(ui: &mut Ui, hotkey: &str) {
    let p = theme::pal();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for cap in hotkey_parts(hotkey) {
            let g = theme::galley(ui, theme::text(&cap, 18.0, theme::MEDIUM), f32::INFINITY);
            let (r, _) = ui.allocate_exact_size(vec2((g.size().x + 26.0).max(46.0), 46.0), Sense::hover());
            ui.painter().rect_filled(r, 10, p.knob);
            ui.painter().rect_stroke(r, 10, Stroke::new(1.0, theme::mix(p.line, egui::Color32::WHITE, 0.1)), egui::StrokeKind::Inside);
            ui.painter().hline(r.shrink(6.0).x_range(), r.bottom() - 1.5, Stroke::new(1.0, egui::Color32::from_black_alpha(90)));
            ui.painter().galley(r.center() - g.size() / 2.0, g, p.bright);
        }
    });
}

impl Onboarding {
    fn welcome(&mut self, ui: &mut Ui) {
        let f = self.fade();
        let p = theme::pal();
        rise(ui, f, 1, |ui| heading(ui, "Everything, one keystroke away."));
        ui.add_space(4.0);
        rise(ui, f, 2, |ui| {
            theme::para(ui, theme::body("Glint opens over whatever you're doing. Type a few letters to launch an app, find a file, do the math or grab a color, and it gets out of the way.").weak());
        });
        ui.add_space(14.0);
        let t = self.animate(ui);
        rise(ui, f, 3, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 196.0), Sense::hover());
            art::launcher_mock(ui.painter(), r, &p, t, None);
        });
        ui.add_space(14.0);
        rise(ui, f, 4, |ui| {
            ui.horizontal(|ui| {
                for c in ["Apps and files", "Math and colors", "Your own extensions"] {
                    theme::pill(ui, Mark::On, c);
                }
            });
        });
    }

    fn features(&mut self, ui: &mut Ui) {
        let f = self.fade();
        let p = theme::pal();
        rise(ui, f, 1, |ui| heading(ui, "Packed in, out of the box."));
        ui.add_space(2.0);
        rise(ui, f, 2, |ui| {
            ui.label(theme::body("No setup, no store. These all work the moment Glint opens.").weak());
        });
        ui.add_space(14.0);
        rise(ui, f, 3, |ui| {
            for chunk in FEATURES.chunks(3) {
                ui.columns(3, |cols| {
                    for (col, (sym, title, detail)) in cols.iter_mut().zip(chunk) {
                        card(col, |ui| ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.add_space(14.0);
                            let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                            ui.painter().rect_filled(r, 8, p.knob);
                            self.icons.paint(ui.painter(), &Img::Symbol(sym), r.shrink(6.0), p.accent);
                            ui.add_space(8.0);
                            ui.label(theme::text(*title, 13.0, theme::SEMIBOLD).color(p.bright));
                            ui.add_space(3.0);
                            ui.add(egui::Label::new(theme::caption(*detail)).wrap());
                            ui.add_space(14.0);
                        }));
                    }
                });
                ui.add_space(8.0);
            }
        });
    }

    fn hotkey(&mut self, ui: &mut Ui) {
        let f = self.fade();
        rise(ui, f, 1, |ui| heading(ui, "Pick your hotkey."));
        ui.add_space(2.0);
        rise(ui, f, 2, |ui| {
            theme::para(ui, theme::body(format!("This opens {APP_NAME} from any app. Most people put it on ⌘ Space, where Spotlight lives.")).weak());
        });
        ui.add_space(16.0);
        let hk = self.s.hotkey.clone();
        rise(ui, f, 3, |ui| big_keys(ui, &hk));
        ui.add_space(16.0);
        let on_cmd_space = self.s.hotkey == "Cmd+Space";
        rise(ui, f, 4, |ui| {
            ui.columns(2, |cols| {
                card(&mut cols[0], |ui| {
                    ui.add_space(14.0);
                    ui.label(theme::text("Record your own", 13.0, theme::SEMIBOLD));
                    ui.add(egui::Label::new(theme::caption("Click, then press a combination with ⌘, ⌥ or ⌃.")).wrap());
                    ui.add_space(10.0);
                    let mut v = Some(self.s.hotkey.clone());
                    if recorder(ui, "onboarding", &mut v, false).changed()
                        && let Some(h) = v
                    {
                        self.s.hotkey = h;
                    }
                    ui.add_space(14.0);
                });
                card(&mut cols[1], |ui| {
                    ui.add_space(14.0);
                    ui.label(theme::text("Replace Spotlight", 13.0, theme::SEMIBOLD));
                    let detail = if on_cmd_space { "Glint is on ⌘ Space. Undo gives Spotlight its shortcut back." } else { "Turns off Spotlight's ⌘ Space and gives it to Glint. You can undo this." };
                    ui.add(egui::Label::new(theme::caption(detail)).wrap());
                    ui.add_space(10.0);
                    if on_cmd_space {
                        if theme::small_button(ui, "Undo", Kind::Secondary).clicked() {
                            give_back_spotlight(&mut self.s);
                            self.spotlight_on = macos::spotlight_shortcut_on();
                        }
                    } else if theme::small_button(ui, "Use ⌘ Space", Kind::Primary).clicked() {
                        take_spotlight(&mut self.s);
                        self.spotlight_on = macos::spotlight_shortcut_on();
                    }
                    ui.add_space(14.0);
                });
            });
        });
        ui.add_space(10.0);
        if let Some(e) = self.rt.hotkey_error.clone() {
            theme::callout(ui, "That hotkey is taken", &format!("{e} Change that app's hotkey, or record another one."), None);
        } else if on_cmd_space {
            theme::para(ui, theme::caption("If another app already uses ⌘ Space, change its hotkey so Glint can have it."));
        }
    }

    fn permissions(&mut self, ui: &mut Ui) {
        let f = self.fade();
        let p = theme::pal();
        let full = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(full - 290.0, ui.available_height()), Layout::top_down(Align::Min), |ui| {
                let title = if self.ax { "You're all set." } else { "Paste right where you are." };
                rise(ui, f, 1, |ui| heading(ui, title));
                ui.add_space(4.0);
                rise(ui, f, 2, |ui| {
                    theme::para(ui, theme::body("Accessibility lets Glint paste clipboard history, snippets and emoji into the app you were using. It's optional: without it, Glint copies and you paste.").weak());
                });
                ui.add_space(12.0);
                rise(ui, f, 3, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for reason in ["Only used to press ⌘V for you", "Never reads what's on your screen", "Nothing leaves your Mac"] {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(10.0, 16.0), Sense::hover());
                            ui.painter().circle_filled(r.center(), 2.5, p.ink);
                            ui.label(theme::body(reason));
                        });
                    }
                });
                ui.add_space(16.0);
                rise(ui, f, 4, |ui| {
                    if self.ax {
                        theme::pill(ui, Mark::On, "Accessibility access granted");
                    } else if !self.ax_requested {
                        if theme::button(ui, "Grant Access ↗", Kind::Primary).clicked() {
                            macos::ax_prompt();
                            macos::open(macos::ACCESSIBILITY_URL);
                            self.ax_requested = true;
                        }
                    } else {
                        ui.label(theme::text("Waiting for permission…", 13.0, theme::SEMIBOLD));
                        theme::para(ui, theme::caption(format!("Turn on {APP_NAME} in the list. If it's already there, switch it off and on again.")));
                        ui.add_space(6.0);
                        if theme::small_button(ui, "Open System Settings", Kind::Secondary).clicked() {
                            macos::open(macos::ACCESSIBILITY_URL);
                        }
                    }
                });
                ui.add_space(14.0);
                rise(ui, f, 5, |ui| {
                    theme::para(ui, theme::caption("Files come from Spotlight's index, so there's no Full Disk Access to grant. macOS asks once the first time a command controls Finder or System Events."));
                });
            });
            ui.add_space(18.0);
            rise(ui, f, 3, |ui| {
                ui.vertical(|ui| {
                    ui.set_width(270.0);
                    card(ui, |ui| {
                        ui.add_space(12.0);
                        ui.label(theme::text("Privacy & Security › Accessibility", 10.5, theme::MEDIUM).weak());
                        ui.add_space(6.0);
                        ui.label(theme::text("Accessibility", 15.0, theme::BOLD));
                        ui.add_space(2.0);
                        theme::para(ui, theme::caption("Allow these apps to control your computer."));
                        ui.add_space(8.0);
                        let mut glint = self.ax;
                        let mut clicked = false;
                        let mut others = [true, false];
                        for (i, name) in [APP_NAME, "Terminal", "Slat"].iter().enumerate() {
                            hairline(ui);
                            ui.horizontal(|ui| {
                                ui.set_height(36.0);
                                ui.label(theme::text(*name, 12.5, theme::MEDIUM));
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let on = if i == 0 { &mut glint } else { &mut others[i - 1] };
                                    if theme::toggle(ui, on).changed() && i == 0 {
                                        clicked = true;
                                    }
                                });
                            });
                        }
                        if clicked && !self.ax {
                            macos::ax_prompt();
                            macos::open(macos::ACCESSIBILITY_URL);
                            self.ax_requested = true;
                        }
                        ui.add_space(4.0);
                    });
                });
            });
        });
    }

    fn try_it(&mut self, ui: &mut Ui) {
        let f = self.fade();
        let p = theme::pal();
        let done = self.done_mask();
        let count = done.iter().filter(|d| **d).count();
        let all = count == STEPS.len();
        let hk = hotkey_label(&self.s.hotkey);
        rise(ui, f, 1, |ui| heading(ui, if all { "You've got it." } else { "Give it a go." }));
        rise(ui, f, 2, |ui| {
            let body = if all { "All five, nicely done.".to_string() } else { format!("Press {hk} and try each one. They tick off as you go.") };
            ui.label(theme::body(body).weak());
        });
        ui.add_space(10.0);
        rise(ui, f, 3, |ui| {
            card(ui, |ui| {
                for (i, (title, how, _)) in STEPS.iter().enumerate() {
                    if i > 0 {
                        hairline(ui);
                    }
                    let t = ui.ctx().animate_bool_with_time(ui.scope_id().with(("tick", i)), done[i], 0.2);
                    ui.horizontal(|ui| {
                        ui.set_height(44.0);
                        let (r, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                        let c = r.center();
                        ui.painter().circle_stroke(c, 8.0, Stroke::new(1.2, theme::mix(p.line, p.ink, t)));
                        ui.painter().circle_filled(c, 8.0 * t, p.ink);
                        if t > 0.5 {
                            ui.painter().line(vec![c + vec2(-3.5, 0.2), c + vec2(-1.0, 2.7), c + vec2(3.6, -2.6)], Stroke::new(1.6, p.bg));
                        }
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add_space(5.0);
                            ui.label(theme::text(*title, 13.0, theme::MEDIUM));
                            ui.label(theme::caption(*how));
                        });
                    });
                }
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label(theme::mono(format!("{count} / {}", STEPS.len()), 12.0));
            if !all {
                let t = self.animate(ui);
                let pulse = 0.35 + 0.65 * (0.5 + 0.5 * (t * 3.0).sin() as f32);
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, p.accent.gamma_multiply(pulse));
                ui.label(theme::caption("Listening"));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::small_button(ui, &format!("Open {APP_NAME}"), Kind::Secondary).clicked() {
                    open_launcher();
                }
            });
        });
    }

    fn extensions(&mut self, ui: &mut Ui) {
        let f = self.fade();
        let p = theme::pal();
        rise(ui, f, 1, |ui| heading(ui, "Make it yours."));
        ui.add_space(2.0);
        rise(ui, f, 2, |ui| {
            theme::para(ui, theme::body("Extensions are scripts, in any language. Drop one in the Extensions folder and it becomes a command.").weak());
        });
        ui.add_space(12.0);
        rise(ui, f, 3, |ui| {
            let sample = "#!/bin/bash\n# @glint.title Say Hello\n# @glint.mode fullOutput\n# @glint.argument1 { \"type\": \"text\", \"placeholder\": \"Name\" }\n\necho \"Hello, $1!\"";
            egui::Frame::new().fill(egui::Color32::from_black_alpha(100)).stroke(Stroke::new(1.0, p.line)).corner_radius(10).inner_margin(Margin::same(14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(theme::mono(sample, 12.0).color(p.ink));
            });
        });
        ui.add_space(10.0);
        rise(ui, f, 4, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (mode, what) in [("fullOutput", "shows output"), ("list", "searchable JSON results"), ("compact", "a one-line HUD"), ("silent", "runs quietly"), ("inline", "a live subtitle")] {
                    theme::pill(ui, Mark::Off, &format!("{mode} · {what}"));
                }
            });
        });
        ui.add_space(14.0);
        rise(ui, f, 5, |ui| {
            ui.horizontal(|ui| {
                if theme::button(ui, "Add Sample Extensions", Kind::Primary).clicked() {
                    self.note = Some(match scripts::install_samples() {
                        Ok(0) => "The samples are already in your Extensions folder.".into(),
                        Ok(n) => format!("Added {n} samples. Search for “Say Hello” or “Recent Downloads”."),
                        Err(e) => format!("Couldn't add samples: {e}"),
                    });
                }
                if theme::button(ui, "Open Folder", Kind::Secondary).clicked() {
                    paths::ensure_dirs();
                    macos::open_path(&paths::extensions_dir());
                }
            });
            if let Some(n) = &self.note {
                ui.add_space(6.0);
                ui.label(theme::caption(n));
            }
        });
    }

    fn done(&mut self, ui: &mut Ui) {
        let f = self.fade();
        let hk = hotkey_label(&self.s.hotkey);
        rise(ui, f, 1, |ui| heading(ui, "Ready when you are."));
        ui.add_space(4.0);
        rise(ui, f, 2, |ui| {
            theme::para(ui, theme::body(format!("Press {hk} anywhere. Settings live in the menu bar, or press ⌘, inside {APP_NAME}.")).weak());
        });
        ui.add_space(18.0);
        let ext = scripts::scan().len();
        let cells = [
            ("Hotkey", hk.clone()),
            ("Files", if self.s.file_scope == FileScope::Home { "Home".to_string() } else { "Everywhere".to_string() }),
            ("Clipboard", if self.s.clipboard_history { "On".to_string() } else { "Off".to_string() }),
            ("Extensions", ext.to_string()),
        ];
        rise(ui, f, 3, |ui| {
            ui.columns(4, |cols| {
                for (col, (label, value)) in cols.iter_mut().zip(cells) {
                    card(col, |ui| {
                        ui.add_space(14.0);
                        ui.label(theme::eyebrow(label));
                        ui.add_space(6.0);
                        ui.label(theme::title(value, 20.0));
                        ui.add_space(14.0);
                    });
                }
            });
        });
        ui.add_space(20.0);
        rise(ui, f, 4, |ui| {
            if theme::checkbox(ui, &mut self.s.start_at_login, &format!("Open {APP_NAME} when I log in")).changed() {
                let _ = crate::login::apply(self.s.start_at_login);
            }
        });
        ui.add_space(18.0);
        rise(ui, f, 5, |ui| {
            let p = theme::pal();
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
            let t = self.animate(ui);
            art::launcher_mock(ui.painter(), r, &p, t, None);
        });
    }
}
