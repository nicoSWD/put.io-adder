//! The progress badge on the app icon (macOS Dock).

/// `Some(42)` → `Some("42%")`, `None` clears the badge.
#[must_use]
pub fn label(percent: Option<u8>) -> Option<String> {
    percent.map(|value| format!("{}%", value.min(100)))
}

/// Set or clear the Dock badge. Must be called on the main thread (the UI thread on macOS); from
/// any other thread it does nothing and returns `false`. Other platforms have no Dock badge, so
/// the call is a no-op there (the UI shows progress in the window title instead).
#[must_use]
pub fn set_dock_badge(percent: Option<u8>) -> bool {
    imp::set(label(percent).as_deref())
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    pub(super) fn set(label: Option<&str>) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let tile = NSApplication::sharedApplication(mtm).dockTile();
        let text = label.map(NSString::from_str);
        tile.setBadgeLabel(text.as_deref());
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn set(_label: Option<&str>) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(label(None), None);
        assert_eq!(label(Some(0)).as_deref(), Some("0%"));
        assert_eq!(label(Some(42)).as_deref(), Some("42%"));
        assert_eq!(label(Some(250)).as_deref(), Some("100%"));
    }

    #[test]
    fn off_the_main_thread_it_is_a_harmless_no_op() {
        // Tests run on worker threads.
        assert!(!set_dock_badge(Some(5)));
    }
}
