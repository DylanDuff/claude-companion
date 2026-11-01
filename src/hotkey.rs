//! System-wide toggle shortcut. GPUI only sees keys while its window is
//! focused, so this uses `global-hotkey` and polls its event channel.

use std::time::Duration;

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use gpui::{App, WindowHandle};

use crate::companion::Companion;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Registers ⌥Space to toggle `window`. Must run on the main thread.
pub fn listen(window: WindowHandle<Companion>, cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(error) => {
            eprintln!("claude-companion: hotkeys unavailable: {error}");
            return;
        }
    };
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::Space);
    if let Err(error) = manager.register(hotkey) {
        eprintln!("claude-companion: couldn't register ⌥Space: {error}");
        return;
    }

    cx.spawn(async move |cx| {
        // The registration lasts only as long as the manager.
        let _manager = manager;
        loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.id == hotkey.id && event.state == HotKeyState::Pressed {
                    window
                        .update(cx, |companion, window, cx| companion.toggle(window, cx))
                        .ok();
                }
            }
        }
    })
    .detach();
}
