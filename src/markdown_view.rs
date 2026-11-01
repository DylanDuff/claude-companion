//! Renders a parsed [`Document`] as GPUI elements.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, FontStyle, FontWeight, HighlightStyle, InteractiveText,
    SharedString, StrikethroughStyle, StyledText, UnderlineStyle, Window, div, prelude::*, px, rgb,
};

use crate::{
    markdown::{Block, Document, Inline, Marks},
    theme,
};

const MONOSPACE: &str = "Menlo";

#[derive(IntoElement)]
pub struct MarkdownView {
    /// Prefix for element ids, unique per message.
    id: SharedString,
    document: Rc<Document>,
}

impl MarkdownView {
    pub fn new(id: impl Into<SharedString>, document: Rc<Document>) -> Self {
        Self {
            id: id.into(),
            document,
        }
    }
}

impl RenderOnce for MarkdownView {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut ids = Ids {
            prefix: self.id,
            next: 0,
        };
        render_blocks(&self.document.blocks, &mut ids)
    }
}

/// Hands out element ids for the interactive parts (links, copy buttons,
/// scrolling code), which GPUI needs to be stable across frames.
struct Ids {
    prefix: SharedString,
    next: usize,
}

impl Ids {
    fn next(&mut self) -> (SharedString, usize) {
        self.next += 1;
        (self.prefix.clone(), self.next)
    }
}

fn render_blocks(blocks: &[Block], ids: &mut Ids) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(blocks.iter().map(|block| render_block(block, ids)))
        .into_any_element()
}

fn render_block(block: &Block, ids: &mut Ids) -> AnyElement {
    match block {
        Block::Paragraph(inline) => render_inline(inline, ids),
        Block::Heading { level, content } => {
            let heading = div().font_weight(FontWeight::BOLD);
            match level {
                1 => heading.text_lg(),
                2 => heading.text_base(),
                _ => heading.text_sm(),
            }
            .child(render_inline(content, ids))
            .into_any_element()
        }
        Block::Code {
            language,
            code,
            highlights,
        } => render_code(language.as_deref(), code, highlights, ids),
        Block::List { start, items } => div()
            .flex()
            .flex_col()
            .gap_1()
            .children(items.iter().enumerate().map(|(index, item)| {
                let marker = match start {
                    Some(start) => format!("{}.", start + index as u64),
                    None => "•".to_string(),
                };
                div()
                    .flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(14.))
                            .text_color(theme::muted_text())
                            .child(marker),
                    )
                    .child(div().flex_1().min_w_0().child(render_blocks(item, ids)))
            }))
            .into_any_element(),
        Block::Quote(blocks) => div()
            .border_l_2()
            .border_color(theme::border())
            .pl_3()
            .text_color(theme::muted_text())
            .child(render_blocks(blocks, ids))
            .into_any_element(),
        Block::Table { header, rows } => render_table(header, rows, ids),
        Block::Rule => div()
            .h(px(1.))
            .my_1()
            .bg(theme::border())
            .into_any_element(),
    }
}

fn render_code(
    language: Option<&str>,
    code: &str,
    highlights: &[(std::ops::Range<usize>, u32)],
    ids: &mut Ids,
) -> AnyElement {
    let copied = code.to_string();
    let copy = div()
        .id(ids.next())
        .px_1()
        .rounded_sm()
        .cursor_pointer()
        .hover(|style| style.bg(theme::user_bubble()).text_color(theme::text()))
        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copied.clone())))
        .child("Copy");
    let header = div()
        .flex()
        .justify_between()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(theme::muted_text())
        .border_b_1()
        .border_color(theme::border())
        .child(language.unwrap_or("text").to_string())
        .child(copy);
    let styled = StyledText::new(code.to_string()).with_highlights(highlights.iter().map(
        |(range, color)| {
            (
                range.clone(),
                HighlightStyle {
                    color: Some(rgb(*color).into()),
                    ..HighlightStyle::default()
                },
            )
        },
    ));
    div()
        .rounded_md()
        .border_1()
        .border_color(theme::border())
        .bg(theme::code_background())
        .child(header)
        .child(
            div()
                .id(ids.next())
                .overflow_x_scroll()
                .px_2()
                .py_2()
                .font_family(MONOSPACE)
                .text_xs()
                .line_height(px(18.))
                .whitespace_nowrap()
                .child(styled),
        )
        .into_any_element()
}

fn render_table(header: &[Inline], rows: &[Vec<Inline>], ids: &mut Ids) -> AnyElement {
    let row = |cells: &[Inline], bold: bool, ids: &mut Ids| {
        div().flex().children(cells.iter().map(|cell| {
            div()
                .flex_1()
                .min_w_0()
                .px_2()
                .py_1()
                .when(bold, |this| this.font_weight(FontWeight::BOLD))
                .child(render_inline(cell, ids))
        }))
    };
    div()
        .rounded_md()
        .border_1()
        .border_color(theme::border())
        .text_xs()
        .flex()
        .flex_col()
        .child(row(header, true, ids).bg(theme::code_background()))
        .children(
            rows.iter()
                .map(|cells| {
                    row(cells, false, ids)
                        .border_t_1()
                        .border_color(theme::border())
                })
                .collect::<Vec<_>>(),
        )
        .into_any_element()
}

fn render_inline(inline: &Inline, ids: &mut Ids) -> AnyElement {
    let styled = StyledText::new(inline.text.clone()).with_highlights(
        inline
            .marks
            .iter()
            .map(|(range, marks)| (range.clone(), highlight_style(*marks))),
    );
    if inline.links.is_empty() {
        return styled.into_any_element();
    }
    let ranges = inline
        .links
        .iter()
        .map(|(range, _)| range.clone())
        .collect();
    let urls: Vec<String> = inline.links.iter().map(|(_, url)| url.clone()).collect();
    InteractiveText::new(ids.next(), styled)
        .on_click(ranges, move |index, _, cx| cx.open_url(&urls[index]))
        .into_any_element()
}

fn highlight_style(marks: Marks) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    if marks.strong {
        style.font_weight = Some(FontWeight::BOLD);
    }
    if marks.emphasis {
        style.font_style = Some(FontStyle::Italic);
    }
    if marks.code {
        // Highlights can't switch font family, so inline code is set apart
        // with colour and a background instead.
        style.color = Some(theme::inline_code().into());
        style.background_color = Some(theme::code_background().into());
    }
    if marks.strikethrough {
        style.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        });
    }
    if marks.link {
        style.color = Some(theme::link().into());
        style.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: None,
            wavy: false,
        });
    }
    style
}
