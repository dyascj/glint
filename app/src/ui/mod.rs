//! Window processes: `Glint ui <settings|onboarding|about> [section]`.
//! One eframe window per process; the process exits when it closes.

mod about;
pub mod art;
mod hotkey;
mod onboarding;
mod settings;
pub mod theme;

use eframe::egui;
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};

use crate::launcher::{RUNTIME, Runtime};
use crate::paths::APP_NAME;

pub fn run(args: &[String]) {
    let kind = args.first().map_or("settings", String::as_str);
    let section = args.get(1).cloned();
    let (title, size, resizable) = match kind {
        "onboarding" => (format!("Welcome to {APP_NAME}"), [720.0, 560.0], false),
        "about" => (format!("About {APP_NAME}"), [400.0, 360.0], false),
        _ => ("Settings".to_string(), [800.0, 580.0], true),
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(&title)
        .with_inner_size(size)
        .with_resizable(resizable)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_transparent(true)
        .with_maximize_button(resizable);
    if resizable {
        viewport = viewport.with_min_inner_size([700.0, 480.0]);
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        event_loop_builder: Some(Box::new(|b| {
            b.with_activation_policy(ActivationPolicy::Accessory);
        })),
        ..Default::default()
    };
    let kind = kind.to_string();
    let result = eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let app: Box<dyn eframe::App> = match kind.as_str() {
                "onboarding" => Box::new(onboarding::Onboarding::new(&cc.egui_ctx)),
                "about" => Box::new(about::AboutWindow::default()),
                _ => Box::new(settings::SettingsApp::new(&cc.egui_ctx, section.as_deref())),
            };
            Ok(app)
        }),
    );
    if let Err(e) = result {
        eprintln!("{APP_NAME}: window failed: {e}");
    }
}

/// Per-frame chrome shared by every window: bring to front once, glass, ⌘W / ⌘Q.
fn chrome(ui: &egui::Ui) {
    let ctx = ui.ctx();
    let frame = ctx.cumulative_frame_nr();
    if frame < 3
        && let Some(mtm) = MainThreadMarker::new()
    {
        let app = NSApplication::sharedApplication(mtm);
        if frame == 0 && !(cfg!(debug_assertions) && std::env::var_os("GLINT_UI_SNAPSHOT").is_some()) {
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
        for window in app.windows().iter() {
            window.setOpaque(false);
            window.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
            crate::launcher::blur(&window, 23);
        }
    }
    if ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::W) || i.key_pressed(egui::Key::Q))) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    #[cfg(debug_assertions)]
    snapshot(ctx);
}

pub fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// The launcher's published state (pid, onboarding progress, updates).
pub fn runtime() -> Runtime {
    crate::prefs::load(RUNTIME)
}

/// Sends `sig` to the running launcher; false when it isn't running.
pub fn signal_launcher(sig: i32) -> bool {
    let pid = runtime().pid;
    // SAFETY: plain kill(2) to a pid the launcher published.
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0 && unsafe { libc::kill(pid, sig) } == 0
}

/// Shows the launcher, starting it if needed. This window is the active app,
/// so it hands activation over (macOS won't let a background app take focus).
pub fn open_launcher() {
    let pid = runtime().pid;
    if signal_launcher(crate::launcher::SIG_SHOW) {
        if let Some(app) = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            #[allow(deprecated)]
            app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps);
        }
    } else if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
}

/// Dev builds only: `GLINT_UI_SNAPSHOT=out.bmp` renders a few frames, writes
/// the window's own framebuffer composited over a stand-in blurred wallpaper,
/// and exits. Needs no Screen Recording permission (the real compositor blur
/// is not in the framebuffer, so the wallpaper is simulated).
#[cfg(debug_assertions)]
pub fn snapshot(ctx: &egui::Context) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    static WRITTEN: AtomicBool = AtomicBool::new(false);
    static FRAMES: AtomicU64 = AtomicU64::new(0);
    let Ok(path) = std::env::var("GLINT_UI_SNAPSHOT") else { return };
    if WRITTEN.load(Ordering::Relaxed) {
        std::process::exit(0);
    }
    ctx.request_repaint();
    let frames: u64 = std::env::var("GLINT_UI_FRAMES").ok().and_then(|f| f.parse().ok()).unwrap_or(45);
    if FRAMES.fetch_add(1, Ordering::Relaxed) != frames {
        return;
    }
    let ctx2 = ctx.clone();
    ctx.request_screenshot(move |img| {
        let [w, h] = img.size;
        let mut out = Vec::with_capacity(54 + w * h * 4);
        out.extend(b"BM");
        out.extend(((54 + w * h * 4) as u32).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend((w as i32).to_le_bytes());
        out.extend((h as i32).to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend([0u8; 24]);
        for y in (0..h).rev() {
            for (x, px) in img.pixels[y * w..(y + 1) * w].iter().enumerate() {
                // soft wallpaper stand-in: diagonal colour bands
                let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
                let bg = [
                    40.0 + 160.0 * (fx * 3.1).sin().abs(),
                    60.0 + 120.0 * ((fx + fy) * 2.3).cos().abs(),
                    90.0 + 150.0 * (fy * 2.7).sin().abs(),
                ];
                let a = 1.0 - px.a() as f32 / 255.0;
                let ch = |c: u8, b: f32| (c as f32 + b * a).min(255.0) as u8;
                out.extend([ch(px.b(), bg[2]), ch(px.g(), bg[1]), ch(px.r(), bg[0]), 255]);
            }
        }
        let _ = std::fs::write(&path, out);
        WRITTEN.store(true, Ordering::Relaxed);
        ctx2.request_repaint();
    });
}
