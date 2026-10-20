//! The launcher window: a floating glass panel that appears over everything
//! on the screen with the mouse, and the small HUD that confirms actions
//! after it has gone ("Copied to Clipboard").

use std::cell::RefCell;
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSBox, NSBoxType, NSColor, NSEvent, NSFloatingWindowLevel, NSFont, NSFontWeightMedium, NSPanel,
    NSScreen, NSStatusWindowLevel, NSTextField, NSTitlePosition, NSView, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

pub const SIZE: [f32; 2] = [750.0, 474.0];

fn window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    // The egui window carries the app's name; the HUD and alerts don't.
    let name = NSString::from_str(crate::paths::APP_NAME);
    NSApplication::sharedApplication(mtm).windows().iter().find(|w| w.title() == name)
}

/// Glass, floating level, every Space and over full-screen apps.
pub fn configure(mtm: MainThreadMarker) {
    let Some(w) = window(mtm) else { return };
    w.setOpaque(false);
    w.setBackgroundColor(Some(&NSColor::clearColor()));
    w.setLevel(NSFloatingWindowLevel);
    w.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::IgnoresCycle,
    );
    w.setHidesOnDeactivate(false);
    // eframe shows the window after its first frame; keep that frame invisible.
    w.setAlphaValue(0.0);
    blur(&w, 30);
}

/// After eframe's first frame: take the window off screen, ready for show().
pub fn park(mtm: MainThreadMarker) {
    if let Some(w) = window(mtm) {
        w.orderOut(None);
        w.setAlphaValue(1.0);
    }
}

/// Ghostty-style backdrop blur (private CGS call, looked up at runtime).
pub fn blur(window: &NSWindow, radius: u32) {
    type MainConnection = unsafe extern "C" fn() -> i32;
    type SetBlur = unsafe extern "C" fn(i32, u32, i32) -> i32;
    // SAFETY: dlsym lookups of C functions with the signatures above.
    let (conn, set_blur) = unsafe {
        (libc::dlsym(libc::RTLD_DEFAULT, c"CGSMainConnectionID".as_ptr()), libc::dlsym(libc::RTLD_DEFAULT, c"CGSSetWindowBackgroundBlurRadius".as_ptr()))
    };
    if !conn.is_null() && !set_blur.is_null() {
        // SAFETY: both symbols exist and match the declared signatures.
        unsafe {
            let conn: MainConnection = std::mem::transmute(conn);
            let set_blur: SetBlur = std::mem::transmute(set_blur);
            set_blur(conn(), window.windowNumber() as u32, radius as i32);
        }
    }
}

/// The screen under the mouse (where you're looking), else the main one.
fn active_screen(mtm: MainThreadMarker) -> Option<Retained<NSScreen>> {
    let m = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    screens.iter().find(|s| {
        let f = s.frame();
        m.x >= f.origin.x && m.x <= f.origin.x + f.size.width && m.y >= f.origin.y && m.y <= f.origin.y + f.size.height
    }).or_else(|| NSScreen::mainScreen(mtm))
}

pub fn is_shown(mtm: MainThreadMarker) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    !app.isHidden() && window(mtm).is_some_and(|w| w.isVisible())
}

/// Centred horizontally, a little above the middle, like Spotlight.
pub fn show(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    let Some(w) = window(mtm) else { return };
    if let Some(screen) = active_screen(mtm) {
        let v = screen.visibleFrame();
        let size = w.frame().size;
        let x = v.origin.x + (v.size.width - size.width) / 2.0;
        let top = v.origin.y + v.size.height - (v.size.height - size.height) * 0.32;
        w.setFrameTopLeftPoint(NSPoint::new(x.round(), top.round()));
    }
    app.unhideWithoutActivation();
    if cfg!(debug_assertions) && std::env::var_os("GLINT_UI_SNAPSHOT").is_some() {
        // Dev snapshots never take focus (or your keystrokes).
        w.orderFrontRegardless();
        return;
    }
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    w.makeKeyAndOrderFront(None);
}

/// Hides Glint, which hands focus back to the app you came from.
pub fn hide(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm).hide(None);
}

// ---- alerts ----

static MODAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True while an alert is up.
pub fn modal() -> bool {
    MODAL.load(std::sync::atomic::Ordering::Relaxed)
}

/// A standard alert, the same as Slat's update messages. Focus goes back
/// to the app you were in afterwards unless the launcher is open.
pub fn alert(mtm: MainThreadMarker, text: &str) {
    use std::sync::atomic::Ordering;
    MODAL.store(true, Ordering::Relaxed);
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    let a = objc2_app_kit::NSAlert::new(mtm);
    a.setMessageText(&NSString::from_str(text));
    a.runModal();
    MODAL.store(false, Ordering::Relaxed);
    if !window(mtm).is_some_and(|w| w.isVisible() && w.alphaValue() > 0.0) {
        app.hide(None);
    }
}

// ---- HUD ----

thread_local! {
    static HUD: RefCell<Option<(Retained<NSPanel>, Retained<NSTextField>)>> = const { RefCell::new(None) };
    static HUD_GEN: RefCell<u64> = const { RefCell::new(0) };
}

/// A dark pill at the bottom of the screen that goes away after a moment.
/// Drawn with an NSBox (fill, hairline, corner radius) so it renders the same
/// everywhere, with no material or layer tricks.
pub fn hud(mtm: MainThreadMarker, text: &str) {
    let (panel, label) = HUD.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(|| {
                let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                    NSPanel::alloc(mtm),
                    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 40.0)),
                    NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                    NSBackingStoreType::Buffered,
                    false,
                );
                panel.setLevel(NSStatusWindowLevel);
                panel.setOpaque(false);
                panel.setBackgroundColor(Some(&NSColor::clearColor()));
                panel.setIgnoresMouseEvents(true);
                panel.setHasShadow(true);
                // SAFETY: the panel is ours and lives for the process.
                unsafe { panel.setReleasedWhenClosed(false) };
                panel.setCollectionBehavior(NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary);
                let pill = NSBox::new(mtm);
                pill.setBoxType(NSBoxType::Custom);
                pill.setTitlePosition(NSTitlePosition::NoTitle);
                pill.setContentViewMargins(NSSize::new(0.0, 0.0));
                pill.setFillColor(&NSColor::colorWithWhite_alpha(0.09, 0.94));
                pill.setBorderColor(&NSColor::colorWithWhite_alpha(1.0, 0.14));
                pill.setBorderWidth(1.0);
                pill.setCornerRadius(20.0);
                let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
                // SAFETY: AppKit's font weight constant.
                label.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, unsafe { NSFontWeightMedium })));
                label.setTextColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.92)));
                pill.addSubview(&label);
                panel.setContentView(Some(&pill as &NSView));
                (panel, label)
            })
            .clone()
    });
    label.setStringValue(&NSString::from_str(text));
    label.sizeToFit();
    let ls = label.frame().size;
    let (w, h) = ((ls.width + 44.0).ceil(), 40.0);
    label.setFrameOrigin(NSPoint::new(22.0, ((h - ls.height) / 2.0).round()));
    if let Some(screen) = active_screen(mtm) {
        let v = screen.visibleFrame();
        let origin = NSPoint::new((v.origin.x + (v.size.width - w) / 2.0).round(), v.origin.y + 110.0);
        panel.setFrame_display(NSRect::new(origin, NSSize::new(w, h)), true);
    }
    panel.orderFrontRegardless();
    #[cfg(debug_assertions)]
    if let Ok(path) = std::env::var("GLINT_HUD_SNAPSHOT")
        && let Some(view) = panel.contentView()
    {
        // Dev only: render the pill to a PNG (no Screen Recording needed).
        let b = view.bounds();
        if let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(b) {
            view.cacheDisplayInRect_toBitmapImageRep(b, &rep);
            // SAFETY: an empty properties dictionary is valid for PNG output.
            let png = unsafe { rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &objc2_foundation::NSDictionary::new()) };
            if let Some(png) = png {
                let _ = std::fs::write(path, png.to_vec());
            }
        }
    }
    let stamp = HUD_GEN.with(|g| {
        *g.borrow_mut() += 1;
        *g.borrow()
    });
    let when = DispatchTime::try_from(Duration::from_millis(1500)).unwrap_or(DispatchTime::NOW);
    let _ = DispatchQueue::main().after(when, move || {
        if HUD_GEN.with(|g| *g.borrow()) == stamp {
            HUD.with(|h| {
                if let Some((p, _)) = h.borrow().as_ref() {
                    p.orderOut(None);
                }
            });
        }
    });
}
