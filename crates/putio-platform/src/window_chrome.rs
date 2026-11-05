//! Keeps the window's content steady while its size changes (macOS).
//!
//! When a window is resized, the system shows the last rendered frame until the next one arrives,
//! and by default it *stretches* that frame to the new size. For an animated resize that is a visible
//! wobble: the header is squashed for a frame, and the new area is black. Here the frame is instead
//! pinned to the top-left corner at its own size (nothing is scaled), and the window behind it is
//! painted in the app's background colour, so the only thing that changes is how much of the window
//! is covered.
//!
//! Other platforms do not stretch like this, so there it does nothing.
//!
//! One subtlety found by looking at the frames: the window's view keeps its layers in *flipped* geometry
//! (origin at the top), and Core Animation then reads "top" and "bottom" the other way round for the
//! layer inside it. "topLeft" put the content at the visual bottom (a gap above the header while the
//! window grew); "bottomLeft" puts it at the top. The code asks each layer whether an ancestor is
//! flipped instead of assuming.

/// Apply to every window of the app. Call it on the main thread, after the first frame has been drawn
/// (the rendering layer does not exist before), and again when the background colour changes.
/// Returns whether anything was done.
#[must_use]
pub fn calm_live_resize(background: [u8; 3]) -> bool {
    imp::calm(background)
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Objective-C messages; every block carries a SAFETY comment
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, class, msg_send};
    use objc2_app_kit::NSApplication;
    use objc2_foundation::{NSArray, NSString};

    /// Core Animation's names for "keep the contents at their own size in this corner". Which one is
    /// the visual top-left depends on whether the layer sits inside flipped geometry (see the module
    /// documentation).
    const GRAVITY_TOP_LEFT: &str = "topLeft";
    const GRAVITY_BOTTOM_LEFT: &str = "bottomLeft";
    /// Layers are nested a few deep (view layer, then the one the renderer draws into).
    const MAX_DEPTH: usize = 4;

    pub(super) fn calm(background: [u8; 3]) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: `windows` is a plain getter that returns an NSArray of NSWindow, on the main thread
        // (checked above).
        let windows: Retained<NSArray<AnyObject>> = unsafe { msg_send![&app, windows] };
        let mut done = false;
        for index in 0..windows.count() {
            let window = windows.objectAtIndex(index);
            // SAFETY: `window` is a live NSWindow (it came from the application's window list) and
            // every message below is a documented NSWindow / NSView / CALayer / NSColor method with
            // the argument types used here, sent on the main thread.
            unsafe {
                let [red, green, blue] = background.map(|c| f64::from(c) / 255.0);
                let colour: Retained<AnyObject> = msg_send![
                    class!(NSColor),
                    colorWithSRGBRed: red,
                    green: green,
                    blue: blue,
                    alpha: 1.0_f64
                ];
                let _: () = msg_send![&window, setBackgroundColor: &*colour];
                let content: Option<Retained<AnyObject>> = msg_send![&window, contentView];
                let layer: Option<Retained<AnyObject>> =
                    content.and_then(|view| msg_send![&view, layer]);
                if let Some(layer) = layer {
                    pin_to_top_left(&layer, false, 0);
                    done = true;
                }
            }
        }
        done
    }

    /// # Safety
    /// `layer` must be a live `CALayer` and this must run on the main thread.
    unsafe fn pin_to_top_left(layer: &AnyObject, flipped_above: bool, depth: usize) {
        // SAFETY: the caller guarantees `layer` is a live `CALayer`; `isGeometryFlipped` returns a BOOL.
        let flipped: bool = unsafe { msg_send![layer, isGeometryFlipped] };
        let gravity = NSString::from_str(if flipped_above {
            GRAVITY_BOTTOM_LEFT
        } else {
            GRAVITY_TOP_LEFT
        });
        // SAFETY: the caller guarantees `layer` is a live CALayer; `setContentsGravity:` takes an NSString.
        let _: () = unsafe { msg_send![layer, setContentsGravity: &*gravity] };
        if depth >= MAX_DEPTH {
            return;
        }
        // SAFETY: `sublayers` returns nil or an NSArray of CALayer.
        let sublayers: Option<Retained<NSArray<AnyObject>>> =
            unsafe { msg_send![layer, sublayers] };
        if let Some(sublayers) = sublayers {
            for index in 0..sublayers.count() {
                let sublayer = sublayers.objectAtIndex(index);
                // SAFETY: each element is a live CALayer, and we are on the main thread.
                unsafe { pin_to_top_left(&sublayer, flipped_above || flipped, depth + 1) };
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn calm(_background: [u8; 3]) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_the_main_thread_it_does_nothing() {
        // The test harness runs tests on worker threads, where AppKit must not be touched.
        assert!(!calm_live_resize([255, 255, 255]));
    }
}
