use gpui::{App, ElementId, Window, div, prelude::*, px};

use crate::theme;

const WIDTH: f32 = 36.;
const HEIGHT: f32 = 22.;
const INSET: f32 = 2.;

type OnToggle = Box<dyn Fn(&bool, &mut Window, &mut App)>;

/// An on/off switch. `on_toggle` receives the new value.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_toggle: OnToggle,
}

impl Switch {
    pub fn new(
        id: impl Into<ElementId>,
        on: bool,
        on_toggle: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            on,
            on_toggle: Box::new(on_toggle),
        }
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let knob = HEIGHT - INSET * 2.;
        let left = if self.on { WIDTH - knob - INSET } else { INSET };
        let next = !self.on;
        let on_toggle = self.on_toggle;
        div()
            .id(self.id)
            .flex_none()
            .relative()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .rounded_full()
            .cursor_pointer()
            .bg(if self.on {
                theme::accent()
            } else {
                theme::switch_off()
            })
            .on_click(move |_, window, cx| on_toggle(&next, window, cx))
            .child(
                div()
                    .absolute()
                    .top(px(INSET))
                    .left(px(left))
                    .size(px(knob))
                    .rounded_full()
                    .bg(theme::switch_knob())
                    .shadow_sm(),
            )
    }
}
