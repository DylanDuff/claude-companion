//! The menu bar icon, showing the app is running and offering its commands.
//!
//! gpui has no status bar support, so the item and its menu are AppKit
//! objects. Menu clicks arrive from AppKit outside gpui, so they're sent over
//! a channel to a gpui task that applies them to the companion.

use gpui::{App, WindowHandle};

use crate::companion::Companion;

#[derive(Clone, Copy, Debug)]
enum Command {
    Show,
    NewChat,
    Settings,
    Quit,
}

/// Adds the menu bar icon and starts handling its menu.
#[cfg(target_os = "macos")]
pub fn install(window: WindowHandle<Companion>, cx: &mut App) {
    let (sender, receiver) = smol::channel::unbounded();
    native::install(sender);

    cx.spawn(async move |cx| {
        while let Ok(command) = receiver.recv().await {
            if matches!(command, Command::Quit) {
                cx.update(|cx| cx.quit()).ok();
                continue;
            }
            window
                .update(cx, |companion, window, cx| match command {
                    Command::Show => companion.show(window, cx),
                    Command::NewChat => companion.start_new_chat(window, cx),
                    Command::Settings => companion.open_settings(window, cx),
                    Command::Quit => {}
                })
                .ok();
        }
    })
    .detach();
}

#[cfg(not(target_os = "macos"))]
pub fn install(_window: WindowHandle<Companion>, _cx: &mut App) {}

#[cfg(target_os = "macos")]
mod native {
    use std::sync::OnceLock;

    use cocoa::{
        base::{id, nil},
        foundation::NSString,
    };
    use objc::{
        class,
        declare::ClassDecl,
        msg_send,
        runtime::{Class, Object, Sel, YES},
        sel, sel_impl,
    };

    use super::Command;

    // NSVariableStatusItemLength
    const VARIABLE_LENGTH: f64 = -1.;

    /// Where menu clicks go. Set once, before any menu exists.
    static COMMANDS: OnceLock<smol::channel::Sender<Command>> = OnceLock::new();

    fn send(command: Command) {
        if let Some(sender) = COMMANDS.get() {
            sender.try_send(command).ok();
        }
    }

    extern "C" fn show(_: &Object, _: Sel, _: id) {
        send(Command::Show);
    }

    extern "C" fn new_chat(_: &Object, _: Sel, _: id) {
        send(Command::NewChat);
    }

    extern "C" fn settings(_: &Object, _: Sel, _: id) {
        send(Command::Settings);
    }

    extern "C" fn quit(_: &Object, _: Sel, _: id) {
        send(Command::Quit);
    }

    /// An Objective-C object whose methods the menu items call.
    fn target_class() -> &'static Class {
        let mut decl = ClassDecl::new("ClaudeCompanionMenuTarget", class!(NSObject))
            .expect("menu target class is declared once");
        unsafe {
            decl.add_method(
                sel!(showCompanion:),
                show as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(sel!(newChat:), new_chat as extern "C" fn(&Object, Sel, id));
            decl.add_method(
                sel!(openSettings:),
                settings as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(quitCompanion:),
                quit as extern "C" fn(&Object, Sel, id),
            );
        }
        decl.register()
    }

    unsafe fn string(text: &str) -> id {
        unsafe { NSString::alloc(nil).init_str(text) }
    }

    unsafe fn menu_item(title: &str, action: Sel, key: &str, target: id) -> id {
        unsafe {
            let item: id = msg_send![class!(NSMenuItem), alloc];
            let item: id = msg_send![item, initWithTitle: string(title) action: action keyEquivalent: string(key)];
            let _: () = msg_send![item, setTarget: target];
            item
        }
    }

    pub(super) fn install(sender: smol::channel::Sender<Command>) {
        if COMMANDS.set(sender).is_err() {
            return;
        }
        unsafe {
            // Lives for the app's lifetime; never released.
            let target: id = msg_send![target_class(), new];

            let menu: id = msg_send![class!(NSMenu), new];
            let _: () = msg_send![menu, setAutoenablesItems: objc::runtime::NO];
            for item in [
                menu_item("Show Companion", sel!(showCompanion:), "", target),
                menu_item("New Chat", sel!(newChat:), "", target),
                menu_item("Settings…", sel!(openSettings:), "", target),
            ] {
                let _: () = msg_send![menu, addItem: item];
            }
            let separator: id = msg_send![class!(NSMenuItem), separatorItem];
            let _: () = msg_send![menu, addItem: separator];
            let quit = menu_item("Quit Claude Companion", sel!(quitCompanion:), "q", target);
            let _: () = msg_send![menu, addItem: quit];

            let status_bar: id = msg_send![class!(NSStatusBar), systemStatusBar];
            let item: id = msg_send![status_bar, statusItemWithLength: VARIABLE_LENGTH];
            // The status bar keeps only a weak reference; retain it so the
            // icon stays for the app's lifetime.
            let _: id = msg_send![item, retain];
            let button: id = msg_send![item, button];
            let image: id = msg_send![
                class!(NSImage),
                imageWithSystemSymbolName: string("sparkle")
                accessibilityDescription: string("Claude Companion")
            ];
            if image != nil {
                // A template image follows the menu bar's light or dark look.
                let _: () = msg_send![image, setTemplate: YES];
                let _: () = msg_send![button, setImage: image];
            } else {
                let _: () = msg_send![button, setTitle: string("✦")];
            }
            let _: () = msg_send![item, setMenu: menu];
        }
    }
}
