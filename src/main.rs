mod claude;
mod companion;
mod highlight;
mod hotkey;
mod icon;
mod markdown;
mod markdown_view;
mod message_bubble;
mod panel;
mod segmented;
mod settings;
mod settings_panel;
mod state;
mod storage;
mod text_input;
mod theme;

use gpui::{
    App, AppContext, Application, Bounds, KeyBinding, WindowBackgroundAppearance, WindowBounds,
    WindowKind, WindowOptions, actions, px, size,
};

use crate::{companion::Companion, settings::Settings};

actions!(app, [Quit]);

fn main() {
    Application::new()
        .with_assets(icon::Assets)
        .run(|cx: &mut App| {
            hide_from_dock();
            highlight::preload();

            cx.bind_keys(text_input::key_bindings());
            cx.bind_keys(companion::key_bindings());
            cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
            cx.on_action(|_: &Quit, cx| cx.quit());

            let settings = Settings::load();
            let hotkey = settings.hotkey;
            let window = cx
                .open_window(window_options(cx), |window, cx| {
                    cx.new(|cx| Companion::new(settings, window, cx))
                })
                .expect("failed to open the companion window");
            window
                .update(cx, |companion, window, cx| {
                    panel::configure(window);
                    companion.show(window, cx);
                })
                .ok();

            hotkey::init(hotkey, window, cx);
        });
}

fn window_options(cx: &App) -> WindowOptions {
    // Starts at the expanded size; the companion shrinks it to the compact
    // bar on first render when there's no conversation, keeping the top edge.
    let bounds = Bounds::centered(None, size(px(440.), px(540.)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // No title bar; `panel::configure` also drops the frame entirely.
        titlebar: None,
        kind: WindowKind::PopUp,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Blurred,
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
