mod claude;
mod companion;
mod hotkey;
mod message_bubble;
mod state;
mod text_input;
mod theme;

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, TitlebarOptions, WindowBounds, WindowKind,
    WindowOptions, actions, px, size,
};

use crate::companion::Companion;

actions!(app, [Quit]);

fn main() {
    Application::new().run(|cx: &mut App| {
        hide_from_dock();

        cx.bind_keys(text_input::key_bindings());
        cx.bind_keys(companion::key_bindings());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let window = cx
            .open_window(window_options(cx), |window, cx| {
                cx.new(|cx| Companion::new(window, cx))
            })
            .expect("failed to open the companion window");
        window
            .update(cx, |companion, window, cx| companion.show(window, cx))
            .ok();

        hotkey::listen(window, cx);
    });
}

fn window_options(cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(480.), px(560.)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Claude Companion".into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    }
}

/// Runs as an accessory app: no Dock icon or menu bar, as `LSUIElement` would
/// give a bundled build, but also when launched with `cargo run`.
#[cfg(target_os = "macos")]
fn hide_from_dock() {
    use objc::{class, msg_send, runtime::BOOL, runtime::Object, sel, sel_impl};

    // NSApplicationActivationPolicyAccessory
    const ACCESSORY: isize = 1;
    unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: BOOL = msg_send![app, setActivationPolicy: ACCESSORY];
    }
}

#[cfg(not(target_os = "macos"))]
fn hide_from_dock() {}
