//! Thin wrappers over AppKit and friends: drawing icons, the pasteboard,
//! pasting into the front app, the color sampler, running apps and the
//! system actions.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::process::Command;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSApplicationActivationPolicy, NSBitmapImageRep, NSColor, NSColorSampler, NSColorSpace, NSCompositingOperation, NSDeviceRGBColorSpace,
    NSBitmapImageFileType, NSFont, NSFontAttributeName, NSGraphicsContext, NSImage, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG,
    NSPasteboardTypeString, NSPasteboardTypeTIFF,
    NSRunningApplication, NSStringDrawing, NSWorkspace,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSFileManager, NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::search::color::Rgb;

// ---- images ----

/// Something Glint draws as an icon.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Img {
    /// Finder's icon for a file, folder or app.
    File(PathBuf),
    /// An SF Symbol, drawn white so it can be tinted.
    Symbol(&'static str),
    /// An emoji in Apple Color Emoji (the emoji picker only; never as an icon).
    Emoji(String),
    /// The picture inside an image file, fitted into the square.
    Thumb(PathBuf),
    /// The same, large, for the preview pane.
    Preview(PathBuf),
}

/// Files `Img::Thumb` can draw: what NSImage reads.
pub fn is_image(p: &Path) -> bool {
    p.extension().is_some_and(|e| matches!(e.to_ascii_lowercase().to_str(), Some("png" | "jpg" | "jpeg" | "gif" | "tif" | "tiff" | "heic" | "icns" | "bmp" | "webp" | "pdf")))
}

/// The icon a script, quicklink or list item asked for: an image path, or an
/// SF Symbol name. Anything else (emoji included) gets `fallback`.
pub fn icon_named(name: Option<&str>, fallback: &'static str) -> Img {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) if n.starts_with('/') || n.starts_with('~') => {
            let p = PathBuf::from(n.replacen('~', &crate::paths::home().display().to_string(), 1));
            // An image is the icon itself, not Finder's icon for an image file.
            if is_image(&p) { Img::Thumb(p) } else { Img::File(p) }
        }
        Some(n) if n.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') => Img::Symbol(intern(n)),
        _ => Img::Symbol(fallback),
    }
}

/// SF Symbol names live for the process; each distinct one is stored once.
fn intern(name: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static NAMES: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let set = names.get_or_insert_with(HashSet::new);
    if let Some(n) = set.get(name) {
        return n;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    set.insert(leaked);
    leaked
}

/// Renders `img` to `px`×`px` premultiplied RGBA. Safe off the main thread.
pub fn render(img: &Img, px: usize) -> Option<Vec<u8>> {
    // SAFETY: a private RGBA bitmap and graphics context, used only on this thread.
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            px as isize,
            px as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (px * 4) as isize,
            32,
        )?;
        let ctx = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&ctx));
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(px as f64, px as f64));
        match img {
            Img::File(path) => {
                let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&path.to_string_lossy()));
                image.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0);
            }
            Img::Symbol(name) => {
                // Unknown names (typos in an extension) get a neutral glyph.
                let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
                    .or_else(|| NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("app.dashed"), None))?;
                // Fit the symbol's aspect inside the square, then paint it white.
                let s = image.size();
                let scale = (px as f64 * 0.86) / s.width.max(s.height).max(1.0);
                let (w, h) = (s.width * scale, s.height * scale);
                let r = NSRect::new(NSPoint::new((px as f64 - w) / 2.0, (px as f64 - h) / 2.0), NSSize::new(w, h));
                image.drawInRect_fromRect_operation_fraction(r, NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0);
                NSColor::whiteColor().set();
                objc2_app_kit::NSRectFillUsingOperation(rect, NSCompositingOperation::SourceIn);
            }
            Img::Thumb(path) | Img::Preview(path) => {
                let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(&path.to_string_lossy()))?;
                let s = image.size();
                let scale = px as f64 / s.width.max(s.height).max(1.0);
                let (w, h) = (s.width * scale, s.height * scale);
                let r = NSRect::new(NSPoint::new((px as f64 - w) / 2.0, (px as f64 - h) / 2.0), NSSize::new(w, h));
                image.drawInRect_fromRect_operation_fraction(r, NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0);
            }
            Img::Emoji(e) => {
                let size = px as f64 * 0.8;
                let font = NSFont::fontWithName_size(&NSString::from_str("Apple Color Emoji"), size).unwrap_or_else(|| NSFont::systemFontOfSize(size));
                let key: &NSString = NSFontAttributeName;
                let attrs = NSDictionary::from_slices(&[key], &[&*font as &AnyObject]);
                let s = NSString::from_str(e);
                let m = s.sizeWithAttributes(Some(&attrs));
                s.drawAtPoint_withAttributes(NSPoint::new((px as f64 - m.width) / 2.0, (px as f64 - m.height) / 2.0), Some(&attrs));
            }
        }
        ctx.flushGraphics();
        NSGraphicsContext::restoreGraphicsState_class();
        let data = rep.bitmapData();
        if data.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(data, px * px * 4).to_vec())
    }
}

// ---- pasteboard ----

pub fn change_count() -> isize {
    NSPasteboard::generalPasteboard().changeCount()
}

/// The clipboard as text, or as a file path when Finder copied a file.
pub fn read_clipboard() -> Option<(String, bool)> {
    let pb = NSPasteboard::generalPasteboard();
    // SAFETY: AppKit-provided pasteboard type constants.
    unsafe {
        if let Some(url) = pb.stringForType(NSPasteboardTypeFileURL)
            && let Some(u) = NSURL::URLWithString(&url)
            && let Some(p) = u.path()
        {
            return Some((p.to_string(), true));
        }
        pb.stringForType(NSPasteboardTypeString).map(|s| (s.to_string(), false))
    }
}

/// An image on the clipboard (screenshots, copied pictures) as PNG bytes and
/// its pixel size. Only asked when there's no text or file.
pub fn read_clipboard_image() -> Option<(Vec<u8>, [u32; 2])> {
    read_image(&NSPasteboard::generalPasteboard())
}

fn read_image(pb: &NSPasteboard) -> Option<(Vec<u8>, [u32; 2])> {
    // SAFETY: AppKit-provided pasteboard type constants.
    let (png, tiff) = unsafe { (NSPasteboardTypePNG, NSPasteboardTypeTIFF) };
    for ty in [png, tiff] {
        let Some(data) = pb.dataForType(ty) else { continue };
        let rep = NSBitmapImageRep::imageRepWithData(&data)?;
        let size = [rep.pixelsWide() as u32, rep.pixelsHigh() as u32];
        let bytes = if ty == png {
            data.to_vec()
        } else {
            // SAFETY: an empty properties dictionary is valid for PNG output.
            unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?.to_vec()
        };
        return Some((bytes, size));
    }
    None
}

/// Puts an image file on the clipboard as PNG and TIFF (some apps only read TIFF).
pub fn copy_image(path: &Path) -> bool {
    write_image(&NSPasteboard::generalPasteboard(), path)
}

fn write_image(pb: &NSPasteboard, path: &Path) -> bool {
    let Some(data) = NSData::dataWithContentsOfFile(&NSString::from_str(&path.to_string_lossy())) else { return false };
    pb.clearContents();
    // SAFETY: AppKit-provided pasteboard type constants.
    unsafe {
        pb.setData_forType(Some(&data), NSPasteboardTypePNG);
        if let Some(tiff) = NSBitmapImageRep::imageRepWithData(&data).and_then(|r| r.TIFFRepresentation()) {
            pb.setData_forType(Some(&tiff), NSPasteboardTypeTIFF);
        }
    }
    true
}

/// Text copied by password managers is marked concealed; history skips it.
pub fn clipboard_concealed() -> bool {
    let pb = NSPasteboard::generalPasteboard();
    let types = pb.types();
    types.is_some_and(|t| t.iter().any(|ty| {
        let s = ty.to_string();
        s == "org.nspasteboard.ConcealedType" || s == "org.nspasteboard.TransientType" || s == "com.agilebits.onepassword"
    }))
}

pub fn copy(text: &str) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    // SAFETY: AppKit-provided pasteboard type constant.
    unsafe {
        pb.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
    }
}

/// Copies a file itself (pastes as the file in Finder, Mail, Slack...).
pub fn copy_file(path: &Path) {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let objs = NSArray::from_retained_slice(&[Retained::into_super(url)]);
    // SAFETY: NSURL conforms to NSPasteboardWriting.
    unsafe {
        let objs: &NSArray<objc2::runtime::ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>> = &*(&*objs as *const NSArray<objc2_foundation::NSObject> as *const _);
        pb.writeObjects(objs);
    }
}

// ---- input ----

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    fn CGEventCreateKeyboardEvent(source: *const c_void, keycode: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

pub fn ax_trusted() -> bool {
    // SAFETY: no arguments, no side effects.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Shows the system's "allow Glint to control your computer" prompt.
pub fn ax_prompt() {
    let key = NSString::from_str("AXTrustedCheckOptionPrompt");
    let yes = objc2_foundation::NSNumber::new_bool(true);
    let dict = NSDictionary::from_slices(&[&*key], &[&*yes as &AnyObject]);
    // SAFETY: a CFDictionary (toll-free bridged NSDictionary) with the documented key.
    unsafe { AXIsProcessTrustedWithOptions(Retained::as_ptr(&dict).cast()) };
}

pub const ACCESSIBILITY_URL: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/// ⌘V into whatever app is frontmost. Needs Accessibility.
pub fn paste() {
    const V: u16 = 9;
    const CMD: u64 = 0x10_0000;
    // SAFETY: plain CGEvent creation and posting; events are released after use.
    unsafe {
        for down in [true, false] {
            let e = CGEventCreateKeyboardEvent(std::ptr::null(), V, down);
            if e.is_null() {
                return;
            }
            CGEventSetFlags(e, CMD);
            CGEventPost(0, e);
            CFRelease(e);
        }
    }
}

// ---- color sampler ----

/// The system eyedropper with its magnifier. `done` gets the color (None if
/// cancelled), on the main thread.
pub fn pick_color(_mtm: MainThreadMarker, done: impl Fn(Option<Rgb>) + 'static) {
    let sampler = NSColorSampler::new();
    let block = RcBlock::new(move |c: *mut NSColor| {
        // SAFETY: AppKit passes a valid NSColor or null.
        let color = unsafe { c.as_ref() }.and_then(|c| {
            let srgb = c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
            let mut rgb = Rgb::new(srgb.redComponent() as f32, srgb.greenComponent() as f32, srgb.blueComponent() as f32);
            rgb.a = srgb.alphaComponent() as f32;
            Some(rgb)
        });
        done(color);
    });
    // SAFETY: the sampler retains the block until it calls it.
    unsafe { sampler.showSamplerWithSelectionHandler(&block) };
    // The sampler must outlive this call; it's tiny, so keep it.
    std::mem::forget(sampler);
}

// ---- apps and files ----

#[derive(Clone, Debug)]
pub struct Running {
    pub name: String,
    pub pid: i32,
    pub path: Option<PathBuf>,
    pub bundle_id: String,
    pub hidden: bool,
}

/// Regular (Dock) apps that are running, Glint excluded.
pub fn running_apps() -> Vec<Running> {
    let me = std::process::id() as i32;
    NSWorkspace::sharedWorkspace()
        .runningApplications()
        .iter()
        .filter(|a| a.activationPolicy() == NSApplicationActivationPolicy::Regular && a.processIdentifier() != me && !a.isTerminated())
        .map(|a| Running {
            name: a.localizedName().map(|n| n.to_string()).unwrap_or_default(),
            pid: a.processIdentifier(),
            path: a.bundleURL().and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())),
            bundle_id: a.bundleIdentifier().map(|b| b.to_string()).unwrap_or_default(),
            hidden: a.isHidden(),
        })
        .collect()
}

fn app(pid: i32) -> Option<Retained<NSRunningApplication>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
}

pub fn quit_app(pid: i32, force: bool) -> bool {
    app(pid).is_some_and(|a| if force { a.forceTerminate() } else { a.terminate() })
}

/// The app that was in front before Glint (name, pid).
pub fn frontmost() -> Option<(String, i32)> {
    let a = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some((a.localizedName().map(|n| n.to_string()).unwrap_or_default(), a.processIdentifier()))
}

pub fn hide_others(except_pid: Option<i32>) {
    for a in running_apps() {
        if Some(a.pid) != except_pid
            && let Some(app) = app(a.pid)
        {
            app.hide();
        }
    }
}

pub fn unhide_all() {
    for a in running_apps() {
        if let Some(app) = app(a.pid) {
            app.unhide();
        }
    }
}

pub fn quit_all() {
    for a in running_apps() {
        if a.bundle_id != "com.apple.finder" {
            quit_app(a.pid, false);
        }
    }
}

/// Runs `open` on its own thread: it takes 50-300 ms, which the caller
/// (often the UI) shouldn't wait for.
pub fn open_with(args: &[&std::ffi::OsStr]) {
    let args: Vec<std::ffi::OsString> = args.iter().map(|a| a.to_os_string()).collect();
    std::thread::spawn(move || Command::new("/usr/bin/open").args(args).status());
}

pub fn open(target: &str) {
    open_with(&[target.as_ref()]);
}

pub fn open_path(p: &Path) {
    open_with(&[p.as_os_str()]);
}

pub fn reveal(p: &Path) {
    open_with(&["-R".as_ref(), p.as_os_str()]);
}

pub fn open_in(app_name: &str, p: &Path) {
    open_with(&["-a".as_ref(), app_name.as_ref(), p.as_os_str()]);
}

/// Quick Look preview panel.
pub fn quick_look(p: &Path) {
    let _ = Command::new("/usr/bin/qlmanage").arg("-p").arg(p).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
}

pub fn trash(p: &Path) -> bool {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()));
    NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).is_ok()
}

pub fn bundle_id(app: &Path) -> Option<String> {
    let out = Command::new("/usr/bin/mdls").args(["-name", "kMDItemCFBundleIdentifier", "-raw"]).arg(app).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty() && s != "(null)").then_some(s)
}

// ---- system actions ----

pub fn osascript(script: &str) -> Result<String, String> {
    let out = Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Locks the screen through the login framework (what the menu bar's Lock
/// Screen does); falls back to the ⌃⌘Q shortcut.
pub fn lock_screen() {
    type Lock = unsafe extern "C" fn() -> i32;
    // SAFETY: dlopen/dlsym of a C function with no arguments.
    unsafe {
        let h = libc::dlopen(c"/System/Library/PrivateFrameworks/login.framework/login".as_ptr(), libc::RTLD_LAZY);
        if !h.is_null() {
            let f = libc::dlsym(h, c"SACLockScreenImmediate".as_ptr());
            if !f.is_null() {
                let lock: Lock = std::mem::transmute(f);
                lock();
                return;
            }
        }
    }
    let _ = osascript(r#"tell application "System Events" to keystroke "q" using {control down, command down}"#);
}

pub fn pmset(arg: &str) {
    let _ = Command::new("/usr/bin/pmset").arg(arg).status();
}

/// Restart, shut down or log out via loginwindow (no second dialog).
pub fn power(event: &str) -> Result<String, String> {
    osascript(&format!(r#"tell application "loginwindow" to «event aevt{event}»"#))
}

pub fn toggle_hidden_files() -> bool {
    let shown = Command::new("/usr/bin/defaults")
        .args(["read", "com.apple.finder", "AppleShowAllFiles"])
        .output()
        .is_ok_and(|o| matches!(String::from_utf8_lossy(&o.stdout).trim().to_lowercase().as_str(), "1" | "true" | "yes"));
    let _ = Command::new("/usr/bin/defaults").args(["write", "com.apple.finder", "AppleShowAllFiles", "-bool", if shown { "false" } else { "true" }]).status();
    let _ = Command::new("/usr/bin/killall").arg("Finder").status();
    !shown
}

// ---- Spotlight's ⌘Space ----

const SYMBOLIC: &str = "com.apple.symbolichotkeys";

/// Whether Spotlight's ⌘Space shortcut is switched on.
pub fn spotlight_shortcut_on() -> bool {
    let out = Command::new("/usr/bin/defaults").args(["read", SYMBOLIC, "AppleSymbolicHotKeys"]).output();
    let Ok(out) = out else { return true };
    let text = String::from_utf8_lossy(&out.stdout);
    // The entry for 64 ("Show Spotlight search"); absent means the default (on).
    let Some(i) = text.find("    64 =") else { return true };
    let block = &text[i..text.len().min(i + 200)];
    !block.contains("enabled = 0")
}

/// Turns Spotlight's ⌘Space on or off and applies it without logging out.
pub fn set_spotlight_shortcut(on: bool) -> bool {
    let entry = format!(
        "<dict><key>enabled</key><{}/><key>value</key><dict><key>parameters</key><array><integer>32</integer><integer>49</integer><integer>1048576</integer></array><key>type</key><string>standard</string></dict></dict>",
        if on { "true" } else { "false" }
    );
    let ok = Command::new("/usr/bin/defaults").args(["write", SYMBOLIC, "AppleSymbolicHotKeys", "-dict-add", "64", &entry]).status().is_ok_and(|s| s.success());
    let _ = Command::new("/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings").arg("-u").status();
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_round_trip_through_a_pasteboard_and_render() {
        // A private pasteboard: the test never touches your clipboard.
        let pb = NSPasteboard::pasteboardWithUniqueName();
        let dir = std::env::temp_dir().join(format!("glint-img-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("in.png");
        // Render a symbol and save it as a 64x64 PNG to use as the copied image.
        let rgba = render(&Img::Symbol("star.fill"), 64).expect("symbol renders");
        let png = {
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(), std::ptr::null_mut(), 64, 64, 8, 4, true, false, NSDeviceRGBColorSpace, 256, 32,
                )
            }
            .unwrap();
            unsafe { std::ptr::copy_nonoverlapping(rgba.as_ptr(), rep.bitmapData(), rgba.len()) };
            unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }.unwrap().to_vec()
        };
        std::fs::write(&src, &png).unwrap();
        assert!(write_image(&pb, &src));
        let (bytes, size) = read_image(&pb).expect("the image reads back");
        assert_eq!(size, [64, 64]);
        assert!(bytes.starts_with(b"\x89PNG"));
        let thumb = render(&Img::Thumb(src.clone()), 32).expect("thumbnails render");
        assert!(thumb.chunks(4).any(|px| px[3] > 0), "the thumbnail has visible pixels");
        assert!(is_image(&src) && !is_image(Path::new("/Applications/Safari.app")));
        let _ = std::fs::remove_dir_all(dir);
    }
}
