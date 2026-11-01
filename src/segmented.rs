use std::rc::Rc;

use gpui::{App, SharedString, Window, div, prelude::*};

use crate::{settings::Choice, theme};

type OnSelect<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

/// A row of buttons for picking one value of a [`Choice`].
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
        let options = T::all().iter().enumerate().map(|(index, &choice)| {
            let selected = choice == self.value;
            let on_select = self.on_select.clone();
            div()
                .id((self.id.clone(), index))
                .px_2()
                .py_1()
                .rounded_md()
                .text_xs()
                .cursor_pointer()
                .map(|this| {
                    if selected {
                        this.bg(theme::accent()).text_color(theme::on_accent())
                    } else {
                        this.bg(theme::surface())
                            .hover(|style| style.bg(theme::user_bubble()))
                    }
                })
                .on_click(move |_, window, cx| on_select(&choice, window, cx))
                .child(choice.label())
        });
        div().flex().flex_wrap().gap_1().children(options)
    }
}
