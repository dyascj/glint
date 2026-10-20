//! About content, shown in its own window and on the Settings page.

use eframe::egui::{self, Margin, Sense, Ui, vec2};

use super::theme::{self, Kind};
use super::{art, chrome, runtime, signal_launcher};
use crate::launcher::{Runtime, SIG_CHECK_UPDATES, SIG_INSTALL_UPDATE};
use crate::macos;
use crate::paths::{APP_NAME, REPO_URL};

#[derive(Default)]
pub struct About {
    rt: Option<Runtime>,
    next_poll: f64,
    checked_at: Option<f64>,
}

impl About {
    pub fn ui(&mut self, ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        if now >= self.next_poll {
            self.rt = Some(runtime());
            self.next_poll = now + 1.0;
        }
        ui.ctx().request_repaint_after_secs(1.0);
        let p = theme::pal();
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let (r, _) = ui.allocate_exact_size(vec2(36.0, 30.0), Sense::hover());
            art::mark(ui.painter(), r, &p);
            ui.add_space(4.0);
            ui.label(theme::text(APP_NAME, 30.0, theme::HEAVY).color(p.bright).extra_letter_spacing(-1.0));
            ui.label(theme::mono(format!("Version {}", env!("CARGO_PKG_VERSION")), 11.5).weak());
            ui.add_space(6.0);
            ui.label(theme::text("Everything, one keystroke away.", 14.0, theme::MEDIUM));
            ui.add_space(14.0);

            let update = self.rt.as_ref().and_then(|r| r.update_available.clone());
            let status = self.rt.as_ref().and_then(|r| r.update_status.clone()).unwrap_or_default();
            if let Some(v) = update {
                let installing = status == "installing";
                let label = if installing { "Updating…".to_string() } else { format!("Update to {v} and Restart") };
                ui.add_enabled_ui(!installing, |ui| {
                    if theme::button(ui, &label, Kind::Primary).clicked() {
                        signal_launcher(SIG_INSTALL_UPDATE);
                    }
                });
                if let Some(reason) = status.strip_prefix("failed:") {
                    theme::para(ui, theme::caption(format!("Update failed: {}", reason.trim())));
                }
            }
            theme::centered_row(ui, "about-buttons", |ui| {
                if theme::small_button(ui, "Check for Updates…", Kind::Secondary).clicked() {
                    if signal_launcher(SIG_CHECK_UPDATES) {
                        self.checked_at = Some(now);
                    } else {
                        macos::open(&format!("{REPO_URL}/releases/latest"));
                    }
                }
                if theme::small_button(ui, "Send feedback", Kind::Secondary).clicked() {
                    macos::open(&format!("{REPO_URL}/issues/new"));
                }
            });
            if let Some(t) = self.checked_at
                && self.rt.as_ref().is_some_and(|r| r.update_available.is_none())
            {
                ui.label(theme::caption(if now - t < 5.0 { "Checking for updates…" } else { "You're on the latest version." }));
            }
            ui.add_space(12.0);
            theme::centered_row(ui, "about-links", |ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if theme::link(ui, "github.com/dyascj/glint").clicked() {
                    macos::open(REPO_URL);
                }
                if theme::link(ui, "Made by dyascj").clicked() {
                    macos::open("https://github.com/dyascj");
                }
            });
            ui.add_space(14.0);
            ui.add(egui::Label::new(theme::text("A fast, native launcher for macOS, written in Rust.", 10.5, theme::REGULAR).weak()).wrap());
        });
    }
}

#[derive(Default)]
pub struct AboutWindow {
    about: About,
}

impl eframe::App for AboutWindow {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::GLASS
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        chrome(ui);
        egui::CentralPanel::default().frame(theme::glass(Margin::symmetric(28, 0))).show(ui, |ui| {
            ui.add_space(44.0);
            self.about.ui(ui);
        });
    }
}
