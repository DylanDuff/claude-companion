//! Native styling, sizing and visibility for the companion's panel.
//!
//! gpui's `WindowKind::PopUp` is a non-activating `NSPanel`: it can take
//! keyboard focus while another app stays frontmost, as Spotlight does.
//! Ordering the panel in and out (instead of activating or hiding the whole
//! app) avoids the activation handover that made the shortcut unreliable,
//! and returns focus to the previous app on hide.

/// Corner radius of the panel, matched by the root element's border.
pub const CORNER_RADIUS: f32 = crate::theme::PANEL_RADIUS;

const PLACEMENT_FILE: &str = "window.json";

/// Where the user left the panel: its bottom-left corner in AppKit screen
/// coordinates. The bottom edge is what stays put as the panel resizes, so
/// it is what gets remembered.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    pub x: f64,
    pub bottom: f64,
}

impl Placement {
    pub fn load() -> Option<Self> {
        crate::storage::load::<Option<Self>>(PLACEMENT_FILE)
    }

    pub fn save(self) {
        if let Err(error) = crate::storage::save(PLACEMENT_FILE, &Some(self)) {
            eprintln!("claude-companion: couldn't save window position: {error}");
        }
    }
}

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
            }
            self.set_corner_radius(CORNER_RADIUS);
        }

        /// Clips the content, blur included, to rounded corners of `radius`.
        pub fn set_corner_radius(self, radius: f32) {
            unsafe {
                let content_view: id = msg_send![self.native_window, contentView];
                round_corners(content_view, radius);
                // gpui puts the blur in a subview; round it too in case the
                // content view's mask doesn't reach it.
                let subviews: id = msg_send![content_view, subviews];
                let count: usize = msg_send![subviews, count];
                for index in 0..count {
                    let subview: id = msg_send![subviews, objectAtIndex: index];
                    round_corners(subview, radius);
                }
                let _: () = msg_send![self.native_window, invalidateShadow];
            }
        }

        /// Brings the app forward, for system dialogs (like the file picker)
        /// that otherwise open without keyboard focus. Showing and hiding
        /// the panel itself never activates the app.
        pub fn activate_app() {
            unsafe {
                let app: id = msg_send![class!(NSApplication), sharedApplication];
                let _: () = msg_send![app, activateIgnoringOtherApps: YES];
            }
        }

        /// Whether the left mouse button is physically down, including when
        /// it was released outside the panel and gpui never saw the release.
        pub fn left_button_down() -> bool {
            let buttons: usize = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
            buttons & 1 == 1
        }

        /// Resizes to `height`, keeping the bottom edge where it is, so the
        /// panel grows upward from where the composer sits (typically near
        /// the bottom of the screen). If that would push the top under the
        /// menu bar, the panel slides down just enough to fit. The lowest it
        /// may sit is the screen's real bottom, matching `snap_target`, so a
        /// panel snapped below the Dock's top isn't pushed up as it grows.
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
                    let whole: NSRect = msg_send![screen, frame];
                    let usable: NSRect = msg_send![screen, visibleFrame];
                    let usable_top = usable.origin.y + usable.size.height;
                    bottom = bottom.min(usable_top - height).max(whole.origin.y);
                }
                let frame = NSRect::new(
                    NSPoint::new(frame.origin.x, bottom),
                    NSSize::new(frame.size.width, height),
                );
                let _: () = msg_send![window, setFrame: frame display: YES];
                let _: () = msg_send![window, invalidateShadow];
            }
        }

        /// Where the panel should snap to: `gap` in from any screen edge it's
        /// within `reach` of or past. The sides and bottom are the screen's
        /// real edges (the panel floats above the Dock); the top stops below
        /// the menu bar. `None` if it's already there.
        pub fn snap_target(self, reach: f64, gap: f64) -> Option<(f64, f64)> {
            unsafe {
                let window = self.native_window;
                let screen: id = msg_send![window, screen];
                if screen == nil {
                    return None;
                }
                let whole: NSRect = msg_send![screen, frame];
                let usable: NSRect = msg_send![screen, visibleFrame];
                let frame: NSRect = msg_send![window, frame];
                let x = super::snap_axis(
                    frame.origin.x,
                    whole.origin.x..whole.origin.x + whole.size.width,
                    frame.size.width,
                    reach,
                    gap,
                );
                let y = super::snap_axis(
                    frame.origin.y,
                    whole.origin.y..usable.origin.y + usable.size.height,
                    frame.size.height,
                    reach,
                    gap,
                );
                (x != frame.origin.x || y != frame.origin.y).then_some((x, y))
            }
        }

        /// The panel's bottom-left corner, in AppKit screen coordinates.
        pub fn origin(self) -> (f64, f64) {
            let frame: NSRect = unsafe { msg_send![self.native_window, frame] };
            (frame.origin.x, frame.origin.y)
        }

        pub fn set_origin(self, x: f64, y: f64) {
            unsafe {
                let _: () = msg_send![self.native_window, setFrameOrigin: NSPoint::new(x, y)];
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

        pub fn placement(self) -> super::Placement {
            let frame: NSRect = unsafe { msg_send![self.native_window, frame] };
            super::Placement {
                x: frame.origin.x,
                bottom: frame.origin.y,
            }
        }

        /// Moves the panel back to a remembered spot, unless no connected
        /// screen shows it any more (e.g. the display it was on is gone),
        /// in which case it stays where gpui placed it.
        pub fn restore(self, placement: super::Placement) {
            unsafe {
                let screens: id = msg_send![class!(NSScreen), screens];
                let count: usize = msg_send![screens, count];
                let visible = (0..count).any(|index| {
                    let screen: id = msg_send![screens, objectAtIndex: index];
                    let area: NSRect = msg_send![screen, visibleFrame];
                    // The composer's corner must land on screen.
                    let (x, y) = (placement.x + 24., placement.bottom + 24.);
                    x >= area.origin.x
                        && x <= area.origin.x + area.size.width
                        && y >= area.origin.y
                        && y <= area.origin.y + area.size.height
                });
                if visible {
                    let origin = NSPoint::new(placement.x, placement.bottom);
                    let _: () = msg_send![self.native_window, setFrameOrigin: origin];
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

    unsafe fn round_corners(view: id, radius: f32) {
        unsafe {
            let _: () = msg_send![view, setWantsLayer: YES];
            let layer: id = msg_send![view, layer];
            if layer != nil {
                let _: () = msg_send![layer, setCornerRadius: radius as f64];
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

    pub fn set_corner_radius(self, _radius: f32) {}

    pub fn activate_app() {}

    pub fn left_button_down() -> bool {
        false
    }

    pub fn set_height(self, _height: f32) {}

    pub fn begin_drag(self) {}

    pub fn snap_target(self, _reach: f64, _gap: f64) -> Option<(f64, f64)> {
        None
    }

    pub fn origin(self) -> (f64, f64) {
        (0., 0.)
    }

    pub fn set_origin(self, _x: f64, _y: f64) {}

    pub fn placement(self) -> Placement {
        Placement { x: 0., bottom: 0. }
    }

    pub fn restore(self, _placement: Placement) {}

    pub fn show(self) {}

    pub fn hide(self) {}
}

/// Snaps one axis of the panel's position. `area` is the usable screen span
/// on that axis and `size` the panel's extent. Within `reach` of an edge's
/// snapped spot (`gap` in from the edge), or past it (e.g. over the Dock or
/// off screen), the position moves to that spot; otherwise it stays.
fn snap_axis(position: f64, area: std::ops::Range<f64>, size: f64, reach: f64, gap: f64) -> f64 {
    let start = area.start + gap;
    let end = (area.end - gap - size).max(start);
    if position <= start + reach {
        start
    } else if position >= end - reach {
        end
    } else {
        position
    }
}

#[cfg(test)]
mod tests {
    use super::snap_axis;

    const AREA: std::ops::Range<f64> = 0.0..1000.0;

    #[test]
    fn snaps_within_reach() {
        assert_eq!(snap_axis(40., AREA, 100., 40., 12.), 12.);
        assert_eq!(snap_axis(860., AREA, 100., 40., 12.), 888.);
    }

    #[test]
    fn snaps_back_from_past_an_edge() {
        // Below the usable area, e.g. dropped over the Dock.
        assert_eq!(snap_axis(-60., AREA, 100., 40., 12.), 12.);
        // Partly off the far edge.
        assert_eq!(snap_axis(950., AREA, 100., 40., 12.), 888.);
    }

    #[test]
    fn leaves_the_middle_alone() {
        assert_eq!(snap_axis(400., AREA, 100., 40., 12.), 400.);
    }
}
