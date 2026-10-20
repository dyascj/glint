//! The menu bar item: Glint's mark as a template image, and its menu.

use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use super::Launcher;
use super::updates::Status;
use crate::commands::{self, Cmd};
use crate::paths::APP_NAME;
use crate::settings::hotkey_label;

const SCALE: f64 = 2.0;
/// Points; 22 pt is the status bar's height.
const W: f64 = 20.0;
const H: f64 = 22.0;

fn in_pill(px: f64, py: f64, (x, y, w, h): (f64, f64, f64, f64)) -> bool {
    let r = h / 2.0;
    let cx = px.clamp(x + r, x + w - r);
    let (dx, dy) = (px - cx, py - (y + r));
    dx * dx + dy * dy <= r * r
}

/// The mark at menu bar size, same proportions as the app icon: search bar
/// (outline), selected row (solid), a shorter row (outline), and the glint.
fn covered(px: f64, py: f64) -> bool {
    const STROKE: f64 = 1.1;
    let (h, top) = (13.0, 4.5);
    let r = 0.2235 * h;
    let w = (W - 1.0 - r) / 0.93;
    let rows = [(0.0, 0.28, 1.0, false), (0.37, 0.65, 0.72, true), (0.74, 1.0, 0.48, false)];
    for (y0, y1, frac, solid) in rows {
        let outer = (0.5, top + y0 * h, frac * w, (y1 - y0) * h);
        let inner = (outer.0 + STROKE, outer.1 + STROKE, outer.2 - 2.0 * STROKE, outer.3 - 2.0 * STROKE);
        if in_pill(px, py, outer) && (solid || !in_pill(px, py, inner)) {
            return true;
        }
    }
    // Astroid glint: |x|^(2/3) + |y|^(2/3) <= r^(2/3).
    let (dx, dy) = ((px - (0.5 + 0.93 * w)).abs(), (py - (top + 0.51 * h)).abs());
    dx.powf(2.0 / 3.0) + dy.powf(2.0 / 3.0) <= r.powf(2.0 / 3.0)
}

pub fn rgba() -> (Vec<u8>, u32, u32) {
    const SS: u32 = 4;
    let (w, h) = ((W * SCALE) as u32, (H * SCALE) as u32);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut hits = 0;
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (f64::from(x) + (f64::from(sx) + 0.5) / f64::from(SS)) / SCALE;
                    let py = (f64::from(y) + (f64::from(sy) + 0.5) / f64::from(SS)) / SCALE;
                    hits += u32::from(covered(px, py));
                }
            }
            out.extend_from_slice(&[0, 0, 0, (hits * 255 / (SS * SS)) as u8]);
        }
    }
    (out, w, h)
}

pub fn build(menu: Menu) -> Option<TrayIcon> {
    let (data, w, h) = rgba();
    let icon = Icon::from_rgba(data, w, h).ok()?;
    TrayIconBuilder::new().with_icon_templated(icon).with_tooltip(APP_NAME).with_menu(Box::new(menu)).build().ok()
}

fn item(id: &str, text: &str, enabled: bool, accel: Option<(Modifiers, Code)>) -> MenuItem {
    MenuItem::with_id(id, text, enabled, accel.map(|(m, c)| Accelerator::new(m, c)))
}

pub fn menu(l: &Launcher) -> Menu {
    let menu = Menu::new();
    let add = |i: &dyn IsMenuItem| {
        let _ = menu.append(i);
    };
    let sep = PredefinedMenuItem::separator;
    add(&item("open", &format!("Open {APP_NAME}    {}", hotkey_label(&l.settings.hotkey)), true, None));
    add(&sep());
    for c in [Cmd::ClipboardHistory, Cmd::PickColor, Cmd::SearchFiles, Cmd::SearchEmoji] {
        let d = commands::def(c);
        add(&item(&d.id(), d.title, true, None));
    }
    add(&sep());
    add(&item("settings", "Settings…", true, Some((Modifiers::META, Code::Comma))));
    add(&item(&commands::def(Cmd::OpenExtensionsFolder).id(), "Open Extensions Folder", true, None));
    add(&item("onboarding", "Show Onboarding", true, None));
    add(&item("feedback", "Report a Bug or Send Feedback…", true, None));
    match &l.update {
        Status::Available(r) => add(&item("update", &format!("Update to {} and Restart", r.version), true, None)),
        Status::Installing(_) => add(&item("", "Updating…", false, None)),
        Status::Failed(..) => add(&item("update", "Update failed, try again", true, None)),
        Status::Idle => {}
    }
    add(&item("check", "Check for Updates…", true, None));
    add(&sep());
    add(&item("about", &format!("About {APP_NAME}"), true, None));
    add(&item("quit", &format!("Quit {APP_NAME}"), true, Some((Modifiers::META, Code::KeyQ))));
    menu
}

#[cfg(test)]
mod tests {
    #[test]
    fn glyph_has_rows_and_a_glint() {
        let (px, w, h) = super::rgba();
        assert_eq!((w, h), (40, 44));
        let alpha = |x: u32, y: u32| px[((y * w + x) * 4 + 3) as usize];
        if std::env::var_os("GLINT_PRINT_GLYPH").is_some() {
            for y in 0..h {
                let line: String = (0..w).map(|x| match alpha(x, y) { 0..=40 => ' ', 41..=160 => '+', _ => '#' }).collect();
                println!("{line}");
            }
        }
        let inked = px.chunks(4).filter(|p| p[3] > 128).count();
        assert!(inked > 150 && inked < 900, "{inked}");
    }
}
