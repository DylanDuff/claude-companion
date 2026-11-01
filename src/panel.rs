//! Shows and hides the companion's native panel directly.
//!
//! gpui's `WindowKind::PopUp` is a non-activating `NSPanel`: it can take
//! keyboard focus while another app stays frontmost, as Spotlight does.
//! Ordering the panel in and out (instead of activating or hiding the whole
//! app) avoids the activation handover that made the shortcut unreliable,
//! and returns focus to the previous app on hide.

use gpui::Window;

#[cfg(target_os = "macos")]
fn with_native_window(window: &Window, f: impl FnOnce(*mut objc::runtime::Object)) {
    use objc::{msg_send, runtime::Object, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // gpui's inherent `Window::window_handle` shadows the trait method.
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    let view = handle.ns_view.as_ptr() as *mut Object;
    let native_window: *mut Object = unsafe { msg_send![view, window] };
    if !native_window.is_null() {
        f(native_window);
    }
}

#[cfg(target_os = "macos")]
pub fn show(window: &Window) {
    use objc::{msg_send, runtime::Object, sel, sel_impl};

    with_native_window(window, |native_window| unsafe {
        let _: () = msg_send![native_window, makeKeyAndOrderFront: std::ptr::null_mut::<Object>()];
    });
}

#[cfg(target_os = "macos")]
pub fn hide(window: &Window) {
    use objc::{msg_send, runtime::Object, sel, sel_impl};

    with_native_window(window, |native_window| unsafe {
        let _: () = msg_send![native_window, orderOut: std::ptr::null_mut::<Object>()];
    });
}

#[cfg(not(target_os = "macos"))]
pub fn show(window: &Window) {
    window.activate_window();
}

#[cfg(not(target_os = "macos"))]
pub fn hide(_window: &Window) {}
