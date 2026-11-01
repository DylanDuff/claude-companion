//! A header that stays visible while its block scrolls, like CSS
//! `position: sticky`, which gpui doesn't have.

use gpui::{
    AnyElement, App, Bounds, Display, ElementId, FlexDirection, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Pixels, Style, Window, point, prelude::*, px,
    relative,
};

/// Stacks `header` above `body`. While the block is partly scrolled out of
/// the enclosing scroller, the header is pushed down to the scroller's top
/// edge, stopping at the bottom of the block so it never leaves it. The
/// header is painted after the body, so give it a background to cover the
/// body when pinned.
pub struct StickyHeader {
    header: AnyElement,
    body: AnyElement,
    gap: Pixels,
}

impl StickyHeader {
    pub fn new(header: impl IntoElement, body: impl IntoElement, gap: Pixels) -> Self {
        Self {
            header: header.into_any_element(),
            body: body.into_any_element(),
            gap,
        }
    }
}

impl IntoElement for StickyHeader {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for StickyHeader {
    /// The header's layout, needed to measure it in prepaint.
    type RequestLayoutState = LayoutId;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let header = self.header.request_layout(window, cx);
        let body = self.body.request_layout(window, cx);
        let mut style = Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            ..Style::default()
        };
        style.gap.height = self.gap.into();
        style.size.width = relative(1.).into();
        (window.request_layout(style, [header, body], cx), header)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        header: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let header_bounds = window.layout_bounds(*header);
        // The content mask is the visible area of the enclosing scroller.
        let visible_top = window.content_mask().bounds.top();
        let furthest = (bounds.bottom() - header_bounds.bottom()).max(px(0.));
        let offset = (visible_top - header_bounds.top()).clamp(px(0.), furthest);

        self.body.prepaint(window, cx);
        // Shifting before prepaint moves the header's hitboxes too, so its
        // buttons stay clickable where they're drawn.
        window.with_element_offset(point(px(0.), offset), |window| {
            self.header.prepaint(window, cx);
        });
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _header: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.body.paint(window, cx);
        self.header.paint(window, cx);
    }
}
