//! Drawing the launcher and its keyboard: search bar, results, grids,
//! detail pane, action bar, the ⌘K actions panel and confirmations.

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Frame, Id, Key, Layout, Margin, Order, Rect, Response, ScrollArea, Sense, Stroke, StrokeKind,
    TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::items::{self, Action, Icon, Item, Kind, Run, Shortcut};
use super::{EXTENSION_MODES, Launcher, View};
use crate::commands::{self, Cmd};
use crate::icons::Icons;
use crate::macos::Img;
use crate::search::color::Rgb;
use crate::settings::{ColorFormat, hotkey_parts};
use crate::ui::art;
use crate::ui::theme::{self, MEDIUM, REGULAR, SEMIBOLD, glass, keycap, mono, pal, text};

const EMOJI_COLS: usize = 9;
const COLOR_COLS: usize = 6;
const TOAST_SECS: f64 = 2.6;

/// One emoji grid cell.
pub type Emoji = &'static emojis::Emoji;

fn group_name(g: emojis::Group) -> &'static str {
    use emojis::Group::*;
    match g {
        SmileysAndEmotion => "Smileys & Emotion",
        PeopleAndBody => "People & Body",
        AnimalsAndNature => "Animals & Nature",
        FoodAndDrink => "Food & Drink",
        TravelAndPlaces => "Travel & Places",
        Activities => "Activities",
        Objects => "Objects",
        Symbols => "Symbols",
        Flags => "Flags",
    }
}

/// Removes one key press with exactly these modifiers (egui's own matching
/// ignores extra Shift, which would mix up ⌘↵ and ⌘⇧↵).
fn take(ctx: &egui::Context, cmd: bool, shift: bool, ctrl: bool, key: Key) -> bool {
    ctx.input_mut(|i| {
        let pos = i.events.iter().position(|e| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. }
                if *k == key && m.command == cmd && m.shift == shift && m.ctrl == ctrl && !m.alt)
        });
        pos.map(|p| i.events.remove(p)).is_some()
    })
}

fn take_shortcut(ctx: &egui::Context, s: &Shortcut) -> bool {
    take(ctx, s.cmd, s.shift, s.ctrl, s.key)
}

impl Launcher {
    fn grid_cols(&self) -> Option<usize> {
        match self.page().view {
            View::Emoji => Some(EMOJI_COLS),
            View::Colors => Some(COLOR_COLS),
            _ => None,
        }
    }

    // ---- data for grids ----

    pub fn emoji_sections(&self) -> Vec<(String, Vec<Emoji>)> {
        let q = self.page().query.trim().to_lowercase();
        if q.is_empty() {
            let mut recent: Vec<(i32, Emoji)> = emojis::iter().filter_map(|e| {
                let b = self.frecency.bonus(&format!("emoji:{}", e.as_str()));
                (b > 0).then_some((b, e))
            }).collect();
            recent.sort_by(|a, b| b.0.cmp(&a.0));
            let mut out = Vec::new();
            if !recent.is_empty() {
                out.push(("Frequently Used".to_string(), recent.into_iter().take(EMOJI_COLS * 2).map(|(_, e)| e).collect()));
            }
            for g in emojis::Group::iter() {
                out.push((group_name(g).to_string(), g.emojis().collect()));
            }
            return out;
        }
        let mut hits: Vec<(i32, Emoji)> = emojis::iter()
            .filter_map(|e| {
                let codes: Vec<&str> = e.shortcodes().collect();
                crate::search::fuzzy::score(&q, e.name(), &codes).filter(|s| *s >= 500).map(|s| (s + self.frecency.bonus(&format!("emoji:{}", e.as_str())), e))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        vec![("Results".into(), hits.into_iter().map(|(_, e)| e).collect())]
    }

    fn selected_emoji(&self) -> Option<Emoji> {
        self.emoji_sections().into_iter().flat_map(|(_, v)| v).nth(self.page().selected)
    }

    /// Saved colors matching the query; index 0 is the Pick Color tile.
    pub fn color_cells(&self) -> Vec<Option<(usize, Rgb)>> {
        let q = self.page().query.trim().to_lowercase();
        let mut cells = vec![None];
        cells.extend(self.colors.iter().enumerate().filter(|(_, c)| {
            q.is_empty() || ColorFormat::ALL.iter().any(|f| c.color.format(*f).to_lowercase().contains(q.trim_start_matches('#')))
        }).map(|(i, c)| Some((i, c.color))));
        cells
    }

    fn grid_len(&self) -> usize {
        match self.page().view {
            View::Emoji => self.emoji_sections().iter().map(|(_, v)| v.len()).sum(),
            View::Colors => self.color_cells().len(),
            _ => self.item_count(),
        }
    }

    /// The actions for whatever is selected, and its ranking id.
    pub fn current_actions(&self) -> (String, Vec<Action>) {
        let a = |title: &str, symbol: &'static str, sc: Option<Shortcut>, run: Run| Action { title: title.into(), symbol, shortcut: sc, run };
        match &self.page().view {
            View::Emoji => match self.selected_emoji() {
                Some(e) => (
                    format!("emoji:{}", e.as_str()),
                    vec![
                        a("Paste Emoji", "doc.on.clipboard", None, Run::Paste(e.as_str().into())),
                        a("Copy Emoji", "doc.on.doc", Some(Shortcut::cmd(Key::Enter)), Run::Copy(e.as_str().into())),
                        a("Copy Name", "textformat", Some(Shortcut::cmd_shift(Key::C)), Run::Copy(e.shortcode().map_or_else(|| e.name().to_string(), |s| format!(":{s}:")))),
                    ],
                ),
                None => (String::new(), vec![]),
            },
            View::Colors => match self.color_cells().get(self.page().selected) {
                Some(Some((i, c))) => {
                    let mut v = items::color_actions(*c, self.settings.color_format);
                    v.push(a("Pick Another Color", "eyedropper", Some(Shortcut::cmd(Key::P)), Run::Cmd(Cmd::PickColor)));
                    v.push(a("Delete Color", "trash", Some(Shortcut::ctrl(Key::X)), Run::DeleteColor(*i)));
                    (String::new(), v)
                }
                Some(None) => (commands::def(Cmd::PickColor).id(), vec![a("Pick Color", "eyedropper", None, Run::Cmd(Cmd::PickColor))]),
                None => (String::new(), vec![]),
            },
            View::Output { text, rerun, .. } => {
                let path = rerun.0.clone();
                (
                    String::new(),
                    vec![
                        a("Copy Output", "doc.on.doc", None, Run::Copy(text.clone().unwrap_or_default())),
                        a("Run Again", "arrow.clockwise", Some(Shortcut::cmd(Key::R)), Run::Rerun(rerun.0.clone(), rerun.1.clone())),
                        a("Edit Script", "pencil", Some(Shortcut::cmd(Key::E)), Run::EditFile(path.clone())),
                        a("Show in Finder", "folder", Some(Shortcut::cmd_shift(Key::F)), Run::Reveal(path)),
                    ],
                )
            }
            View::Form(_) => (String::new(), vec![a("Save", "checkmark", None, Run::SubmitForm)]),
            View::Args { .. } => (String::new(), vec![a("Run", "play", None, Run::SubmitArgs)]),
            _ => self.selected_item().map_or((String::new(), vec![]), |it| (it.id.clone(), it.actions.clone())),
        }
    }

    fn run_action(&mut self, id: &str, action: Action, now: f64) {
        self.actions = None;
        if matches!(self.page().view, View::Emoji) {
            self.tried("emoji");
        }
        self.run(action.run, id, now);
    }

    fn move_selection(&mut self, delta: isize) {
        let n = self.grid_len();
        if n == 0 {
            return;
        }
        let page = self.page_mut();
        page.selected = (page.selected as isize + delta).clamp(0, n as isize - 1) as usize;
        self.scroll_to_selected = true;
    }

    // ---- keys ----

    fn keys(&mut self, ctx: &egui::Context, now: f64) {
        if let Some(c) = self.confirm {
            if take(ctx, false, false, false, Key::Enter) {
                self.run_cmd(c, now);
            } else if take(ctx, false, false, false, Key::Escape) {
                self.confirm = None;
            }
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Text(_) | egui::Event::Key { .. })));
            return;
        }
        let (id, acts) = self.current_actions();
        if let Some(panel) = &mut self.actions {
            let q = panel.query.to_lowercase();
            let shown: Vec<&Action> = acts.iter().filter(|a| a.title.to_lowercase().contains(&q)).collect();
            if take(ctx, false, false, false, Key::Escape) || take(ctx, true, false, false, Key::K) {
                self.actions = None;
                self.focus_query = true;
                return;
            }
            if take(ctx, false, false, false, Key::ArrowDown) || take(ctx, false, false, true, Key::N) {
                panel.selected = (panel.selected + 1).min(shown.len().saturating_sub(1));
            }
            if take(ctx, false, false, false, Key::ArrowUp) || take(ctx, false, false, true, Key::P) {
                panel.selected = panel.selected.saturating_sub(1);
            }
            if take(ctx, false, false, false, Key::Enter)
                && let Some(a) = shown.get(panel.selected).map(|a| (*a).clone())
            {
                return self.run_action(&id, a, now);
            }
            for a in &acts {
                if let Some(s) = a.shortcut
                    && take_shortcut(ctx, &s)
                {
                    return self.run_action(&id, a.clone(), now);
                }
            }
            return;
        }

        if take(ctx, true, false, false, Key::K) {
            if !acts.is_empty() {
                self.actions = Some(super::ActionsPanel { query: String::new(), selected: 0 });
            }
            return;
        }
        if take(ctx, true, false, false, Key::Comma) {
            return self.run_cmd(Cmd::Settings, now);
        }
        if take(ctx, true, false, false, Key::W) {
            return self.hide(now);
        }
        if take(ctx, false, false, false, Key::Escape) {
            if self.stack.len() > 1 {
                self.pop();
            } else if !self.page().query.is_empty() {
                self.page_mut().query.clear();
            } else {
                self.hide(now);
            }
            return;
        }
        // Specific shortcuts (⌘⇧↵ before ⌘↵) from the selected item's actions.
        for a in acts.iter().skip(1) {
            if let Some(s) = a.shortcut
                && take_shortcut(ctx, &s)
            {
                return self.run_action(&id, a.clone(), now);
            }
        }
        let form_multiline = matches!(&self.page().view, View::Form(f) if f.fields.get(f.focus).is_some_and(|x| x.3));
        if take(ctx, true, false, false, Key::Enter) {
            let a = if matches!(self.page().view, View::Form(_)) { acts.first() } else { acts.get(1) };
            if let Some(a) = a.cloned() {
                return self.run_action(&id, a, now);
            }
            return;
        }
        if !form_multiline && take(ctx, false, false, false, Key::Enter) {
            // In a form, ↵ steps to the next field until the last one.
            if let View::Form(f) = &mut self.page_mut().view
                && f.focus + 1 < f.fields.len()
            {
                f.focus += 1;
                self.focus_query = true;
                return;
            }
            if let View::Args { values, focus, .. } = &mut self.page_mut().view
                && *focus + 1 < values.len()
                && values[*focus + 1].is_empty()
                && !values[*focus].is_empty()
            {
                *focus += 1;
                self.focus_query = true;
                return;
            }
            if let Some(a) = acts.first().cloned() {
                return self.run_action(&id, a, now);
            }
            return;
        }
        let tab = take(ctx, false, false, false, Key::Tab);
        let back_tab = take(ctx, false, true, false, Key::Tab);
        if tab || back_tab {
            let step = |focus: &mut usize, n: usize| *focus = if back_tab { (*focus + n - 1) % n } else { (*focus + 1) % n };
            match &mut self.page_mut().view {
                View::Form(f) => step(&mut f.focus, f.fields.len()),
                View::Args { focus, values, .. } => step(focus, values.len()),
                _ => {}
            }
            self.focus_query = true;
            return;
        }
        if matches!(self.page().view, View::Form(_)) {
            return;
        }
        let cols = self.grid_cols();
        if take(ctx, false, false, false, Key::ArrowDown) || take(ctx, false, false, true, Key::N) || take(ctx, false, false, true, Key::J) {
            self.move_selection(cols.unwrap_or(1) as isize);
        }
        if take(ctx, false, false, false, Key::ArrowUp) || take(ctx, false, false, true, Key::P) || take(ctx, false, false, true, Key::K) {
            self.move_selection(-(cols.unwrap_or(1) as isize));
        }
        if cols.is_some() {
            if take(ctx, false, false, false, Key::ArrowRight) {
                self.move_selection(1);
            }
            if take(ctx, false, false, false, Key::ArrowLeft) {
                self.move_selection(-1);
            }
        }
        // ⌫ on an empty search goes back.
        let empty = self.page().query.is_empty() && !matches!(self.page().view, View::Args { .. });
        if self.stack.len() > 1 && empty && take(ctx, false, false, false, Key::Backspace) {
            self.pop();
        }
    }

    // ---- frame ----

    pub fn draw(&mut self, ui: &mut Ui, now: f64) {
        let ctx = ui.ctx().clone();
        self.keys(&ctx, now);
        if !self.shown {
            return;
        }
        // keys may have changed the page or selection
        self.refresh_sections(now);
        egui::Panel::top("search").exact_size(58.0).show_separator_line(false).frame(glass(Margin::symmetric(18, 0))).show(ui, |ui| self.search_bar(ui, now));
        egui::Panel::bottom("bar").exact_size(42.0).show_separator_line(false).frame(glass(Margin::symmetric(14, 0))).show(ui, |ui| self.action_bar(ui, now));
        egui::CentralPanel::default().frame(glass(Margin { left: 8, right: 8, top: 6, bottom: 4 })).show(ui, |ui| self.content(ui, now));
        if self.actions.is_some() {
            self.actions_panel(&ctx, now);
        }
        if let Some(c) = self.confirm {
            self.confirm_card(&ctx, c, now);
        }
        if let Some((_, at, _)) = self.toast {
            if now - at > TOAST_SECS {
                self.toast = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
    }

    fn placeholder(&self) -> String {
        match &self.page().view {
            View::Root => "Search for apps, files and commands…".into(),
            View::Files => "Search files by name…".into(),
            View::Clipboard => "Type to filter clipboard history…".into(),
            View::Emoji => "Search emoji and symbols…".into(),
            View::Colors => "Filter saved colors…".into(),
            View::Calculations => "Search past calculations…".into(),
            View::Quicklinks => "Search quicklinks…".into(),
            View::Snippets => "Search snippets…".into(),
            View::Extensions => "Search extensions…".into(),
            View::Processes => "Search running apps…".into(),
            View::List { .. } => "Search…".into(),
            View::Menus { app, .. } if !app.is_empty() => format!("Search {app} menus…"),
            View::Menus { .. } => "Search menus…".into(),
            View::Windows { .. } => "Search open windows…".into(),
            _ => String::new(),
        }
    }

    fn search_bar(&mut self, ui: &mut Ui, now: f64) {
        let p = pal();
        let r = ui.max_rect();
        let depth = self.stack.len();
        ui.painter().hline((r.left() - 18.0)..=(r.right() + 18.0), r.bottom() - 0.5, Stroke::new(1.0, p.line));
        let loading = self.file_loading
            || matches!(&self.page().view, View::Output { text: None, .. } | View::List { due: Some(_), .. } | View::Menus { items: None, error: None, .. } | View::Windows { items: None, .. });
        if loading {
            // A thin progress sweep under the search bar.
            let t = ((now * 0.9) % 1.0) as f32;
            let w = r.width() * 0.3;
            let x = r.left() - 18.0 + (r.width() + 36.0 + w) * t - w;
            let seg = Rect::from_min_max(pos2(x.max(r.left() - 18.0), r.bottom() - 1.5), pos2((x + w).min(r.right() + 18.0), r.bottom() + 0.5));
            ui.painter().rect_filled(seg, 1, p.accent.gamma_multiply(0.8));
            ui.ctx().request_repaint();
        }
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if depth > 1 {
                let (br, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                let fill = if resp.hovered() { p.knob } else { p.chip };
                ui.painter().rect_filled(br, 8, fill);
                ui.painter().text(br.center(), Align2::CENTER_CENTER, "←", FontId::proportional(14.0), p.ink);
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    self.pop();
                    return;
                }
            }
            let title = self.page().view.title();
            let page_id = Id::unique(("page", depth));
            match &mut self.stack.last_mut().expect("page").view {
                View::Args { values, placeholders, secure, focus, .. } => {
                    ui.label(text(title, 15.0, MEDIUM).color(p.bright));
                    for i in 0..values.len() {
                        let id = page_id.with(("arg", i));
                        let resp = Frame::new().fill(p.raised).stroke(Stroke::new(1.0, if *focus == i { p.focus } else { p.line })).corner_radius(8).inner_margin(Margin::symmetric(10, 6)).show(ui, |ui| {
                            ui.add(TextEdit::singleline(&mut values[i]).id(id).frame(Frame::NONE).password(secure[i]).font(FontId::proportional(14.0)).desired_width(150.0)
                                .hint_text(text(placeholders[i].clone(), 14.0, REGULAR).color(p.dim)))
                        }).inner;
                        if resp.gained_focus() {
                            *focus = i;
                        }
                        if self.focus_query && *focus == i {
                            resp.request_focus();
                        }
                    }
                    self.focus_query = false;
                }
                View::Form(_) | View::Output { .. } => {
                    ui.label(text(title, 15.0, MEDIUM).color(p.bright));
                }
                _ => {
                    let id = page_id.with("query");
                    let hint = self.placeholder();
                    let page = self.stack.last_mut().expect("page");
                    let resp = ui.add(
                        TextEdit::singleline(&mut page.query)
                            .id(id)
                            .frame(Frame::NONE)
                            .font(FontId::proportional(18.0))
                            .text_color(p.bright)
                            .desired_width(ui.available_width())
                            .hint_text(text(hint, 18.0, REGULAR).color(p.dim)),
                    );
                    if resp.changed() {
                        self.scroll_to_selected = true;
                    }
                    if self.actions.is_none() && (self.focus_query || !resp.has_focus()) {
                        resp.request_focus();
                        self.focus_query = false;
                    }
                }
            }
        });
    }

    fn content(&mut self, ui: &mut Ui, now: f64) {
        match &self.page().view {
            View::Emoji => self.emoji_grid(ui),
            View::Colors => self.color_grid(ui),
            View::Output { .. } => self.output(ui),
            View::Form(_) => self.form_body(ui),
            View::Args { .. } => {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(theme::caption("Fill in the fields above, then press Return. Tab moves between them."));
                });
            }
            View::List { error: Some(e), .. } => {
                let e = e.clone();
                ui.add_space(12.0);
                theme::callout(ui, "The extension didn't return a list", &e, None);
            }
            View::Clipboard | View::Snippets | View::Files => self.split(ui, now),
            _ => {
                self.list(ui);
            }
        }
    }

    fn empty(&self, ui: &mut Ui) {
        let p = pal();
        let r = ui.max_rect();
        let (title, sub) = match &self.page().view {
            View::Files if self.page().query.trim().chars().count() < 2 => ("Search your files", "Type at least two letters of a file or folder name."),
            View::Files if self.file_loading => ("Searching…", ""),
            View::Clipboard => ("Nothing copied yet", "Copy something and it shows up here."),
            View::Snippets => ("No snippets yet", "Create one with Create Snippet."),
            View::List { .. } => ("No results", ""),
            View::Menus { error: Some(e), .. } => (e.as_str(), "Open an app, then try again."),
            View::Menus { items: None, .. } => ("Reading menus…", ""),
            View::Windows { items: None, .. } => ("Finding windows…", ""),
            View::Windows { .. } if self.page().query.is_empty() => ("No open windows", "Windows on other Spaces aren't listed."),
            _ => ("No results", "Try another word, or press ⌘K for actions."),
        };
        let c = r.center() - vec2(0.0, 20.0);
        art::mark_colored(ui.painter(), Rect::from_center_size(c - vec2(0.0, 26.0), vec2(30.0, 24.0)), p.faint, p.dim);
        ui.painter().text(c + vec2(0.0, 8.0), Align2::CENTER_CENTER, title, FontId::proportional(13.0), p.muted);
        ui.painter().text(c + vec2(0.0, 28.0), Align2::CENTER_CENTER, sub, FontId::proportional(12.0), p.dim);
    }

    /// The sectioned result list; returns the selected row's rect.
    fn list(&mut self, ui: &mut Ui) {
        if self.item_count() == 0 {
            return self.empty(ui);
        }
        let selected = self.page().selected;
        let scroll = std::mem::take(&mut self.scroll_to_selected);
        let moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
        let mut hovered = None;
        let mut clicked = None;
        let hotkeys = self.settings.hotkeys.clone();
        let (sections, icons) = (&self.sections, &mut self.icons);
        ScrollArea::vertical().auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut idx = 0;
            for s in sections {
                if s.items.is_empty() {
                    continue;
                }
                section_title(ui, &s.title);
                for it in &s.items {
                    let resp = row(ui, icons, it, idx == selected, hotkeys.get(&it.id).map(String::as_str));
                    if idx == selected && scroll {
                        resp.scroll_to_me(None);
                    }
                    if resp.hovered() && moved {
                        hovered = Some(idx);
                    }
                    if resp.clicked() {
                        clicked = Some(idx);
                    }
                    idx += 1;
                }
            }
            ui.add_space(6.0);
        });
        // With ⌘K open, the panel's actions stay bound to the row they were opened on.
        if let Some(h) = hovered
            && self.actions.is_none()
        {
            self.page_mut().selected = h;
        }
        if let Some(c) = clicked {
            self.page_mut().selected = c;
            let (id, acts) = self.current_actions();
            if let Some(a) = acts.into_iter().next() {
                let now = ui.input(|i| i.time);
                self.run_action(&id, a, now);
            }
        }
    }

    /// List on the left, details of the selection on the right.
    fn split(&mut self, ui: &mut Ui, _now: f64) {
        let full = ui.available_rect_before_wrap();
        let left_w = (full.width() * 0.44).round();
        let left = Rect::from_min_size(full.min, vec2(left_w, full.height()));
        let right = Rect::from_min_max(pos2(full.left() + left_w + 8.0, full.top()), full.max);
        ui.scope_builder(UiBuilder::new().max_rect(left), |ui| self.list(ui));
        let p = pal();
        ui.painter().vline(left.right() + 4.0, (full.top() - 6.0)..=(full.bottom() + 4.0), Stroke::new(1.0, p.line));
        let Some(it) = self.selected_item().cloned() else { return };
        ui.scope_builder(UiBuilder::new().max_rect(right.shrink2(vec2(10.0, 6.0))), |ui| {
            ui.set_clip_rect(right);
            if let Icon::Img(Img::File(path)) = &it.icon
                && it.preview.is_none()
            {
                ui.add_space(14.0);
                let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
                let icon_r = Rect::from_center_size(r.center(), vec2(80.0, 80.0));
                self.icons.paint(ui.painter(), &Img::File(path.clone()), icon_r, p.ink);
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Label::new(text(&it.title, 13.0, MEDIUM).color(p.bright)).wrap());
                });
                ui.add_space(12.0);
            } else if let Some(preview) = &it.preview {
                ScrollArea::vertical().id_salt("preview").max_height((ui.available_height() - 130.0).max(80.0)).auto_shrink([false, true]).show(ui, |ui| {
                    let shown: String = preview.chars().take(4000).collect();
                    ui.add(egui::Label::new(mono(shown, 12.5).color(p.ink)).wrap().selectable(true));
                });
                ui.add_space(10.0);
            }
            if !it.detail.is_empty() {
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (k, v) in it.detail.iter().rev() {
                        ui.horizontal(|ui| {
                            ui.set_height(26.0);
                            ui.label(text(k, 11.5, REGULAR).color(p.dim));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add(egui::Label::new(text(v, 11.5, REGULAR).color(p.ink)).truncate());
                            });
                        });
                        theme::hairline(ui);
                    }
                    ui.label(theme::eyebrow("Information"));
                });
            }
        });
    }

    fn emoji_grid(&mut self, ui: &mut Ui) {
        let sections = self.emoji_sections();
        if sections.iter().all(|(_, v)| v.is_empty()) {
            return self.empty(ui);
        }
        let p = pal();
        let selected = self.page().selected;
        let scroll = std::mem::take(&mut self.scroll_to_selected);
        let mut clicked = None;
        let icons = &mut self.icons;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            let w = ui.available_width();
            let cell = (w / EMOJI_COLS as f32).floor();
            let mut idx = 0;
            for (title, list) in &sections {
                if list.is_empty() {
                    continue;
                }
                section_title(ui, title);
                for chunk in list.chunks(EMOJI_COLS) {
                    let (row_r, _) = ui.allocate_exact_size(vec2(w, cell), Sense::hover());
                    let visible = ui.is_rect_visible(row_r);
                    for (k, e) in chunk.iter().enumerate() {
                        let r = Rect::from_min_size(pos2(row_r.left() + k as f32 * cell, row_r.top()), vec2(cell, cell)).shrink(3.0);
                        let resp = ui.interact(r, ui.scope_id().with(("emoji", idx)), Sense::click());
                        if idx == selected {
                            ui.painter().rect_filled(r, 10, p.knob);
                            if scroll {
                                ui.scroll_to_rect(r, None);
                            }
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 10, p.chip);
                        }
                        if visible {
                            let img = Img::Emoji(e.as_str().to_string());
                            let er = Rect::from_center_size(r.center(), vec2(cell * 0.5, cell * 0.5));
                            if !icons.paint(ui.painter(), &img, er, p.ink) {
                                ui.painter().text(r.center(), Align2::CENTER_CENTER, e.as_str(), FontId::proportional(cell * 0.4), p.ink);
                            }
                        }
                        if resp.clicked() {
                            clicked = Some(idx);
                        }
                        idx += 1;
                    }
                }
            }
        });
        if let Some(c) = clicked {
            self.page_mut().selected = c;
            let (id, acts) = self.current_actions();
            if let Some(a) = acts.into_iter().next() {
                let now = ui.input(|i| i.time);
                self.run_action(&id, a, now);
            }
        }
    }

    fn color_grid(&mut self, ui: &mut Ui) {
        let p = pal();
        let cells = self.color_cells();
        let selected = self.page().selected;
        let scroll = std::mem::take(&mut self.scroll_to_selected);
        let fmt = self.settings.color_format;
        let mut clicked = None;
        let icons = &mut self.icons;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            section_title(ui, &format!("Saved colors · {}", cells.len() - 1));
            let w = ui.available_width();
            let cw = (w / COLOR_COLS as f32).floor();
            let ch = cw + 22.0;
            for (row_i, chunk) in cells.chunks(COLOR_COLS).enumerate() {
                let (row_r, _) = ui.allocate_exact_size(vec2(w, ch), Sense::hover());
                for (k, cell) in chunk.iter().enumerate() {
                    let idx = row_i * COLOR_COLS + k;
                    let r = Rect::from_min_size(pos2(row_r.left() + k as f32 * cw, row_r.top()), vec2(cw, ch)).shrink(4.0);
                    let resp = ui.interact(r, ui.scope_id().with(("color", idx)), Sense::click());
                    let sw = Rect::from_min_size(r.min + vec2(6.0, 6.0), vec2(r.width() - 12.0, r.width() - 12.0));
                    if idx == selected {
                        ui.painter().rect_filled(r, 12, p.knob);
                        if scroll {
                            ui.scroll_to_rect(r, None);
                        }
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, 12, p.chip);
                    }
                    match cell {
                        None => {
                            ui.painter().rect_stroke(sw, 10, Stroke::new(1.0, p.line), StrokeKind::Inside);
                            icons.paint(ui.painter(), &Img::Symbol("eyedropper"), Rect::from_center_size(sw.center(), vec2(24.0, 24.0)), p.ink);
                            ui.painter().text(pos2(r.center().x, sw.bottom() + 12.0), Align2::CENTER_CENTER, "Pick Color", FontId::proportional(11.0), p.muted);
                        }
                        Some((_, c)) => {
                            let [rr, gg, bb, aa] = c.u8();
                            ui.painter().rect_filled(sw, 10, Color32::from_rgba_unmultiplied(rr, gg, bb, aa));
                            ui.painter().rect_stroke(sw, 10, Stroke::new(1.0, p.line), StrokeKind::Inside);
                            let label = c.format(fmt);
                            ui.painter().text(pos2(r.center().x, sw.bottom() + 12.0), Align2::CENTER_CENTER, label, FontId::proportional(11.0), if idx == selected { p.bright } else { p.muted });
                        }
                    }
                    if resp.clicked() {
                        clicked = Some(idx);
                    }
                }
            }
        });
        if let Some(c) = clicked {
            self.page_mut().selected = c;
            let (id, acts) = self.current_actions();
            if let Some(a) = acts.into_iter().next() {
                let now = ui.input(|i| i.time);
                self.run_action(&id, a, now);
            }
        }
    }

    fn output(&mut self, ui: &mut Ui) {
        let p = pal();
        let View::Output { text: out, ok, .. } = &self.page().view else { return };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            match (out, ok) {
                (None, _) => theme::pill(ui, theme::Mark::Off, "Running…"),
                (Some(_), true) => theme::pill(ui, theme::Mark::On, "Finished"),
                (Some(_), false) => theme::pill(ui, theme::Mark::Problem, "Failed"),
            };
        });
        ui.add_space(8.0);
        if let Some(t) = out {
            let t = if t.trim().is_empty() { "(no output)".to_string() } else { t.clone() };
            ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                Frame::new().inner_margin(Margin::symmetric(10, 4)).show(ui, |ui| {
                    ui.add(egui::Label::new(mono(t, 12.5).color(p.ink)).wrap().selectable(true));
                });
            });
        }
    }

    fn form_body(&mut self, ui: &mut Ui) {
        let p = pal();
        let focus_now = std::mem::take(&mut self.focus_query);
        let page_id = Id::unique(("form", self.stack.len()));
        let View::Form(f) = &mut self.stack.last_mut().expect("page").view else { return };
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            Frame::new().inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let n = f.fields.len();
                for i in 0..n {
                    let (label, value, hint, multi) = &mut f.fields[i];
                    ui.label(theme::eyebrow(label));
                    let id = page_id.with(i);
                    let resp = Frame::new().fill(p.raised).stroke(Stroke::new(1.0, if f.focus == i { p.focus } else { p.line })).corner_radius(8).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
                        let te = if *multi { TextEdit::multiline(value).desired_rows(6) } else { TextEdit::singleline(value) };
                        ui.add(te.id(id).frame(Frame::NONE).font(FontId::proportional(13.0)).desired_width(f32::INFINITY).hint_text(text(*hint, 13.0, REGULAR).color(p.dim)))
                    }).inner;
                    if resp.gained_focus() {
                        f.focus = i;
                    }
                    if focus_now && f.focus == i {
                        resp.request_focus();
                    }
                    ui.add_space(6.0);
                }
                if f.kind == super::FormKind::Extension {
                    ui.label(theme::eyebrow("Mode"));
                    ui.horizontal(|ui| {
                        for (k, (_, label)) in EXTENSION_MODES.iter().enumerate() {
                            if theme::chip(ui, label, f.choice == k).clicked() {
                                f.choice = k;
                            }
                        }
                    });
                    let explain = match f.choice {
                        0 => "Shows what the script prints, right here.",
                        1 => "Gets the search text as $1 and prints JSON items to search through.",
                        2 => "Closes Glint and shows the last line it prints.",
                        _ => "Runs quietly in the background.",
                    };
                    ui.label(theme::caption(explain));
                    ui.add_space(4.0);
                    ui.label(theme::caption(format!("Saved to {} as a bash script.", crate::paths::tilde(&crate::paths::extensions_dir()))));
                }
                if let Some(e) = &f.error {
                    ui.add_space(6.0);
                    theme::callout(ui, "Can't save yet", e, None);
                }
            });
        });
    }

    fn action_bar(&mut self, ui: &mut Ui, now: f64) {
        let p = pal();
        let r = ui.max_rect();
        ui.painter().hline((r.left() - 14.0)..=(r.right() + 14.0), r.top() + 0.5, Stroke::new(1.0, p.line));
        let (id, acts) = self.current_actions();
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let toast = self.toast.clone().filter(|(_, at, _)| now - at <= TOAST_SECS);
            if let Some((msg, _, ok)) = toast {
                let (mr, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                theme::paint_mark(ui, mr.center(), if ok { theme::Mark::On } else { theme::Mark::Problem });
                ui.label(text(msg, 12.0, REGULAR).color(p.ink));
            } else {
                let (mr, _) = ui.allocate_exact_size(vec2(18.0, 15.0), Sense::hover());
                art::mark(ui.painter(), mr, &p);
                let label = if self.stack.len() > 1 { self.page().view.title() } else { crate::paths::APP_NAME.to_string() };
                ui.label(text(label, 12.0, REGULAR).color(p.muted));
                if let super::updates::Status::Available(r) = &self.update
                    && theme::link(ui, &format!("Update to {}", r.version)).clicked()
                {
                    self.install_update();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if acts.len() > 1 || matches!(self.page().view, View::Root) {
                    keycap(ui, "K");
                    keycap(ui, "⌘");
                    let resp = ui.add(egui::Label::new(text("Actions", 12.0, REGULAR).color(p.muted)).sense(Sense::click()));
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.actions = if self.actions.is_some() { None } else { Some(super::ActionsPanel { query: String::new(), selected: 0 }) };
                    }
                    ui.add_space(6.0);
                    let (sep, _) = ui.allocate_exact_size(vec2(1.0, 16.0), Sense::hover());
                    ui.painter().vline(sep.center().x, sep.y_range(), Stroke::new(1.0, p.line));
                    ui.add_space(6.0);
                }
                if let Some(a) = acts.first().cloned() {
                    keycap(ui, "↵");
                    let resp = ui.add(egui::Label::new(text(&a.title, 12.0, MEDIUM).color(p.ink)).sense(Sense::click()));
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        self.run_action(&id, a, now);
                    }
                }
            });
        });
    }

    fn actions_panel(&mut self, ctx: &egui::Context, now: f64) {
        let p = pal();
        let (id, acts) = self.current_actions();
        let screen = ctx.content_rect();
        let w = 330.0;
        let q = self.actions.as_ref().map(|a| a.query.to_lowercase()).unwrap_or_default();
        let shown: Vec<Action> = acts.into_iter().filter(|a| a.title.to_lowercase().contains(&q)).collect();
        let h = (shown.len() as f32 * 34.0 + 60.0).min(screen.height() - 90.0);
        let pos = pos2(screen.right() - w - 10.0, screen.bottom() - 46.0 - h);
        let mut run = None;
        let title = self.selected_item().map(|i| i.title.clone()).unwrap_or_else(|| self.page().view.title());
        let icons = &mut self.icons;
        let Some(panel) = &mut self.actions else { return };
        egui::Area::new(Id::unique("actions")).order(Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_rgb(0x16, 0x16, 0x17))
                .stroke(Stroke::new(1.0, theme::mix(p.line, Color32::WHITE, 0.08)))
                .corner_radius(12)
                .inner_margin(Margin::same(6))
                .shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(150) })
                .show(ui, |ui| {
                ui.set_width(w - 12.0);
                ui.set_max_height(h);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(theme::eyebrow(&title)).truncate());
                });
                ui.add_space(4.0);
                ScrollArea::vertical().max_height(h - 56.0).show(ui, |ui| {
                    for (i, a) in shown.iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                        if i == panel.selected {
                            ui.painter().rect_filled(r, 7, p.knob);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(r, 7, p.chip);
                        }
                        let ir = Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(15.0, 15.0));
                        icons.paint(ui.painter(), &Img::Symbol(a.symbol), ir, p.muted);
                        let color = if a.run_is_destructive() { p.danger } else { p.ink };
                        ui.painter().text(pos2(r.left() + 36.0, r.center().y), Align2::LEFT_CENTER, &a.title, FontId::proportional(12.5), color);
                        let caps = if i == 0 { vec!["↵".to_string()] } else { a.shortcut.map(|s| s.caps()).unwrap_or_default() };
                        let mut x = r.right() - 8.0;
                        for c in caps.iter().rev() {
                            let g = ui.painter().layout_no_wrap(c.clone(), FontId::monospace(11.0), p.muted);
                            let cw = (g.size().x + 10.0).max(20.0);
                            let cr = Rect::from_min_size(pos2(x - cw, r.center().y - 10.0), vec2(cw, 20.0));
                            ui.painter().rect_filled(cr, 5, p.chip);
                            ui.painter().galley(cr.center() - g.size() / 2.0, g, p.muted);
                            x -= cw + 3.0;
                        }
                        if resp.clicked() {
                            run = Some(a.clone());
                        }
                        if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            panel.selected = i;
                        }
                    }
                });
                ui.add_space(4.0);
                theme::hairline(ui);
                ui.add_space(6.0);
                let resp = ui.add(TextEdit::singleline(&mut panel.query).id(Id::unique("actions-q")).frame(Frame::NONE).font(FontId::proportional(12.5)).margin(Margin::symmetric(8, 4)).desired_width(f32::INFINITY).hint_text(text("Search for actions…", 12.5, REGULAR).color(p.dim)));
                resp.request_focus();
                if resp.changed() {
                    panel.selected = 0;
                }
            });
        });
        if let Some(a) = run {
            self.run_action(&id, a, now);
        }
    }

    fn confirm_card(&mut self, ctx: &egui::Context, c: Cmd, now: f64) {
        let p = pal();
        let def = commands::def(c);
        let screen = ctx.content_rect();
        egui::Area::new(Id::unique("dim")).order(Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, 0, Color32::from_black_alpha(140));
        });
        let size = vec2(340.0, 150.0);
        let mut decided = None;
        egui::Area::new(Id::unique("confirm")).order(Order::Foreground).fixed_pos(screen.center() - size / 2.0).show(ctx, |ui| {
            Frame::new().fill(Color32::from_rgb(0x16, 0x16, 0x17)).stroke(Stroke::new(1.0, p.line)).corner_radius(14).inner_margin(Margin::same(20)).show(ui, |ui| {
                ui.set_width(size.x - 40.0);
                ui.label(text(def.title, 15.0, SEMIBOLD).color(p.bright));
                ui.add_space(4.0);
                theme::para(ui, theme::caption(def.confirm.unwrap_or("Are you sure?")));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Cancel  esc", theme::Kind::Secondary).clicked() {
                        decided = Some(false);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::button(ui, &format!("{}  ↵", def.title), theme::Kind::Primary).clicked() {
                            decided = Some(true);
                        }
                    });
                });
            });
        });
        match decided {
            Some(true) => self.run_cmd(c, now),
            Some(false) => self.confirm = None,
            None => {}
        }
    }
}

impl Action {
    fn run_is_destructive(&self) -> bool {
        matches!(self.run, Run::Trash(_) | Run::DeleteClip(_) | Run::DeleteColor(_) | Run::DeleteQuicklink(_) | Run::DeleteSnippet(_) | Run::QuitApp(_, true))
            || matches!(self.run, Run::Cmd(Cmd::ClearClipboardHistory))
    }
}

fn section_title(ui: &mut Ui, title: &str) {
    let p = pal();
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    ui.painter().text(pos2(r.left() + 10.0, r.bottom() - 9.0), Align2::LEFT_CENTER, title.to_lowercase(), FontId::proportional(11.0), p.dim);
}

fn paint_icon(ui: &Ui, icons: &mut Icons, icon: &Icon, r: Rect, selected: bool) {
    let p = pal();
    match icon {
        Icon::Img(img @ Img::Symbol(_)) => {
            ui.painter().rect_filled(r, 6, if selected { theme::mix(p.knob, Color32::WHITE, 0.08) } else { p.knob });
            icons.paint(ui.painter(), img, r.shrink(4.5), p.ink);
        }
        Icon::Img(img) => {
            if !icons.paint(ui.painter(), img, r.expand(1.0), p.ink) {
                ui.painter().rect_filled(r.shrink(1.0), 6, p.chip);
            }
        }
        Icon::Swatch(c) => {
            let [rr, gg, bb, aa] = c.u8();
            ui.painter().rect_filled(r, 6, Color32::from_rgba_unmultiplied(rr, gg, bb, aa));
            ui.painter().rect_stroke(r, 6, Stroke::new(1.0, p.line), StrokeKind::Inside);
        }
        Icon::Mark => art::mark(ui.painter(), r.shrink2(vec2(2.0, 4.0)), &p),
    }
}

/// One result row (or the calculator / color cards).
fn row(ui: &mut Ui, icons: &mut Icons, it: &Item, selected: bool, hotkey: Option<&str>) -> Response {
    let p = pal();
    let h = match it.kind {
        Kind::Calc { .. } => 104.0,
        Kind::Color(_) => 64.0,
        Kind::Row => 42.0,
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let painter = ui.painter();
    let bg = rect.shrink2(vec2(0.0, 1.0));
    if selected {
        painter.rect_filled(bg, 9, p.knob);
    } else if resp.hovered() {
        painter.rect_filled(bg, 9, p.chip);
    }
    match &it.kind {
        Kind::Calc { input, answer } => {
            let card = bg.shrink(8.0);
            let half = card.width() / 2.0;
            let left = Rect::from_min_size(card.min, vec2(half, card.height()));
            let right = Rect::from_min_size(pos2(card.left() + half, card.top()), vec2(half, card.height()));
            let fit = |s: &str, max: f32, size: f32| -> f32 {
                let w = s.chars().count() as f32 * size * 0.6;
                if w > max { (size * max / w).max(11.0) } else { size }
            };
            painter.text(left.center() - vec2(0.0, 8.0), Align2::CENTER_CENTER, input, FontId::proportional(fit(input, half - 30.0, 17.0)), p.ink);
            painter.text(right.center() - vec2(0.0, 8.0), Align2::CENTER_CENTER, &answer.text, FontId::proportional(fit(&answer.text, half - 30.0, 24.0)), p.bright);
            painter.text(left.center_bottom() - vec2(0.0, 16.0), Align2::CENTER_CENTER, "Expression", FontId::proportional(10.5), p.dim);
            painter.text(right.center_bottom() - vec2(0.0, 16.0), Align2::CENTER_CENTER, answer.unit.unwrap_or("Answer"), FontId::proportional(10.5), p.dim);
            painter.vline(card.center().x, (card.top() + 10.0)..=(card.bottom() - 10.0), Stroke::new(1.0, p.line));
            let ar = Rect::from_center_size(card.center(), vec2(24.0, 24.0));
            painter.rect_filled(ar, 12, p.elevated);
            painter.rect_stroke(ar, 12, Stroke::new(1.0, p.line), StrokeKind::Inside);
            painter.text(ar.center(), Align2::CENTER_CENTER, "→", FontId::proportional(12.0), p.muted);
        }
        Kind::Color(c) => {
            let sw = Rect::from_min_size(pos2(bg.left() + 12.0, bg.center().y - 20.0), vec2(40.0, 40.0));
            let [rr, gg, bb, aa] = c.u8();
            painter.rect_filled(sw, 9, Color32::from_rgba_unmultiplied(rr, gg, bb, aa));
            painter.rect_stroke(sw, 9, Stroke::new(1.0, p.line), StrokeKind::Inside);
            painter.text(pos2(sw.right() + 14.0, bg.center().y - 9.0), Align2::LEFT_CENTER, &it.title, FontId::proportional(15.0), p.bright);
            let others = [ColorFormat::Rgb, ColorFormat::Hsl, ColorFormat::Oklch].map(|f| c.format(f)).join("   ");
            painter.text(pos2(sw.right() + 14.0, bg.center().y + 11.0), Align2::LEFT_CENTER, others, FontId::proportional(11.0), p.muted);
            painter.text(pos2(bg.right() - 14.0, bg.center().y), Align2::RIGHT_CENTER, "Color", FontId::proportional(11.0), p.dim);
        }
        Kind::Row => {
            let ir = Rect::from_center_size(pos2(bg.left() + 22.0, bg.center().y), vec2(22.0, 22.0));
            paint_icon(ui, icons, &it.icon, ir, selected);
            let painter = ui.painter();
            // Right side first, so the title knows how much room it has.
            let mut right_x = bg.right() - 12.0;
            if let Some(hk) = hotkey {
                for cap in hotkey_parts(hk).iter().rev() {
                    let g = painter.layout_no_wrap(cap.clone(), FontId::monospace(10.5), p.muted);
                    let cw = (g.size().x + 8.0).max(18.0);
                    let cr = Rect::from_min_size(pos2(right_x - cw, bg.center().y - 9.0), vec2(cw, 18.0));
                    painter.rect_filled(cr, 4, p.chip);
                    painter.galley(cr.center() - g.size() / 2.0, g, p.muted);
                    right_x -= cw + 3.0;
                }
                right_x -= 8.0;
            }
            if !it.accessory.is_empty() {
                let g = painter.layout_no_wrap(it.accessory.clone(), FontId::proportional(11.0), p.dim);
                painter.galley(pos2(right_x - g.size().x, bg.center().y - g.size().y / 2.0), g.clone(), p.dim);
                right_x -= g.size().x + 14.0;
            }
            let x = ir.right() + 12.0;
            let avail = (right_x - x).max(40.0);
            let title_color = if selected { p.bright } else { p.ink };
            let tg = painter.layout(it.title.clone(), FontId::proportional(13.0), title_color, f32::INFINITY);
            let tw = tg.size().x.min(avail);
            let clip = Rect::from_min_max(pos2(x, bg.top()), pos2(x + avail, bg.bottom()));
            let painter = painter.with_clip_rect(clip);
            painter.galley(pos2(x, bg.center().y - tg.size().y / 2.0), tg, title_color);
            if !it.subtitle.is_empty() && tw + 30.0 < avail {
                let sg = painter.layout_no_wrap(it.subtitle.lines().next().unwrap_or("").to_string(), FontId::proportional(11.5), p.muted);
                painter.galley(pos2(x + tw + 10.0, bg.center().y - sg.size().y / 2.0), sg, p.muted);
            }
        }
    }
    resp.on_hover_cursor(CursorIcon::Default)
}
