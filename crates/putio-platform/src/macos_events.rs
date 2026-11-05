//! Receiving magnet links and `.torrent` files from macOS Launch Services.
//!
//! Launch Services delivers them as Apple Events (`GURL` for URLs, `aevt`/`odoc` for documents), not as
//! command-line arguments. The strings are forwarded untouched to a channel; whoever reads the channel
//! validates them (`putio_core::intake::Intake::parse`).
//!
//! **When the handlers are installed matters** (see `docs/spikes.md`): `AppKit` installs its own `odoc`
//! handler during launch, replacing an earlier one, and a cold launch delivers the event before any UI
//! exists. The handlers are therefore installed immediately and again from the
//! `NSApplicationWillFinishLaunchingNotification` observer, which together catch cold and warm launches
//! for both magnets and files.

#![allow(unsafe_code)] // Objective-C FFI; every `unsafe` block has a SAFETY comment

use std::sync::mpsc::Sender;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSNotificationCenter, NSObject, NSObjectProtocol,
    NSString,
};

/// Apple Event four-character codes (`u32`, big-endian).
const fn code(bytes: [u8; 4]) -> u32 {
    u32::from_be_bytes(bytes)
}
const CLASS_INTERNET: u32 = code(*b"GURL");
const EVENT_GET_URL: u32 = code(*b"GURL");
const CLASS_CORE: u32 = code(*b"aevt");
const EVENT_OPEN_DOCUMENTS: u32 = code(*b"odoc");
const KEY_DIRECT_OBJECT: u32 = code(*b"----");
const MAX_ITEMS: isize = 64;

struct Ivars {
    tx: Sender<String>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the ivars are plain Rust data.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "PutioAdderEventHandler"]
    #[ivars = Ivars]
    struct Handler;

    // SAFETY: NSObjectProtocol has no extra invariants.
    unsafe impl NSObjectProtocol for Handler {}

    impl Handler {
        // SAFETY: selector and argument types match what NSAppleEventManager invokes.
        #[unsafe(method(handleGetURL:withReplyEvent:))]
        fn handle_get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            if let Some(text) = param(event, KEY_DIRECT_OBJECT).and_then(|d| d.stringValue()) {
                let _ = self.ivars().tx.send(text.to_string());
            }
        }

        // SAFETY: as above.
        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let Some(list) = param(event, KEY_DIRECT_OBJECT) else { return };
            // A list of file URLs; a lone item is not wrapped in a list.
            let count = list.numberOfItems().min(MAX_ITEMS);
            if count == 0 {
                if let Some(text) = list.stringValue() {
                    let _ = self.ivars().tx.send(text.to_string());
                }
                return;
            }
            for index in 1..=count {
                if let Some(text) = list.descriptorAtIndex(index).and_then(|item| item.stringValue()) {
                    let _ = self.ivars().tx.send(text.to_string());
                }
            }
        }

        // SAFETY: `NSNotificationCenter` calls observers with one NSNotification argument.
        #[unsafe(method(appWillFinishLaunching:))]
        fn app_will_finish_launching(&self, _notification: &AnyObject) {
            register(self);
        }
    }
);

fn param(event: &NSAppleEventDescriptor, key: u32) -> Option<Retained<NSAppleEventDescriptor>> {
    // SAFETY: `paramDescriptorForKeyword:` takes an AEKeyword (u32) and returns an optional descriptor.
    // It is called by message because the typed binding needs an extra crate for the `AEKeyword` alias.
    unsafe { msg_send![event, paramDescriptorForKeyword: key] }
}

/// (Re)register the handlers with the shared Apple Event manager. Safe to call repeatedly.
fn register(handler: &Handler) {
    let manager = NSAppleEventManager::sharedAppleEventManager();
    // SAFETY: the selectors exist on `Handler` with the signature the manager expects. The manager does
    // not retain the handler, which is why `install` leaks it to keep it alive for the whole process.
    unsafe {
        let _: () = msg_send![&*manager, setEventHandler: handler, andSelector: sel!(handleGetURL:withReplyEvent:), forEventClass: CLASS_INTERNET, andEventID: EVENT_GET_URL];
        let _: () = msg_send![&*manager, setEventHandler: handler, andSelector: sel!(handleOpenDocuments:withReplyEvent:), forEventClass: CLASS_CORE, andEventID: EVENT_OPEN_DOCUMENTS];
    }
}

/// Start receiving magnet links and files. Call once, on the main thread, as early as possible
/// (before the event loop starts). Strings arrive on `tx`; returns `false` if not on the main thread.
#[must_use]
pub fn install(tx: Sender<String>) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let handler: Retained<Handler> = {
        let this = Handler::alloc(mtm).set_ivars(Ivars { tx });
        // SAFETY: NSObject's `init` has the standard signature.
        unsafe { msg_send![super(this), init] }
    };

    let center = NSNotificationCenter::defaultCenter();
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    // SAFETY: `appWillFinishLaunching:` is defined above; a nil object observes every sender.
    unsafe {
        let _: () = msg_send![&*center, addObserver: &*handler, selector: sel!(appWillFinishLaunching:), name: &*name, object: std::ptr::null::<AnyObject>()];
    }
    register(&handler);
    // Neither the event manager nor the notification center retains the handler.
    std::mem::forget(handler);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_char_codes() {
        assert_eq!(code(*b"GURL"), 0x4755_524C);
        assert_eq!(KEY_DIRECT_OBJECT, 0x2D2D_2D2D);
    }

    #[test]
    fn installing_off_the_main_thread_is_refused() {
        // Tests run on worker threads.
        let (tx, _rx) = std::sync::mpsc::channel();
        assert!(!install(tx));
    }
}
