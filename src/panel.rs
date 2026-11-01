//! Native styling, sizing and visibility for the companion's panel.
//!
//! gpui's `WindowKind::PopUp` is a non-activating `NSPanel`: it can take
//! keyboard focus while another app stays frontmost, as Spotlight does.
//! Ordering the panel in and out (instead of activating or hiding the whole
//! app) avoids the activation handover that made the shortcut unreliable,
//! and returns focus to the previous app on hide.

/// Corner radius of the panel, matched by the root element's border.
pub const CORNER_RADIUS: f32 = 22.;

#[cfg(target_os = "macos")]
pub use native::Panel;

#[cfg(target_os = "macos")]
mod native {
    use cocoa::{
        base::{id, nil},
        foundation::{NSPoint, NSRect, NSSize},
    };
    use gpui::Window;
    use objc::{class, msg_send, runtime::YES, sel, sel_impl};

    use super::CORNER_RADIUS;

    // NSWindowStyleMaskBorderless (0) | NSWindowStyleMaskNonactivatingPanel
    const BORDERLESS_PANEL: usize = 1 << 7;

    /// The panel's `NSWindow` and gpui's content view. Holding these
    /// directly (rather than going through a `&Window`) lets callers act on
    /// the panel outside gpui window updates: AppKit reports resizes
    /// synchronously, and gpui drops that report if it arrives while the
    /// window is already being updated.
    #[derive(Clone, Copy)]
    pub struct Panel {
        native_window: id,
        view: id,
    }

    impl Panel {
        pub fn new(window: &Window) -> Option<Self> {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};

            // gpui's inherent `Window::window_handle` shadows the trait method.
            let handle = HasWindowHandle::window_handle(window).ok()?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return None;
            };
            let view = handle.ns_view.as_ptr() as id;
            let native_window: id = unsafe { msg_send![view, window] };
            (native_window != nil).then_some(Self {
                native_window,
                view,
            })
        }

        /// Drops the title bar (and its traffic lights and frame outline) and
        /// clips the content, blur included, to rounded corners.
        pub fn configure(self) {
            unsafe {
                let window = self.native_window;
                let _: () = msg_send![window, setStyleMask: BORDERLESS_PANEL];
                let _: () = msg_send![window, setHasShadow: YES];
                let content_view: id = msg_send![window, contentView];
                round_corners(content_view);
                // gpui puts the blur in a subview; round it too in case the
                // content view's mask doesn't reach it.
                let subviews: id = msg_send![content_view, subviews];
                let count: usize = msg_send![subviews, count];
                for index in 0..count {
                    let subview: id = msg_send![subviews, objectAtIndex: index];
                    round_corners(subview);
                }
                let _: () = msg_send![window, invalidateShadow];
            }
        }

        /// Resizes to `height`, keeping the bottom edge where it is, so the
        /// panel grows upward from where the composer sits (typically near
        /// the bottom of the screen). If that would push the top past the
        /// screen's visible area, the panel slides down just enough to fit.
        /// Call it from a spawned task, outside gpui's app borrow, so gpui
        /// sees the new size.
        pub fn set_height(self, height: f32) {
            unsafe {
                let window = self.native_window;
                let frame: NSRect = msg_send![window, frame];
                let height = height as f64;
                if (frame.size.height - height).abs() < 0.5 {
                    return;
                }
                // AppKit frames have a bottom-left origin, so keeping
                // `origin.y` keeps the bottom edge.
                let mut bottom = frame.origin.y;
                let screen: id = msg_send![window, screen];
                if screen != nil {
                    let visible: NSRect = msg_send![screen, visibleFrame];
                    let visible_top = visible.origin.y + visible.size.height;
                    bottom = bottom.min(visible_top - height).max(visible.origin.y);
                }
                let frame = NSRect::new(
                    NSPoint::new(frame.origin.x, bottom),
                    NSSize::new(frame.size.width, height),
                );
                let _: () = msg_send![window, setFrame: frame display: YES];
                let _: () = msg_send![window, invalidateShadow];
            }
        }

        /// Hands the current mouse drag to the window server, which moves
        /// the window until the button is released. gpui's
        /// `start_window_move` is a no-op on macOS.
        pub fn begin_drag(self) {
            unsafe {
                let app: id = msg_send![class!(NSApplication), sharedApplication];
                let event: id = msg_send![app, currentEvent];
                if event != nil {
                    let _: () = msg_send![self.native_window, performWindowDragWithEvent: event];
                }
            }
        }

        pub fn show(self) {
            unsafe {
                // Changing the style mask can leave the window itself as first
                // responder, and then keystrokes never reach gpui.
                let _: () = msg_send![self.native_window, makeFirstResponder: self.view];
                let _: () = msg_send![self.native_window, makeKeyAndOrderFront: nil];
            }
        }

        pub fn hide(self) {
            unsafe {
                let _: () = msg_send![self.native_window, orderOut: nil];
            }
        }
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
}

/// No native panel elsewhere; every operation is a no-op.
#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
pub struct Panel;

#[cfg(not(target_os = "macos"))]
impl Panel {
    pub fn new(_window: &gpui::Window) -> Option<Self> {
        Some(Self)
    }

    pub fn configure(self) {}

    pub fn set_height(self, _height: f32) {}

    pub fn begin_drag(self) {}

    pub fn show(self) {}

    pub fn hide(self) {}
}
