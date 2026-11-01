use gpui::{Context, EventEmitter, SharedString, Window, div, prelude::*, px};

use crate::{
    segmented::Segmented,
    settings::{Choice, Settings},
    theme,
};

pub enum SettingsEvent {
    Changed(Settings),
    Closed,
}

/// Settings view shown in place of the chat. It edits a copy and emits each
/// change; `Companion` owns, saves and applies the real settings.
pub struct SettingsPanel {
    settings: Settings,
    /// A problem applying the last change, e.g. a shortcut already taken.
    notice: Option<SharedString>,
}

impl EventEmitter<SettingsEvent> for SettingsPanel {}

impl SettingsPanel {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            notice: None,
        }
    }

    /// Replaces the displayed settings, e.g. after a change was reverted.
    pub fn reset(
        &mut self,
        settings: Settings,
        notice: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.settings = settings;
        self.notice = notice;
        cx.notify();
    }

    fn change(&mut self, apply: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        apply(&mut self.settings);
        self.notice = None;
        cx.emit(SettingsEvent::Changed(self.settings.clone()));
        cx.notify();
    }

    /// A labelled segmented control bound to one field of [`Settings`].
    fn setting<T: Choice>(
        &self,
        label: &'static str,
        value: T,
        apply: impl Fn(&mut Settings, T) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let on_select = cx.listener(move |this, value: &T, _, cx| {
            this.change(|settings| apply(settings, *value), cx)
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().text_xs().text_color(theme::muted_text()).child(label))
            .child(Segmented::new(label, value, on_select))
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = &self.settings;
        let done = div()
            .id("settings-done")
            .px_3()
            .py_1()
            .rounded_md()
            .text_sm()
            .cursor_pointer()
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed)))
            .child("Done");

        div()
            .id("settings")
            .flex_1()
            .overflow_y_scroll()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.setting(
                "Model",
                settings.model,
                |settings, value| settings.model = value,
                cx,
            ))
            .child(self.setting(
                "Effort",
                settings.effort,
                |settings, value| settings.effort = value,
                cx,
            ))
            .child(self.setting(
                "Search the web",
                settings.web_search,
                |settings, value| settings.web_search = value,
                cx,
            ))
            .child(self.setting(
                "Read web pages",
                settings.web_fetch,
                |settings, value| settings.web_fetch = value,
                cx,
            ))
            .child(self.setting(
                "Start a new chat after idle",
                settings.session_timeout,
                |settings, value| settings.session_timeout = value,
                cx,
            ))
            .child(self.setting(
                "Shortcut",
                settings.hotkey,
                |settings, value| settings.hotkey = value,
                cx,
            ))
            .child(self.setting(
                "Hide when clicking away",
                settings.hide_on_blur,
                |settings, value| settings.hide_on_blur = value,
                cx,
            ))
            .children(
                self.notice
                    .clone()
                    .map(|notice| div().text_xs().text_color(theme::error()).child(notice)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .max_w(px(320.))
                            .text_xs()
                            .text_color(theme::muted_text())
                            .child(
                                "Model, effort and tool changes restart Claude, \
                                 keeping the conversation, once any reply finishes.",
                            ),
                    )
                    .child(done),
            )
    }
}
