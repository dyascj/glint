//! Hotkey recorder: click, press a combination, done. Esc cancels, ⌫ clears.

use eframe::egui::{self, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::theme::{REGULAR, galley, pal, text};
use crate::settings::{hotkey_from, hotkey_parts};

/// egui key name -> the global-hotkey name for it.
fn key_name(k: egui::Key) -> String {
    match k {
        egui::Key::Equals => "Equal".into(),
        egui::Key::OpenBracket => "BracketLeft".into(),
        egui::Key::CloseBracket => "BracketRight".into(),
        egui::Key::Backtick => "Backquote".into(),
        other => other.name().to_string(),
    }
}

/// Shows `value` as keycaps; while recording, the next combination replaces
/// it. `clearable` lets ⌫ remove it. Returns a response marked changed.
pub fn recorder(ui: &mut Ui, salt: impl std::hash::Hash + std::fmt::Debug, value: &mut Option<String>, clearable: bool) -> Response {
    let p = pal();
    let id = ui.scope_id().with(("hotkey", salt));
    let recording: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    let caps: Vec<String> = if recording { vec![] } else { value.as_deref().map(hotkey_parts).unwrap_or_default() };
    let label = if recording {
        "Press keys…".to_string()
    } else if caps.is_empty() {
        "Record Hotkey".to_string()
    } else {
        String::new()
    };
    let gs: Vec<_> = caps.iter().map(|c| galley(ui, text(c, 12.0, REGULAR), f32::INFINITY)).collect();
    let lg = galley(ui, text(&label, 12.0, REGULAR), f32::INFINITY);
    let caps_w: f32 = gs.iter().map(|g| (g.size().x + 12.0).max(22.0) + 4.0).sum();
    let w = (caps_w + lg.size().x + 20.0).max(110.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    let painter = ui.painter();
    let stroke = if recording { Stroke::new(1.0, p.accent) } else { Stroke::new(1.0, p.line) };
    painter.rect_filled(rect, 8, if resp.hovered() || recording { p.chip } else { egui::Color32::TRANSPARENT });
    painter.rect_stroke(rect, 8, stroke, StrokeKind::Inside);
    let mut x = rect.left() + 10.0;
    for g in gs {
        let cw = (g.size().x + 12.0).max(22.0);
        let cr = Rect::from_min_size(pos2(x, rect.center().y - 10.0), vec2(cw, 20.0));
        painter.rect_filled(cr, 5, p.knob);
        painter.galley(cr.center() - g.size() / 2.0, g, p.ink);
        x += cw + 4.0;
    }
    if !label.is_empty() {
        painter.galley(pos2(x, rect.center().y - lg.size().y / 2.0), lg, if recording { p.accent } else { p.muted });
    }
    if recording {
        ui.ctx().request_repaint();
    }
    if resp.clicked() {
        ui.data_mut(|d| d.insert_temp(id, !recording));
    }
    if recording {
        let pressed = ui.input_mut(|i| {
            let pos = i.events.iter().position(|e| matches!(e, egui::Event::Key { pressed: true, .. }));
            pos.map(|p| i.events.remove(p))
        });
        if let Some(egui::Event::Key { key, modifiers: m, .. }) = pressed {
            if key == egui::Key::Escape && !m.any() {
                ui.data_mut(|d| d.insert_temp(id, false));
            } else if key == egui::Key::Backspace && !m.any() {
                if clearable && value.is_some() {
                    *value = None;
                    resp.mark_changed();
                }
                ui.data_mut(|d| d.insert_temp(id, false));
            } else if let Some(hk) = hotkey_from(&key_name(key), m.ctrl, m.alt, m.shift, m.mac_cmd || m.command) {
                *value = Some(hk);
                resp.mark_changed();
                ui.data_mut(|d| d.insert_temp(id, false));
            }
        }
        if ui.input(|i| i.pointer.any_click()) && !resp.clicked() {
            ui.data_mut(|d| d.insert_temp(id, false));
        }
    }

    resp.on_hover_text(if recording { "Press a combination with ⌘, ⌥ or ⌃. Esc cancels." } else { "Click to record" })
}
