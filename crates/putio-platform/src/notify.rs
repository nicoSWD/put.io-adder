//! Desktop notifications for finished downloads.

use std::sync::Arc;

pub use putio_core::state::Notification;

/// Called when the user clicks a notification, with the file to open on put.io (if any).
pub type ClickHandler = Arc<dyn Fn(Option<i64>) + Send + Sync>;

pub trait Notifier: Send + Sync {
    /// Best effort: a notification that cannot be shown is not worth failing over, and the transfer
    /// list in the window always shows the same information.
    fn notify(&self, notification: &Notification);
}

/// Shows nothing.
#[derive(Debug, Default)]
pub struct NullNotifier;

impl Notifier for NullNotifier {
    fn notify(&self, _notification: &Notification) {}
}

/// Collects notifications. For tests.
#[derive(Debug, Default)]
pub struct RecordingNotifier {
    seen: std::sync::Mutex<Vec<Notification>>,
}

impl RecordingNotifier {
    #[must_use]
    pub fn seen(&self) -> Vec<Notification> {
        self.seen.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&self, notification: &Notification) {
        if let Ok(mut seen) = self.seen.lock() {
            seen.push(notification.clone());
        }
    }
}

/// The notifier for this platform. `on_click` is invoked on platforms that report clicks (macOS).
///
/// An unbundled macOS binary (`cargo run`) cannot use `UserNotifications`, so it gets the
/// [`NullNotifier`].
#[must_use]
pub fn system_notifier(on_click: ClickHandler) -> Arc<dyn Notifier> {
    imp::system_notifier(on_click)
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Objective-C FFI; every `unsafe` block has a SAFETY comment
mod imp {
    use std::sync::{Arc, OnceLock};

    use block2::{DynBlock, RcBlock};
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObjectProtocol, ProtocolObject};
    use objc2::{AnyThread, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSError, NSObject, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
        UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
        UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::{ClickHandler, Notification, Notifier, NullNotifier};

    /// Identifier prefix of notifications whose click should open a file; the file id follows.
    const FILE_PREFIX: &str = "file:";
    static CLICK: OnceLock<ClickHandler> = OnceLock::new();

    define_class!(
        // SAFETY: NSObject has no subclassing requirements and this class has no instance variables.
        #[unsafe(super = NSObject)]
        #[name = "PutioAdderNotificationDelegate"]
        #[ivars = ()]
        struct Delegate;

        // SAFETY: NSObjectProtocol has no extra invariants.
        unsafe impl NSObjectProtocol for Delegate {}

        // SAFETY: the method signatures are the ones declared by the generated protocol bindings.
        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                // Show the banner even while the app is in front.
                completion.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion: &DynBlock<dyn Fn()>,
            ) {
                let identifier = response.notification().request().identifier().to_string();
                if let Some(handler) = CLICK.get() {
                    handler(file_id_from_identifier(&identifier));
                }
                completion.call(());
            }
        }
    );

    /// `file:123:uuid-ish` → `Some(123)`; anything else → `None`.
    pub(super) fn file_id_from_identifier(identifier: &str) -> Option<i64> {
        let rest = identifier.strip_prefix(FILE_PREFIX)?;
        let id: i64 = rest.split(':').next()?.parse().ok()?;
        (id >= 0).then_some(id)
    }

    pub(super) fn identifier(file_id: Option<i64>, serial: u64) -> String {
        match file_id {
            Some(id) if id >= 0 => format!("{FILE_PREFIX}{id}:{serial}"),
            _ => format!("none:{serial}"),
        }
    }

    /// `UserNotifications` only works from an app bundle.
    fn is_bundled() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    #[derive(Debug)]
    struct NativeNotifier {
        serial: std::sync::atomic::AtomicU64,
    }

    pub(super) fn system_notifier(on_click: ClickHandler) -> Arc<dyn Notifier> {
        if !is_bundled() {
            return Arc::new(NullNotifier);
        }
        let _ = CLICK.set(on_click);
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc().set_ivars(());
            // SAFETY: NSObject's `init` has the standard signature.
            unsafe { msg_send![super(this), init] }
        };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The center holds its delegate weakly: keep it alive for the life of the process.
        std::mem::forget(delegate);
        let authorized = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &authorized,
        );
        Arc::new(NativeNotifier {
            serial: std::sync::atomic::AtomicU64::new(1),
        })
    }

    impl Notifier for NativeNotifier {
        fn notify(&self, notification: &Notification) {
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notification.title));
            content.setBody(&NSString::from_str(&notification.body));
            content.setSound(Some(&UNNotificationSound::defaultSound()));
            let serial = self
                .serial
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let id = NSString::from_str(&identifier(notification.file_id, serial));
            let request =
                UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
            UNUserNotificationCenter::currentNotificationCenter()
                .addNotificationRequest_withCompletionHandler(&request, None);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn identifiers_round_trip_the_file_id() {
            assert_eq!(
                file_id_from_identifier(&identifier(Some(987), 3)),
                Some(987)
            );
            assert_eq!(file_id_from_identifier(&identifier(Some(0), 1)), Some(0));
            assert_eq!(file_id_from_identifier(&identifier(None, 2)), None);
            assert_eq!(file_id_from_identifier(&identifier(Some(-5), 2)), None);
        }

        #[test]
        fn hostile_identifiers_are_ignored() {
            for bad in [
                "",
                "file:",
                "file:abc:1",
                "file:-1:1",
                "other:5:1",
                "file:99999999999999999999999:1",
            ] {
                assert_eq!(file_id_from_identifier(bad), None, "{bad}");
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::sync::Arc;

    use super::{ClickHandler, Notification, Notifier};

    #[derive(Debug)]
    struct DesktopNotifier;

    pub(super) fn system_notifier(_on_click: ClickHandler) -> Arc<dyn Notifier> {
        // Clicks are not reported on Windows and Linux yet: the notification only informs.
        Arc::new(DesktopNotifier)
    }

    impl Notifier for DesktopNotifier {
        fn notify(&self, notification: &Notification) {
            let mut toast = notify_rust::Notification::new();
            toast
                .appname("put.io adder")
                .summary(&notification.title)
                .body(&notification.body);
            let _ = toast.show();
        }
    }
}
