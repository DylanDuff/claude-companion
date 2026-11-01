use gpui::{
    AnyElement, App, Context, EventEmitter, FontWeight, SharedString, Window, div, prelude::*, px,
};

use crate::{
    icon::{Icon, IconButton},
    segmented::Segmented,
    settings::{Choice, Settings},
    switch::Switch,
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

    /// A listener that writes the picked value into one field of
    /// [`Settings`] and emits the change.
    fn on_change<T: Copy + 'static>(
        apply: impl Fn(&mut Settings, T) + 'static,
        cx: &mut Context<Self>,
    ) -> impl Fn(&T, &mut Window, &mut App) + 'static {
        cx.listener(move |this, value: &T, _, cx| {
            this.change(|settings| apply(settings, *value), cx)
        })
    }

    /// A row with its label above a full-width segmented control.
    fn choice<T: Choice>(
        label: &'static str,
        value: T,
        apply: impl Fn(&mut Settings, T) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(7.))
            .px_4()
            .py(px(10.))
            .child(row_label(label))
            .child(Segmented::new(label, value, Self::on_change(apply, cx)))
            .into_any_element()
    }

    /// A row with a label and description on the left and a switch on the
    /// right.
    fn toggle(
        label: &'static str,
        description: &'static str,
        value: bool,
        apply: impl Fn(&mut Settings, bool) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .px_4()
            .py(px(10.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(row_label(label))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::muted_text())
                            .child(description),
                    ),
            )
            .child(Switch::new(label, value, Self::on_change(apply, cx)))
            .into_any_element()
    }
}

fn row_label(label: &'static str) -> impl IntoElement {
    div()
        .text_size(px(13.5))
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

/// A titled card of rows separated by hairlines, with an optional note
/// under it.
fn section(
    title: &'static str,
    rows: Vec<AnyElement>,
    note: Option<AnyElement>,
) -> impl IntoElement {
    let rows = rows.into_iter().enumerate().map(|(index, row)| {
        div()
            .when(index > 0, |this| {
                this.border_t_1().border_color(theme::window_border())
            })
            .child(row)
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .px_1()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted_text())
                .child(title.to_uppercase()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(14.))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::window_border())
                .children(rows),
        )
        .children(note.map(|note| div().px_1().text_size(px(12.)).child(note)))
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = &self.settings;
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .pt_2()
            .pb_1()
            .child(
                IconButton::new("settings-back", Icon::ChevronLeft)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Closed))),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Settings"),
            );

        let claude = section(
            "Claude",
            vec![
                Self::choice(
                    "Model",
                    settings.model,
                    |settings, value| settings.model = value,
                    cx,
                ),
                Self::choice(
                    "Effort",
                    settings.effort,
                    |settings, value| settings.effort = value,
                    cx,
                ),
            ],
            Some(
                div()
                    .text_color(theme::muted_text())
                    .child("Changes apply once any reply finishes; the conversation carries on.")
                    .into_any_element(),
            ),
        );
        let web = section(
            "Web",
            vec![
                Self::toggle(
                    "Search the web",
                    "Let Claude look things up",
                    settings.web_search,
                    |settings, value| settings.web_search = value,
                    cx,
                ),
                Self::toggle(
                    "Read web pages",
                    "Open the pages it finds or you link",
                    settings.web_fetch,
                    |settings, value| settings.web_fetch = value,
                    cx,
                ),
            ],
            None,
        );
        let window = section(
            "Window",
            vec![
                Self::choice(
                    "Shortcut",
                    settings.hotkey,
                    |settings, value| settings.hotkey = value,
                    cx,
                ),
                Self::choice(
                    "Start a new chat after",
                    settings.session_timeout,
                    |settings, value| settings.session_timeout = value,
                    cx,
                ),
                Self::toggle(
                    "Hide when clicking away",
                    "Once a chat has started; before that it always hides",
                    settings.hide_on_blur,
                    |settings, value| settings.hide_on_blur = value,
                    cx,
                ),
            ],
            self.notice.clone().map(|notice| {
                div()
                    .text_color(theme::error())
                    .child(notice)
                    .into_any_element()
            }),
        );

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .id("settings")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_4()
                    .pt_2()
                    .pb_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(claude)
                    .child(web)
                    .child(window),
            )
    }
}
