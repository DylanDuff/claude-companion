use std::rc::Rc;

use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px,
};

use crate::{markdown::Document, markdown_view::MarkdownView, state::Role, theme};

/// One transcript entry. User messages sit right in a filled bubble as plain
/// text; replies render as Markdown and errors as plain text, full width.
#[derive(IntoElement)]
pub struct MessageBubble {
    role: Role,
    content: AnyElement,
}

impl MessageBubble {
    pub fn text(role: Role, text: impl Into<SharedString>) -> Self {
        Self {
            role,
            content: text.into().into_any_element(),
        }
    }

    pub fn markdown(id: impl Into<SharedString>, document: Rc<Document>) -> Self {
        Self {
            role: Role::Assistant,
            content: MarkdownView::new(id, document).into_any_element(),
        }
    }
}

impl RenderOnce for MessageBubble {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let row = div().flex().w_full();
        let bubble = div()
            .text_size(px(15.))
            .line_height(px(23.))
            .child(self.content);
        match self.role {
            Role::User => row.justify_end().child(
                bubble
                    .max_w(px(320.))
                    .px_4()
                    .py_2()
                    .rounded(px(18.))
                    .bg(theme::user_bubble()),
            ),
            Role::Assistant => row.child(bubble.w_full().min_w_0()),
            Role::Error => row.child(bubble.w_full().text_color(theme::error())),
        }
    }
}
