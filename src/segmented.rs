use std::rc::Rc;

use gpui::{App, FontWeight, SharedString, Window, div, prelude::*, px};

use crate::{settings::Choice, theme};

type OnSelect<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

/// A full-width track of equal segments for picking one value of a
/// [`Choice`], with the selected segment lifted off the track.
#[derive(IntoElement)]
pub struct Segmented<T: Choice> {
    id: SharedString,
    value: T,
    on_select: OnSelect<T>,
}

impl<T: Choice> Segmented<T> {
    pub fn new(
        id: impl Into<SharedString>,
        value: T,
        on_select: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            value,
            on_select: Rc::new(on_select),
        }
    }
}

impl<T: Choice> RenderOnce for Segmented<T> {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let segments = T::all().iter().enumerate().map(|(index, &choice)| {
            let selected = choice == self.value;
            let on_select = self.on_select.clone();
            div()
                .id((self.id.clone(), index))
                .flex_1()
                .min_w_0()
                .h(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.))
                .text_size(px(12.5))
                .map(|this| {
                    if selected {
                        this.bg(theme::segment_selected())
                            .text_color(theme::text())
                            .font_weight(FontWeight::MEDIUM)
                    } else {
                        this.text_color(theme::muted_text())
                            .cursor_pointer()
                            .hover(|style| style.text_color(theme::text()))
                            .on_click(move |_, window, cx| on_select(&choice, window, cx))
                    }
                })
                .child(choice.label())
        });
        div()
            .flex()
            .w_full()
            .p(px(2.))
            .gap(px(2.))
            .rounded(px(9.))
            .bg(theme::segment_track())
            .children(segments)
    }
}
