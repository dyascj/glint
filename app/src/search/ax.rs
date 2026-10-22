//! Reading other apps through Accessibility: the front app's menu bar
//! (Search Menu Items) and every app's open windows (Switch Windows).
//! Each call is a round trip into the other app, so these run off the main
//! thread under a short timeout, and fetch several attributes per trip.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Once;

use objc2::rc::Retained;
use objc2_foundation::{NSArray, NSString};

use crate::macos::Running;

type Ref = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> Ref;
    fn AXUIElementCreateSystemWide() -> Ref;
    fn AXUIElementCopyAttributeValue(e: Ref, attr: Ref, value: *mut Ref) -> i32;
    fn AXUIElementCopyMultipleAttributeValues(e: Ref, attrs: Ref, options: u32, values: *mut Ref) -> i32;
    fn AXUIElementPerformAction(e: Ref, action: Ref) -> i32;
    fn AXUIElementSetAttributeValue(e: Ref, attr: Ref, value: Ref) -> i32;
    fn AXUIElementSetMessagingTimeout(e: Ref, secs: f32) -> i32;
    fn AXUIElementGetTypeID() -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRetain(cf: Ref) -> Ref;
    fn CFRelease(cf: Ref);
    fn CFGetTypeID(cf: Ref) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFArrayGetCount(a: Ref) -> isize;
    fn CFArrayGetValueAtIndex(a: Ref, i: isize) -> Ref;
    fn CFBooleanGetValue(b: Ref) -> u8;
    fn CFNumberGetValue(n: Ref, kind: isize, out: *mut c_void) -> u8;
    static kCFBooleanTrue: Ref;
    static kCFBooleanFalse: Ref;
}

/// A retained AXUIElement, safe to hand between threads.
pub struct Elem(Ref);

// SAFETY: AXUIElement is an immutable CF handle; AX calls on it are thread-safe.
unsafe impl Send for Elem {}
// SAFETY: as above.
unsafe impl Sync for Elem {}

impl Elem {
    /// Takes ownership of a +1 reference (Create/Copy).
    fn owned(r: Ref) -> Option<Elem> {
        (!r.is_null()).then_some(Elem(r))
    }

    /// Retains a borrowed reference (an array member).
    fn borrowed(r: Ref) -> Option<Elem> {
        // SAFETY: r is a live CF object owned by an array we still hold.
        (!r.is_null()).then(|| Elem(unsafe { CFRetain(r) }))
    }

    fn app(pid: i32) -> Option<Elem> {
        timeout_once();
        // SAFETY: plain constructor, returns +1 or null.
        Elem::owned(unsafe { AXUIElementCreateApplication(pid) })
    }

    fn get(&self, attr: &str) -> Val {
        let mut out: Ref = std::ptr::null();
        // SAFETY: valid element and CFString; out receives +1 on success.
        let err = unsafe { AXUIElementCopyAttributeValue(self.0, cfstr(attr).as_ptr(), &mut out) };
        if err != 0 || out.is_null() {
            return Val::None;
        }
        let v = val(out);
        // SAFETY: we own `out`.
        unsafe { CFRelease(out) };
        v
    }

    /// Several attributes in one round trip, in order; missing ones are None.
    fn get_many(&self, attrs: &[&str]) -> Vec<Val> {
        let names: Vec<Retained<NSString>> = attrs.iter().map(|a| NSString::from_str(a)).collect();
        let arr = NSArray::from_retained_slice(&names);
        let mut out: Ref = std::ptr::null();
        // SAFETY: an NSArray of NSStrings is a CFArray of CFStrings; out receives +1.
        let err = unsafe { AXUIElementCopyMultipleAttributeValues(self.0, Retained::as_ptr(&arr).cast(), 0, &mut out) };
        if err != 0 || out.is_null() {
            return attrs.iter().map(|_| Val::None).collect();
        }
        // SAFETY: out is a CFArray we own; members are borrowed while it lives.
        let vals = unsafe { (0..CFArrayGetCount(out)).map(|i| val(CFArrayGetValueAtIndex(out, i))).collect() };
        // SAFETY: we own `out`.
        unsafe { CFRelease(out) };
        vals
    }

    fn act(&self, action: &str) -> bool {
        // SAFETY: valid element and CFString.
        unsafe { AXUIElementPerformAction(self.0, cfstr(action).as_ptr()) == 0 }
    }

    fn set_bool(&self, attr: &str, on: bool) -> bool {
        // SAFETY: the CFBoolean constants are static; valid element and CFString.
        unsafe { AXUIElementSetAttributeValue(self.0, cfstr(attr).as_ptr(), if on { kCFBooleanTrue } else { kCFBooleanFalse }) == 0 }
    }
}

impl Clone for Elem {
    fn clone(&self) -> Elem {
        // SAFETY: self.0 is live; the clone owns the extra retain.
        Elem(unsafe { CFRetain(self.0) })
    }
}

impl Drop for Elem {
    fn drop(&mut self) {
        // SAFETY: we own one retain.
        unsafe { CFRelease(self.0) }
    }
}

impl PartialEq for Elem {
    fn eq(&self, other: &Elem) -> bool {
        self.0 == other.0
    }
}

impl std::fmt::Debug for Elem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Elem({:p})", self.0)
    }
}

/// A CFString for an attribute or action name.
struct CfStr(Retained<NSString>);

impl CfStr {
    fn as_ptr(&self) -> Ref {
        Retained::as_ptr(&self.0).cast()
    }
}

fn cfstr(s: &str) -> CfStr {
    CfStr(NSString::from_str(s))
}

/// A hung app shouldn't stall the list for the default six seconds.
fn timeout_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: the system-wide element sets the timeout for every AX call we make.
        unsafe {
            let sys = AXUIElementCreateSystemWide();
            if !sys.is_null() {
                AXUIElementSetMessagingTimeout(sys, 0.75);
                CFRelease(sys);
            }
        }
    });
}

enum Val {
    None,
    Str(String),
    Bool(bool),
    Num(i64),
    List(Vec<Elem>),
    Elem(Elem),
}

impl Val {
    fn str(&self) -> &str {
        if let Val::Str(s) = self { s } else { "" }
    }
    fn num(&self) -> Option<i64> {
        if let Val::Num(n) = self { Some(*n) } else { None }
    }
    fn bool(&self) -> Option<bool> {
        if let Val::Bool(b) = self { Some(*b) } else { None }
    }
    fn list(self) -> Vec<Elem> {
        if let Val::List(v) = self { v } else { Vec::new() }
    }
}

/// Converts a borrowed CF value. Anything else (AXValue errors, points) is None.
fn val(r: Ref) -> Val {
    if r.is_null() {
        return Val::None;
    }
    // SAFETY: r is a live CF object; each branch checks its type first.
    unsafe {
        let t = CFGetTypeID(r);
        if t == CFStringGetTypeID() {
            let s: &NSString = &*r.cast::<NSString>();
            Val::Str(s.to_string())
        } else if t == CFBooleanGetTypeID() {
            Val::Bool(CFBooleanGetValue(r) != 0)
        } else if t == CFNumberGetTypeID() {
            let mut n: i64 = 0;
            // kCFNumberSInt64Type
            CFNumberGetValue(r, 4, (&mut n as *mut i64).cast());
            Val::Num(n)
        } else if t == CFArrayGetTypeID() {
            Val::List((0..CFArrayGetCount(r)).filter_map(|i| Elem::borrowed(CFArrayGetValueAtIndex(r, i))).collect())
        } else if t == AXUIElementGetTypeID() {
            Elem::borrowed(r).map_or(Val::None, Val::Elem)
        } else {
            Val::None
        }
    }
}

// ---- menus ----

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub title: String,
    /// The menus above it ("File", "Export").
    pub path: Vec<String>,
    /// "⇧⌘S"; empty when it has none.
    pub shortcut: String,
    pub elem: Elem,
}

const MENU_ATTRS: [&str; 7] = ["AXRole", "AXTitle", "AXEnabled", "AXChildren", "AXMenuItemCmdChar", "AXMenuItemCmdModifiers", "AXMenuItemCmdGlyph"];
const MAX_ITEMS: usize = 4000;

/// Every enabled item in `pid`'s menu bar, in menu order (the Apple menu
/// and Services left out).
pub fn menu_items(pid: i32) -> Result<Vec<MenuItem>, String> {
    let app = Elem::app(pid).ok_or("That app can't be read")?;
    let Val::Elem(bar) = app.get("AXMenuBar") else { return Err("That app has no menu bar".into()) };
    let mut out = Vec::new();
    let mut path = Vec::new();
    // The first menu bar item is the Apple menu.
    for top in bar.get("AXChildren").list().into_iter().skip(1) {
        walk(&top, &mut path, &mut out, 0);
    }
    if out.is_empty() { Err("No menu items found".into()) } else { Ok(out) }
}

fn walk(el: &Elem, path: &mut Vec<String>, out: &mut Vec<MenuItem>, depth: usize) {
    if depth > 6 || out.len() >= MAX_ITEMS {
        return;
    }
    let mut v = el.get_many(&MENU_ATTRS).into_iter();
    let mut next = || v.next().unwrap_or(Val::None);
    let (role, title, enabled, children, key, mods, glyph) = (next(), next(), next(), next(), next(), next(), next());
    let children = children.list();
    match role.str() {
        // A menu is just the list under a title.
        "AXMenu" => {
            for c in &children {
                walk(c, path, out, depth + 1);
            }
        }
        "AXMenuBarItem" | "AXMenuItem" => {
            let title = title.str().trim().to_string();
            if title.is_empty() || title == "Services" {
                return;
            }
            if !children.is_empty() {
                path.push(title);
                for c in &children {
                    walk(c, path, out, depth + 1);
                }
                path.pop();
            } else if enabled.bool().unwrap_or(false) {
                out.push(MenuItem { title, path: path.clone(), shortcut: shortcut(key.str(), mods.num(), glyph.num()), elem: el.clone() });
            }
        }
        _ => {}
    }
}

/// Formats a menu shortcut the way menus show it ("⌥⌘I", "⇧⌘⌫").
fn shortcut(key: &str, mods: Option<i64>, glyph: Option<i64>) -> String {
    // Special keys come as a menu glyph code instead of a character.
    let key = match glyph.and_then(glyph_key) {
        Some(g) => g.to_string(),
        None if key.trim().is_empty() || key.chars().any(char::is_control) => return String::new(),
        None => key.trim().to_uppercase(),
    };
    // kAXMenuItemModifier: 1 shift, 2 option, 4 control, 8 no command.
    let m = mods.unwrap_or(0);
    let mut s = String::new();
    if m & 4 != 0 {
        s.push('⌃');
    }
    if m & 2 != 0 {
        s.push('⌥');
    }
    if m & 1 != 0 {
        s.push('⇧');
    }
    if m & 8 == 0 {
        s.push('⌘');
    }
    s.push_str(&key);
    s
}

/// The kMenu…Glyph codes menus use for keys without a character.
fn glyph_key(g: i64) -> Option<&'static str> {
    Some(match g {
        0x02 => "⇥",
        0x04 => "⌤",
        0x09 => "Space",
        0x0A => "⌦",
        0x0B => "↩",
        0x17 => "⌫",
        0x1B => "⎋",
        0x1C => "⌧",
        0x62 => "⇞",
        0x64 => "←",
        0x65 => "→",
        0x66 => "↖",
        0x68 => "↑",
        0x69 => "↘",
        0x6A => "↓",
        0x6B => "⇟",
        0x6F..=0x7A => ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"][(g - 0x6F) as usize],
        _ => return None,
    })
}

/// Clicks a menu item.
pub fn press(e: &Elem) -> bool {
    e.act("AXPress")
}

// ---- windows ----

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub title: String,
    pub app: String,
    pub pid: i32,
    pub app_path: Option<PathBuf>,
    pub minimized: bool,
    pub elem: Elem,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> Ref;
}

/// Standard windows of every running app on the current Space, frontmost
/// app first. Apps are asked in parallel, so one slow app doesn't hold up
/// the rest.
pub fn windows(apps: &[Running]) -> Vec<Window> {
    let order = front_to_back();
    let mut apps: Vec<&Running> = apps.iter().collect();
    apps.sort_by_key(|a| order.iter().position(|p| *p == a.pid).unwrap_or(usize::MAX));
    std::thread::scope(|s| {
        let jobs: Vec<_> = apps.into_iter().map(|a| s.spawn(move || app_windows(a))).collect();
        jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
    })
}

/// Pids in on-screen stacking order (no Screen Recording needed for pids).
fn front_to_back() -> Vec<i32> {
    use objc2_foundation::{NSDictionary, NSNumber};
    // kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements
    // SAFETY: returns a +1 CFArray of CFDictionaries, or null.
    let list = unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) };
    if list.is_null() {
        return Vec::new();
    }
    let key = NSString::from_str("kCGWindowOwnerPID");
    // SAFETY: CFArray/CFDictionary are toll-free bridged to NSArray/NSDictionary.
    let pids = unsafe {
        let arr: &NSArray<NSDictionary<NSString, objc2::runtime::AnyObject>> = &*list.cast();
        let mut pids = Vec::new();
        for d in arr.iter() {
            if let Some(n) = d.objectForKey(&key).and_then(|o| o.downcast::<NSNumber>().ok()) {
                let pid = n.intValue();
                if !pids.contains(&pid) {
                    pids.push(pid);
                }
            }
        }
        CFRelease(list);
        pids
    };
    pids
}

fn app_windows(a: &Running) -> Vec<Window> {
    let Some(app) = Elem::app(a.pid) else { return Vec::new() };
    app.get("AXWindows")
        .list()
        .into_iter()
        .filter_map(|w| {
            let mut v = w.get_many(&["AXTitle", "AXSubrole", "AXMinimized"]).into_iter();
            let mut next = || v.next().unwrap_or(Val::None);
            let (title, subrole, minimized) = (next(), next(), next());
            if !matches!(subrole.str(), "AXStandardWindow" | "AXDialog") {
                return None;
            }
            let title = match title.str().trim() {
                "" => a.name.clone(),
                t => t.to_string(),
            };
            Some(Window { title, app: a.name.clone(), pid: a.pid, app_path: a.path.clone(), minimized: minimized.bool().unwrap_or(false), elem: w })
        })
        .collect()
}

/// Brings a window to the front, restoring it from the Dock if needed.
pub fn raise(pid: i32, e: &Elem) -> bool {
    e.set_bool("AXMinimized", false);
    e.set_bool("AXMain", true);
    let raised = e.act("AXRaise");
    if let Some(app) = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        #[allow(deprecated)]
        app.activateWithOptions(objc2_app_kit::NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
    raised
}

#[cfg(test)]
mod tests {
    use super::shortcut;

    #[test]
    fn menu_shortcuts_read_like_menus() {
        assert_eq!(shortcut("s", Some(0), None), "⌘S");
        assert_eq!(shortcut("S", Some(1), None), "⇧⌘S");
        assert_eq!(shortcut("i", Some(2), Some(0)), "⌥⌘I");
        assert_eq!(shortcut("f", Some(4 | 8), None), "⌃F");
        assert_eq!(shortcut("", Some(0), None), "");
        assert_eq!(shortcut("\u{8}", Some(1), Some(0x17)), "⇧⌘⌫", "glyph keys");
        assert_eq!(shortcut("\u{8}", Some(0), None), "", "control characters alone aren't shown");
        assert_eq!(shortcut("", Some(0), Some(0x71)), "⌘F3");
    }
}
