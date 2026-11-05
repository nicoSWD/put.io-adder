//! The app's icon while it is running (the macOS Dock).
//!
//! A bundle holds one static icon, but the app has a light and a dark look, so while it runs the icon is
//! swapped to match. Windows and Linux set the window icon through the UI toolkit instead.

/// Replace the Dock icon with the image in `png`. Must be called on the main thread (the UI thread on
/// macOS); returns `false` anywhere else, on other platforms, and when the data is not a valid image,
/// leaving the current icon alone.
#[must_use]
pub fn set_dock_icon(png: &[u8]) -> bool {
    imp::set(png)
}

/// Debug builds only: write the image macOS currently holds as the app icon to `path` (TIFF), so a test
/// can check what the Dock will draw. Main thread only.
#[cfg(all(debug_assertions, target_os = "macos"))]
#[must_use]
pub fn dump_dock_icon(path: &std::path::Path) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let Some(image) = NSApplication::sharedApplication(mtm).applicationIconImage() else {
        return false;
    };
    let Some(tiff) = image.TIFFRepresentation() else {
        return false;
    };
    std::fs::write(path, tiff.to_vec()).is_ok()
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // one Objective-C call; its SAFETY comment is below
mod imp {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    pub(super) fn set(png: &[u8]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let data = NSData::with_bytes(png);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return false;
        };
        // SAFETY: called on the main thread (checked above) with a valid, retained NSImage.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn set(_png: &[u8]) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_the_main_thread_it_does_nothing() {
        // Tests run on worker threads.
        assert!(!set_dock_icon(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn garbage_is_refused_rather_than_trusted() {
        assert!(!set_dock_icon(b"not an image"));
        assert!(!set_dock_icon(b""));
    }
}
