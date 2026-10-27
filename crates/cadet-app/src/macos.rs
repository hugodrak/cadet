//! Finder opens documents with an "open documents" Apple Event rather than argv.
//! AppKit installs its own handlers during launch, so ours goes in on WillFinishLaunching.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{MainThreadMarker, MainThreadOnly, class, define_class, msg_send, sel};
use objc2_foundation::{NSNotificationCenter, NSString, ns_string};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static PICKED: Mutex<Vec<isize>> = Mutex::new(Vec::new());

thread_local! {
    static HANDLER: std::cell::OnceCell<Retained<OpenHandler>> = const { std::cell::OnceCell::new() };
    static ITEMS: std::cell::RefCell<Vec<(isize, Retained<AnyObject>, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}
static CTX: OnceLock<egui::Context> = OnceLock::new();

const fn code(c: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*c)
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct OpenHandler;

    impl OpenHandler {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish(&self, _note: &AnyObject) {
            unsafe {
                let mgr: Retained<AnyObject> = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
                let _: () = msg_send![&mgr, setEventHandler: self, andSelector: sel!(handleOpen:withReply:), forEventClass: code(b"aevt"), andEventID: code(b"odoc")];
            }
        }

        #[unsafe(method(menuPick:))]
        fn menu_pick(&self, sender: &AnyObject) {
            let tag: isize = unsafe { msg_send![sender, tag] };
            PICKED.lock().unwrap().push(tag);
            if let Some(ctx) = CTX.get() {
                ctx.request_repaint();
            }
        }

        #[unsafe(method(handleOpen:withReply:))]
        fn handle_open(&self, event: &AnyObject, _reply: &AnyObject) {
            let paths = unsafe { file_paths(event) };
            OPENED.lock().unwrap().extend(paths);
            if let Some(ctx) = CTX.get() {
                ctx.request_repaint();
            }
        }
    }
);

unsafe fn file_paths(event: &AnyObject) -> Vec<PathBuf> {
    let mut out = vec![];
    unsafe {
        let list: Option<Retained<AnyObject>> = msg_send![event, paramDescriptorForKeyword: code(b"----")];
        let Some(list) = list else { return out };
        let n: isize = msg_send![&list, numberOfItems];
        for i in 1..=n {
            let item: Option<Retained<AnyObject>> = msg_send![&list, descriptorAtIndex: i];
            let Some(item) = item else { continue };
            let url: Option<Retained<AnyObject>> = msg_send![&item, fileURLValue];
            let path: Option<Retained<NSString>> = url.and_then(|u| msg_send![&u, path]);
            out.extend(path.map(|p| PathBuf::from(p.to_string())));
        }
    }
    out
}

/// Call before the event loop starts.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let handler: Retained<OpenHandler> = unsafe { msg_send![OpenHandler::alloc(mtm), init] };
    let name = ns_string!("NSApplicationWillFinishLaunchingNotification");
    unsafe { NSNotificationCenter::defaultCenter().addObserver_selector_name_object(&handler, sel!(willFinishLaunching:), Some(name), None) };
    HANDLER.with(|h| {
        let _ = h.set(handler);
    });
}

pub fn set_context(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

pub fn take_opened() -> Vec<PathBuf> {
    std::mem::take(&mut OPENED.lock().unwrap())
}

/// A menu item: label, tag (None for system items), key equivalent and modifiers, or a system selector.
pub enum Item {
    Pick { label: String, tag: isize, key: String, shift: bool, alt: bool },
    System { label: String, selector: objc2::runtime::Sel, key: &'static str, alt: bool },
    Separator,
}

/// Replaces the menu bar. The first menu is the app menu (titled by the system).
pub fn set_menus(menus: &[(&str, Vec<Item>)]) {
    let Some(handler) = HANDLER.with(|h| h.get().cloned()) else { return };
    let mut items = vec![];
    unsafe {
        let ns = |s: &str| NSString::from_str(s);
        let bar: Retained<AnyObject> = msg_send![class!(NSMenu), new];
        let mut window_menu = None;
        for (title, entries) in menus {
            let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
            let _: () = msg_send![&menu, setTitle: &*ns(title)];
            let _: () = msg_send![&menu, setAutoenablesItems: false];
            for e in entries {
                let item: Retained<AnyObject> = match e {
                    Item::Separator => msg_send![class!(NSMenuItem), separatorItem],
                    Item::Pick { label, tag, key, shift, alt } => {
                        let it: Retained<AnyObject> = msg_send![class!(NSMenuItem), new];
                        let _: () = msg_send![&it, setTitle: &*ns(label)];
                        let _: () = msg_send![&it, setAction: sel!(menuPick:)];
                        let _: () = msg_send![&it, setTarget: &*handler];
                        let _: () = msg_send![&it, setTag: *tag];
                        if !key.is_empty() {
                            let mask = (1usize << 20) | if *shift { 1 << 17 } else { 0 } | if *alt { 1 << 19 } else { 0 };
                            let _: () = msg_send![&it, setKeyEquivalent: &*ns(key)];
                            let _: () = msg_send![&it, setKeyEquivalentModifierMask: mask];
                        }
                        items.push((*tag, it.clone(), true));
                        it
                    }
                    Item::System { label, selector, key, alt } => {
                        let it: Retained<AnyObject> = msg_send![class!(NSMenuItem), new];
                        let _: () = msg_send![&it, setTitle: &*ns(label)];
                        let _: () = msg_send![&it, setAction: *selector];
                        let _: () = msg_send![&it, setKeyEquivalent: &*ns(key)];
                        if *alt {
                            let _: () = msg_send![&it, setKeyEquivalentModifierMask: (1usize << 20) | (1 << 19)];
                        }
                        it
                    }
                };
                let _: () = msg_send![&menu, addItem: &*item];
            }
            let top: Retained<AnyObject> = msg_send![class!(NSMenuItem), new];
            let _: () = msg_send![&top, setSubmenu: &*menu];
            let _: () = msg_send![&bar, addItem: &*top];
            if *title == "Window" {
                window_menu = Some(menu);
            }
        }
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![&app, setMainMenu: &*bar];
        if let Some(w) = window_menu {
            let _: () = msg_send![&app, setWindowsMenu: &*w];
        }
    }
    ITEMS.with(|i| *i.borrow_mut() = items);
}

/// Enables or disables menu items by tag (only touching AppKit when something changes).
pub fn set_enabled(enabled: impl Fn(isize) -> bool) {
    ITEMS.with(|items| {
        for (tag, item, state) in items.borrow_mut().iter_mut() {
            let want = enabled(*tag);
            if want != *state {
                *state = want;
                let _: () = unsafe { msg_send![&**item, setEnabled: want] };
            }
        }
    });
}

pub fn take_picked() -> Vec<isize> {
    std::mem::take(&mut PICKED.lock().unwrap())
}

/// The installed menu bar as text, for checking it in dev (`CADET_DUMP_MENU=1`).
pub fn dump() -> String {
    let mut out = String::new();
    unsafe {
        let app: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let bar: Option<Retained<AnyObject>> = msg_send![&app, mainMenu];
        let Some(bar) = bar else { return "no main menu".into() };
        let n: isize = msg_send![&bar, numberOfItems];
        for i in 0..n {
            let top: Retained<AnyObject> = msg_send![&bar, itemAtIndex: i];
            let sub: Option<Retained<AnyObject>> = msg_send![&top, submenu];
            let Some(sub) = sub else { continue };
            let title: Retained<NSString> = msg_send![&sub, title];
            out += &format!("{title}:");
            let m: isize = msg_send![&sub, numberOfItems];
            for j in 0..m {
                let it: Retained<AnyObject> = msg_send![&sub, itemAtIndex: j];
                let sep: bool = msg_send![&it, isSeparatorItem];
                if sep {
                    out += " |";
                    continue;
                }
                let t: Retained<NSString> = msg_send![&it, title];
                let k: Retained<NSString> = msg_send![&it, keyEquivalent];
                let on: bool = msg_send![&it, isEnabled];
                out += &format!(" {t}{}{}", if k.to_string().is_empty() { String::new() } else { format!(" [⌘{}]", k) }, if on { "" } else { " (off)" });
            }
            out += "\n";
        }
    }
    out
}
