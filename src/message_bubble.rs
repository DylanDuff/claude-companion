use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px};

use crate::{state::Role, theme};

/// One transcript entry. User messages sit right in a filled bubble;
/// replies and errors run full width.
#[derive(IntoElement)]
pub struct MessageBubble {
    role: Role,
    text: SharedString,
}

impl MessageBubble {
    pub fn new(role: Role, text: impl Into<SharedString>) -> Self {
        Self {
            role,
            text: text.into(),
        }
    }
}

impl RenderOnce for MessageBubble {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let row = div().flex().w_full();
        let bubble = div().text_sm().line_height(px(20.)).child(self.text);
        match self.role {
            Role::User => row.justify_end().child(
                bubble
                    .max_w(px(360.))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .bg(theme::user_bubble()),
            ),
            Role::Assistant => row.child(bubble.w_full()),
            Role::Error => row.child(bubble.w_full().text_color(theme::error())),
        }
    }
}
