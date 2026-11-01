use std::rc::Rc;

use gpui::{
    AnyElement, App, Font, FontFeatures, FontStyle, FontWeight, IntoElement, ParentElement,
    RenderOnce, SharedString, Styled, TextRun, Window, div, px,
};

use crate::{
    markdown::Document,
    markdown_view::{CodeCopy, MarkdownView},
    selection::Selectable,
    state::Role,
    theme,
};

/// One transcript entry. User messages sit right in a filled bubble as plain
/// text; replies render as Markdown and errors as plain text, full width.
#[derive(IntoElement)]
pub struct MessageBubble {
    role: Role,
    content: AnyElement,
}

impl MessageBubble {
    pub fn text(role: Role, text: impl Into<SharedString>, mut selectable: Selectable) -> Self {
        let text = text.into();
        let run = TextRun {
            len: text.len(),
            font: Font {
                family: ".SystemUIFont".into(),
                features: FontFeatures::default(),
                fallbacks: None,
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
            },
            color: match role {
                Role::Error => theme::error().into(),
                _ => theme::text().into(),
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        Self {
            role,
            content: selectable.text(text, vec![run]).into_any_element(),
        }
    }

    pub fn markdown(
        id: impl Into<SharedString>,
        document: Rc<Document>,
        selectable: Selectable,
        code_copy: CodeCopy,
    ) -> Self {
        Self {
            role: Role::Assistant,
            content: MarkdownView::new(id, document, selectable, code_copy).into_any_element(),
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
