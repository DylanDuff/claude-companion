use gpui::{IntoElement, ParentElement, Styled, div, prelude::*, px};

/// Keeps a single-axis scroller to its own axis. By default gpui turns wheel
/// movement along the other axis into scrolling along this one: horizontal
/// swipes scroll a vertical list and vice versa, so nested scrollers fight.
pub fn lock_axis<E: Styled>(mut element: E) -> E {
    element.style().restrict_scroll_to_axis = Some(true);
    element
}

/// Wraps a horizontal scroller so mostly-sideways swipes over it end there.
/// gpui sends a wheel event to every scroller under the pointer, inner first,
/// and trackpad swipes are never perfectly flat, so without this the
/// enclosing vertical list would drift while the inner one scrolls.
pub fn contain_horizontal(scroller: impl IntoElement) -> impl IntoElement {
    div()
        .on_scroll_wheel(|event, _, cx| {
            let delta = event.delta.pixel_delta(px(1.));
            if delta.x.abs() > delta.y.abs() {
                cx.stop_propagation();
            }
        })
        .child(scroller)
}
