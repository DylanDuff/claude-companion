//! Native styling, sizing and visibility for the companion's panel.
//!
//! gpui's `WindowKind::PopUp` is a non-activating `NSPanel`: it can take
//! keyboard focus while another app stays frontmost, as Spotlight does.
//! Ordering the panel in and out (instead of activating or hiding the whole
//! app) avoids the activation handover that made the shortcut unreliable,
//! and returns focus to the previous app on hide.

#[cfg(not(target_os = "macos"))]
use gpui::Window;

/// Corner radius of the panel, matched by the root element's border.
pub const CORNER_RADIUS: f32 = 22.;

#[cfg(target_os = "macos")]
mod native {
    use cocoa::{
        base::{id, nil},
        foundation::{NSPoint, NSRect, NSSize},
    };
    use gpui::Window;
    use objc::{msg_send, runtime::YES, sel, sel_impl};

    use super::CORNER_RADIUS;

    // NSWindowStyleMaskBorderless (0) | NSWindowStyleMaskNonactivatingPanel
    const BORDERLESS_PANEL: usize = 1 << 7;

    fn with_native_window(window: &Window, f: impl FnOnce(id)) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        // gpui's inherent `Window::window_handle` shadows the trait method.
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        let view = handle.ns_view.as_ptr() as id;
        let native_window: id = unsafe { msg_send![view, window] };
        if native_window != nil {
            f(native_window);
        }
    }

    /// Drops the title bar (and its traffic lights and frame outline) and
    /// clips the content, blur included, to rounded corners.
    pub fn configure(window: &Window) {
        with_native_window(window, |native_window| unsafe {
            let _: () = msg_send![native_window, setStyleMask: BORDERLESS_PANEL];
            let _: () = msg_send![native_window, setHasShadow: YES];
            let content_view: id = msg_send![native_window, contentView];
            round_corners(content_view);
            // gpui puts the blur in a subview; round it too in case the
            // content view's mask doesn't reach it.
            let subviews: id = msg_send![content_view, subviews];
            let count: usize = msg_send![subviews, count];
            for index in 0..count {
                let subview: id = msg_send![subviews, objectAtIndex: index];
                round_corners(subview);
            }
            let _: () = msg_send![native_window, invalidateShadow];
        });
    }

    unsafe fn round_corners(view: id) {
        unsafe {
            let _: () = msg_send![view, setWantsLayer: YES];
            let layer: id = msg_send![view, layer];
            if layer != nil {
                let _: () = msg_send![layer, setCornerRadius: CORNER_RADIUS as f64];
                let _: () = msg_send![layer, setMasksToBounds: YES];
            }
        }
    }

    /// Resizes to `height`, keeping the top edge where it is (AppKit frames
    /// grow from the bottom-left).
    pub fn set_height(window: &Window, height: f32) {
        with_native_window(window, |native_window| unsafe {
            let frame: NSRect = msg_send![native_window, frame];
            let height = height as f64;
            if (frame.size.height - height).abs() < 0.5 {
                return;
            }
            let top = frame.origin.y + frame.size.height;
            let frame = NSRect::new(
                NSPoint::new(frame.origin.x, top - height),
                NSSize::new(frame.size.width, height),
            );
            let _: () = msg_send![native_window, setFrame: frame display: YES];
            let _: () = msg_send![native_window, invalidateShadow];
        });
    }

    pub fn show(window: &Window) {
        with_native_window(window, |native_window| unsafe {
            let _: () = msg_send![native_window, makeKeyAndOrderFront: nil];
        });
    }

    pub fn hide(window: &Window) {
        with_native_window(window, |native_window| unsafe {
            let _: () = msg_send![native_window, orderOut: nil];
        });
    }
}

#[cfg(target_os = "macos")]
pub use native::{configure, hide, set_height, show};

#[cfg(not(target_os = "macos"))]
pub fn configure(_window: &Window) {}

#[cfg(not(target_os = "macos"))]
pub fn set_height(_window: &Window, _height: f32) {}

#[cfg(not(target_os = "macos"))]
pub fn show(window: &Window) {
    window.activate_window();
}

#[cfg(not(target_os = "macos"))]
pub fn hide(_window: &Window) {}
