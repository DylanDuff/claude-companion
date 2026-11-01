//! Renders a parsed [`Document`] as GPUI elements.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClipboardItem, Font, FontFeatures, FontStyle, FontWeight, Hsla,
    InteractiveText, SharedString, StrikethroughStyle, StyledText, TextRun, UnderlineStyle, Window,
    div, prelude::*, px, rgb,
};

use crate::{
    icon::Icon,
    markdown::{Block, Document, Inline, Marks},
    theme,
};

pub const MONOSPACE: &str = "Menlo";
const SYSTEM: &str = ".SystemUIFont";

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
        let mut context = RenderContext {
            prefix: self.id,
            next_id: 0,
        };
        let look = Look {
            color: theme::text().into(),
            weight: FontWeight::NORMAL,
        };
        context.blocks(&self.document.blocks, look)
    }
}

/// Colour and weight that inline text inherits from its block. Text is built
/// from explicit runs (so inline code can switch font), and runs don't
/// inherit these from parent elements the way plain text does.
#[derive(Clone, Copy)]
struct Look {
    color: Hsla,
    weight: FontWeight,
}

/// Hands out element ids for the interactive parts (links, copy buttons,
/// scrolling code), which GPUI needs to be stable across frames.
struct RenderContext {
    prefix: SharedString,
    next_id: usize,
}

impl RenderContext {
    fn id(&mut self) -> (SharedString, usize) {
        self.next_id += 1;
        (self.prefix.clone(), self.next_id)
    }

    fn blocks(&mut self, blocks: &[Block], look: Look) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(blocks.iter().map(|block| self.block(block, look)))
            .into_any_element()
    }

    fn block(&mut self, block: &Block, look: Look) -> AnyElement {
        match block {
            Block::Paragraph(inline) => self.inline(inline, look),
            Block::Heading { level, content } => {
                let (size, line_height) = match level {
                    1 => (22., 30.),
                    2 => (19., 26.),
                    _ => (17., 24.),
                };
                let look = Look {
                    weight: FontWeight::BOLD,
                    ..look
                };
                div()
                    .mt_1()
                    .text_size(px(size))
                    .line_height(px(line_height))
                    .child(self.inline(content, look))
                    .into_any_element()
            }
            Block::Code {
                language,
                code,
                highlights,
            } => self.code(language.as_deref(), code, highlights),
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
                                .min_w(px(16.))
                                .text_color(theme::muted_text())
                                .child(marker),
                        )
                        .child(div().flex_1().min_w_0().child(self.blocks(item, look)))
                }))
                .into_any_element(),
            Block::Quote(blocks) => {
                let look = Look {
                    color: theme::muted_text(),
                    ..look
                };
                div()
                    .border_l_2()
                    .border_color(theme::window_border())
                    .pl_3()
                    .child(self.blocks(blocks, look))
                    .into_any_element()
            }
            Block::Table { header, rows } => self.table(header, rows, look),
            Block::Rule => div()
                .h(px(1.))
                .my_1()
                .bg(theme::window_border())
                .into_any_element(),
        }
    }

    fn code(
        &mut self,
        language: Option<&str>,
        code: &str,
        highlights: &[(std::ops::Range<usize>, u32)],
    ) -> AnyElement {
        let copied = code.to_string();
        let copy = div()
            .id(self.id())
            .flex()
            .items_center()
            .gap_1()
            .px_1()
            .rounded_md()
            .cursor_pointer()
            .hover(|style| style.bg(theme::hover()))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
            })
            .child(Icon::Copy.render(15., theme::text()))
            .child("Copy");
        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(language.unwrap_or("text").to_uppercase())
            .child(copy);
        let styled = StyledText::new(code.to_string()).with_highlights(highlights.iter().map(
            |(range, color)| {
                (
                    range.clone(),
                    gpui::HighlightStyle {
                        color: Some(rgb(*color).into()),
                        ..gpui::HighlightStyle::default()
                    },
                )
            },
        ));
        div()
            .rounded(px(14.))
            .bg(theme::code_background())
            .px_4()
            .pt_3()
            .pb_4()
            .flex()
            .flex_col()
            .gap_2()
            .child(header)
            .child(
                div()
                    .id(self.id())
                    .overflow_x_scroll()
                    .font_family(MONOSPACE)
                    .text_size(px(13.5))
                    .line_height(px(21.))
                    .whitespace_nowrap()
                    .child(styled),
            )
            .into_any_element()
    }

    fn table(&mut self, header: &[Inline], rows: &[Vec<Inline>], look: Look) -> AnyElement {
        let header_look = Look {
            weight: FontWeight::BOLD,
            ..look
        };
        let header_row = self.table_row(header, header_look);
        let body: Vec<AnyElement> = rows
            .iter()
            .map(|cells| {
                self.table_row(cells, look)
                    .border_t_1()
                    .border_color(theme::window_border())
                    .into_any_element()
            })
            .collect();
        div()
            .rounded(px(10.))
            .border_1()
            .border_color(theme::window_border())
            .text_size(px(13.5))
            .line_height(px(20.))
            .flex()
            .flex_col()
            .child(header_row.bg(theme::surface()))
            .children(body)
            .into_any_element()
    }

    fn table_row(&mut self, cells: &[Inline], look: Look) -> gpui::Div {
        div().flex().children(
            cells
                .iter()
                .map(|cell| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .px_2()
                        .py_1()
                        .child(self.inline(cell, look))
                })
                .collect::<Vec<_>>(),
        )
    }

    fn inline(&mut self, inline: &Inline, look: Look) -> AnyElement {
        let styled = StyledText::new(inline.text.clone()).with_runs(runs(inline, look));
        if inline.links.is_empty() {
            return styled.into_any_element();
        }
        let ranges = inline
            .links
            .iter()
            .map(|(range, _)| range.clone())
            .collect();
        let urls: Vec<String> = inline.links.iter().map(|(_, url)| url.clone()).collect();
        InteractiveText::new(self.id(), styled)
            .on_click(ranges, move |index, _, cx| cx.open_url(&urls[index]))
            .into_any_element()
    }
}

/// Covers the whole text with runs: unmarked gaps in the base look, marked
/// ranges styled by their marks.
fn runs(inline: &Inline, look: Look) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut offset = 0;
    for (range, marks) in &inline.marks {
        if range.start > offset {
            runs.push(run(range.start - offset, Marks::default(), look));
        }
        runs.push(run(range.len(), *marks, look));
        offset = range.end;
    }
    if offset < inline.text.len() {
        runs.push(run(inline.text.len() - offset, Marks::default(), look));
    }
    runs
}

fn run(len: usize, marks: Marks, look: Look) -> TextRun {
    let color = if marks.link {
        theme::link().into()
    } else if marks.code {
        theme::inline_code().into()
    } else {
        look.color
    };
    TextRun {
        len,
        font: Font {
            family: if marks.code { MONOSPACE } else { SYSTEM }.into(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight: if marks.strong {
                FontWeight::BOLD
            } else {
                look.weight
            },
            style: if marks.emphasis {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
        },
        color,
        background_color: None,
        underline: marks.link.then_some(UnderlineStyle {
            thickness: px(1.),
            color: None,
            wavy: false,
        }),
        strikethrough: marks.strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.),
            color: None,
        }),
    }
}
