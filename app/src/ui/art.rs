//! Drawn illustrations: the Glint mark and a little launcher that types.

use eframe::egui::{Color32, FontFamily, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

use super::theme::Pal;

/// Glint's mark: a search bar, the selected result (solid) and one more,
/// with a glint beside the selection. Slat's slats, turned on their side.
pub fn mark(painter: &Painter, rect: Rect, p: &Pal) {
    mark_colored(painter, rect, p.ink, p.accent);
}

pub fn mark_colored(painter: &Painter, rect: Rect, ink: Color32, glint: Color32) {
    // Same geometry as scripts/make-icon.py: bars `w` wide, glint at 0.93w with radius 0.22h.
    let h = rect.height();
    let r = 0.2235 * h;
    let w = (rect.width() - r) / 0.93;
    let bar = |y0: f32, y1: f32, frac: f32| Rect::from_min_max(pos2(rect.left(), rect.top() + y0 * h), pos2(rect.left() + frac * w, rect.top() + y1 * h));
    let stroke = Stroke::new((h * 0.05).clamp(1.0, 3.0), ink);
    let r1 = bar(0.0, 0.28, 1.0);
    painter.rect_stroke(r1, r1.height() / 2.0, stroke, StrokeKind::Inside);
    let r2 = bar(0.37, 0.65, 0.72);
    painter.rect_filled(r2, r2.height() / 2.0, ink);
    let r3 = bar(0.74, 1.0, 0.48);
    painter.rect_stroke(r3, r3.height() / 2.0, stroke, StrokeKind::Inside);
    star(painter, pos2(rect.left() + 0.93 * w, rect.top() + 0.51 * h), r, glint);
}

/// Four-point glint with curved sides (an astroid), as a triangle fan.
pub fn star(painter: &Painter, c: Pos2, r: f32, color: Color32) {
    const N: usize = 64;
    let mut mesh = eframe::egui::Mesh::default();
    mesh.colored_vertex(c, color);
    for i in 0..=N {
        let t = std::f32::consts::TAU * i as f32 / N as f32;
        let (s, co) = t.sin_cos();
        mesh.colored_vertex(pos2(c.x + r * co.abs().powi(3).copysign(co), c.y + r * s.abs().powi(3).copysign(s)), color);
        if i > 0 {
            mesh.add_triangle(0, i as u32, i as u32 + 1);
        }
    }
    painter.add(mesh);
}

pub fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One scripted search the mock launcher types out.
struct Scene {
    query: &'static str,
    rows: &'static [(&'static str, &'static str, Glyph)],
    calc: Option<(&'static str, &'static str)>,
}

#[derive(Clone, Copy)]
enum Glyph {
    App(u32),
    Doc,
    Cmd,
    Swatch(u32),
}

const SCENES: [Scene; 4] = [
    Scene { query: "saf", rows: &[("Safari", "Application", Glyph::App(0x5aa9f7)), ("Saved Colors", "Command", Glyph::Cmd), ("Search Files", "Command", Glyph::Cmd)], calc: None },
    Scene { query: "24px to rem", rows: &[], calc: Some(("24px to rem", "1.5 rem")) },
    Scene { query: "#f3d694", rows: &[("#F3D694", "Color", Glyph::Swatch(0xf3d694)), ("Pick Color", "Command", Glyph::Cmd)], calc: None },
    Scene { query: "invoice", rows: &[("invoice-oct.pdf", "~/Documents", Glyph::Doc), ("invoices", "~/Desktop", Glyph::Doc), ("Invoice Template.pages", "~/Documents", Glyph::Doc)], calc: None },
];

/// A small launcher typing a few searches in a loop (`t` in seconds).
pub fn launcher_mock(painter: &Painter, rect: Rect, p: &Pal, t: f64, only: Option<usize>) {
    const SCENE_SECS: f64 = 3.2;
    let i = only.unwrap_or(((t / SCENE_SECS) as usize) % SCENES.len());
    let local = if only.is_some() { 2.0 } else { (t % SCENE_SECS) as f32 };
    let scene = &SCENES[i];
    let typed = ((local / 0.09) as usize).min(scene.query.chars().count());
    let revealed = smooth((local - scene.query.chars().count() as f32 * 0.09 - 0.1) / 0.3);
    let fade = 1.0 - smooth((local - (SCENE_SECS as f32 - 0.35)) / 0.3);

    painter.rect_filled(rect, 12, Color32::from_white_alpha(0x07));
    painter.rect_stroke(rect, 12, Stroke::new(1.0, p.line), StrokeKind::Inside);
    let bar_h = 40.0;
    let mono = |s: f32| FontId::new(s, FontFamily::Proportional);
    // search bar
    let q: String = scene.query.chars().take(typed).collect();
    let text_pos = pos2(rect.left() + 16.0, rect.top() + bar_h / 2.0);
    let g = painter.layout_no_wrap(q.clone(), mono(14.0), p.bright);
    let gw = g.size().x;
    painter.galley(text_pos - vec2(0.0, g.size().y / 2.0), g, p.bright);
    let blink = (t * 2.0).fract() < 0.6;
    if blink {
        painter.vline(text_pos.x + gw + 1.5, (text_pos.y - 8.0)..=(text_pos.y + 8.0), Stroke::new(1.5, p.accent));
    }
    painter.hline((rect.left() + 1.0)..=(rect.right() - 1.0), rect.top() + bar_h, Stroke::new(1.0, p.line));
    let alpha = revealed * fade;
    if alpha <= 0.0 {
        return;
    }
    let body = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top() + bar_h + 8.0), rect.max - vec2(8.0, 8.0));
    if let Some((input, out)) = scene.calc {
        let card = Rect::from_min_size(body.min, vec2(body.width(), 74.0_f32.min(body.height())));
        painter.rect_filled(card, 8, Color32::from_white_alpha((0x10 as f32 * alpha) as u8));
        let half = card.width() / 2.0;
        for (k, (s, size, c)) in [(input, 13.0, p.muted), (out, 20.0, p.bright)].into_iter().enumerate() {
            let cx = card.left() + half * (k as f32 + 0.5);
            let g = painter.layout_no_wrap(s.to_string(), mono(size), c.gamma_multiply(alpha));
            painter.galley(pos2(cx - g.size().x / 2.0, card.center().y - g.size().y / 2.0), g, c);
        }
        painter.text(card.center(), eframe::egui::Align2::CENTER_CENTER, "→", mono(14.0), p.dim.gamma_multiply(alpha));
        return;
    }
    let row_h = 30.0;
    for (k, (title, acc, glyph)) in scene.rows.iter().enumerate() {
        let a = alpha * smooth(revealed * 3.0 - k as f32 * 0.5);
        let r = Rect::from_min_size(pos2(body.left(), body.top() + k as f32 * (row_h + 2.0)), vec2(body.width(), row_h));
        if r.bottom() > rect.bottom() - 4.0 {
            break;
        }
        if k == 0 {
            painter.rect_filled(r, 7, Color32::from_white_alpha((0x1a as f32 * a) as u8));
        }
        let icon = Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(16.0, 16.0));
        match glyph {
            Glyph::App(c) | Glyph::Swatch(c) => {
                let col = Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, *c as u8);
                painter.rect_filled(icon, if matches!(glyph, Glyph::App(_)) { 4.0 } else { 8.0 }, col.gamma_multiply(a));
            }
            Glyph::Doc => {
                painter.rect_stroke(icon.shrink2(vec2(2.5, 0.5)), 2, Stroke::new(1.2, p.muted.gamma_multiply(a)), StrokeKind::Inside);
            }
            Glyph::Cmd => {
                painter.rect_filled(icon, 4, p.knob.gamma_multiply(a));
                star(painter, icon.center(), 5.0, p.ink.gamma_multiply(a));
            }
        }
        let g = painter.layout_no_wrap(title.to_string(), mono(12.0), p.ink.gamma_multiply(a));
        painter.galley(pos2(icon.right() + 10.0, r.center().y - g.size().y / 2.0), g, p.ink);
        let g = painter.layout_no_wrap(acc.to_string(), mono(10.5), p.dim.gamma_multiply(a));
        painter.galley(pos2(r.right() - 10.0 - g.size().x, r.center().y - g.size().y / 2.0), g, p.dim);
    }
}
