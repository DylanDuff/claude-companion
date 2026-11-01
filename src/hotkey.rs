//! System-wide toggle shortcut. GPUI only sees keys while its window is
//! focused, so this uses `global-hotkey` and polls its event channel.

use std::time::Duration;

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use gpui::{App, Global, WindowHandle};

use crate::{
    companion::Companion,
    settings::{self, Choice},
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Keeps the registration alive; it lasts only as long as the manager.
struct Registration {
    manager: GlobalHotKeyManager,
    current: Option<HotKey>,
}

impl Global for Registration {}

/// Starts listening and registers `choice` to toggle `window`. Must run on
/// the main thread.
pub fn init(choice: settings::Hotkey, window: WindowHandle<Companion>, cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(manager) => manager,
        Err(error) => {
            eprintln!("claude-companion: hotkeys unavailable: {error}");
            return;
        }
    };
    cx.set_global(Registration {
        manager,
        current: None,
    });
    if let Err(error) = set(choice, cx) {
        eprintln!("claude-companion: {error}");
    }

    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(POLL_INTERVAL).await;
            // Only one hotkey is ever registered, so any press is ours.
            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.state == HotKeyState::Pressed {
                    window
                        .update(cx, |companion, window, cx| companion.toggle(window, cx))
                        .ok();
                }
            }
        }
    })
    .detach();
}

/// Swaps the registered shortcut for `choice`.
pub fn set(choice: settings::Hotkey, cx: &mut App) -> Result<(), String> {
    let Some(registration) = cx.try_global::<Registration>() else {
        return Err("hotkeys unavailable".into());
    };
    let hotkey = hotkey_for(choice);
    if registration.current == Some(hotkey) {
        return Ok(());
    }
    // Register the new shortcut before dropping the old one, so a failure
    // (e.g. another app owns it) leaves the previous shortcut working.
    let registration = cx.global_mut::<Registration>();
    registration
        .manager
        .register(hotkey)
        .map_err(|error| format!("Couldn't register {}: {error}", choice.label()))?;
    if let Some(previous) = registration.current.replace(hotkey) {
        registration.manager.unregister(previous).ok();
    }
    Ok(())
}

fn hotkey_for(choice: settings::Hotkey) -> HotKey {
    let modifiers = match choice {
        settings::Hotkey::OptionSpace => Modifiers::ALT,
        settings::Hotkey::ControlSpace => Modifiers::CONTROL,
        settings::Hotkey::OptionShiftSpace => Modifiers::ALT | Modifiers::SHIFT,
        settings::Hotkey::CommandShiftSpace => Modifiers::META | Modifiers::SHIFT,
    };
    HotKey::new(Some(modifiers), Code::Space)
}
