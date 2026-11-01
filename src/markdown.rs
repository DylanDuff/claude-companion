//! Parses Markdown into a small block model for rendering. GPUI-free so it
//! can be unit tested; `markdown_view.rs` turns it into elements.

use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    ops::Range,
    rc::Rc,
};

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::highlight::{self, Highlight};

#[derive(Debug, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

#[derive(Debug, PartialEq)]
pub enum Block {
    Paragraph(Inline),
    Heading {
        level: u8,
        content: Inline,
    },
    Code {
        language: Option<String>,
        code: String,
        highlights: Vec<Highlight>,
    },
    List {
        /// The first number of an ordered list; `None` for bullets.
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Quote(Vec<Block>),
    Table {
        header: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    Rule,
}

/// Styles applied to a run of inline text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub link: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Strong,
    Emphasis,
    Code,
    Strikethrough,
    Link,
}

impl Marks {
    fn add(&mut self, mark: Mark) {
        match mark {
            Mark::Strong => self.strong = true,
            Mark::Emphasis => self.emphasis = true,
            Mark::Code => self.code = true,
            Mark::Strikethrough => self.strikethrough = true,
            Mark::Link => self.link = true,
        }
    }
}

/// A run of text with styled ranges. `marks` is sorted and non-overlapping,
/// as GPUI's highlight runs require.
#[derive(Debug, Default, PartialEq)]
pub struct Inline {
    pub text: String,
    pub marks: Vec<(Range<usize>, Marks)>,
    pub links: Vec<(Range<usize>, String)>,
}

/// An inline being built, with marks that may still overlap.
#[derive(Default)]
struct InlineBuilder {
    text: String,
    marks: Vec<(Range<usize>, Mark)>,
    open_marks: Vec<(Mark, usize)>,
    links: Vec<(Range<usize>, String)>,
    open_links: Vec<(String, usize)>,
}

impl InlineBuilder {
    fn open(&mut self, mark: Mark) {
        self.open_marks.push((mark, self.text.len()));
    }

    fn close(&mut self, mark: Mark) {
        if let Some(index) = self.open_marks.iter().rposition(|(open, _)| *open == mark) {
            let (_, start) = self.open_marks.remove(index);
            if start < self.text.len() {
                self.marks.push((start..self.text.len(), mark));
            }
        }
    }

    fn push_marked(&mut self, text: &str, mark: Mark) {
        let start = self.text.len();
        self.text.push_str(text);
        self.marks.push((start..self.text.len(), mark));
    }

    /// Splits overlapping marks at every boundary so each range carries the
    /// combined style of everything covering it.
    fn finish(self) -> Inline {
        let mut boundaries: Vec<usize> = self
            .marks
            .iter()
            .flat_map(|(range, _)| [range.start, range.end])
            .collect();
        boundaries.sort_unstable();
        boundaries.dedup();

        let mut marks: Vec<(Range<usize>, Marks)> = Vec::new();
        for pair in boundaries.windows(2) {
            let segment = pair[0]..pair[1];
            let mut combined = Marks::default();
            for (range, mark) in &self.marks {
                if range.start <= segment.start && segment.end <= range.end {
                    combined.add(*mark);
                }
            }
            if combined == Marks::default() {
                continue;
            }
            match marks.last_mut() {
                Some((previous, previous_marks))
                    if previous.end == segment.start && *previous_marks == combined =>
                {
                    previous.end = segment.end;
                }
                _ => marks.push((segment, combined)),
            }
        }
        Inline {
            text: self.text,
            marks,
            links: self.links,
        }
    }
}

enum Container {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
}

#[derive(Default)]
struct TableBuilder {
    header: Vec<Inline>,
    rows: Vec<Vec<Inline>>,
    row: Vec<Inline>,
}

struct Builder {
    containers: Vec<Container>,
    inline: Option<InlineBuilder>,
    heading: Option<u8>,
    code: Option<(Option<String>, String)>,
    table: Option<TableBuilder>,
}

impl Builder {
    fn new() -> Self {
        Self {
            containers: vec![Container::Root(Vec::new())],
            inline: None,
            heading: None,
            code: None,
            table: None,
        }
    }

    fn inline(&mut self) -> &mut InlineBuilder {
        self.inline.get_or_insert_with(InlineBuilder::default)
    }

    /// Adds a block to the innermost container that holds blocks.
    fn push_block(&mut self, block: Block) {
        match self.containers.last_mut() {
            Some(Container::Root(blocks) | Container::Quote(blocks) | Container::Item(blocks)) => {
                blocks.push(block)
            }
            // Blocks directly inside a list (rare) go into a new item.
            Some(Container::List { items, .. }) => items.push(vec![block]),
            None => {}
        }
    }

    /// Ends the current run of text as a paragraph or heading. Tight list
    /// items have text with no paragraph tags, so this also runs whenever a
    /// new block starts.
    fn flush_inline(&mut self) {
        let Some(inline) = self.inline.take() else {
            return;
        };
        let content = inline.finish();
        let block = match self.heading.take() {
            Some(level) => Block::Heading { level, content },
            None if content.text.trim().is_empty() => return,
            None => Block::Paragraph(content),
        };
        self.push_block(block);
    }

    fn text(&mut self, text: &str) {
        if let Some((_, code)) = self.code.as_mut() {
            code.push_str(text);
        } else {
            self.inline().text.push_str(text);
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.flush_inline(),
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.heading = Some(heading_level(level));
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                self.containers.push(Container::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().map(str::to_string)
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                self.flush_inline();
                self.containers.push(Container::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.flush_inline();
                self.containers.push(Container::Item(Vec::new()));
            }
            Tag::Table(_) => {
                self.flush_inline();
                self.table = Some(TableBuilder::default());
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.row.clear();
                }
            }
            Tag::TableCell => self.inline = Some(InlineBuilder::default()),
            Tag::Emphasis => self.inline().open(Mark::Emphasis),
            Tag::Strong => self.inline().open(Mark::Strong),
            Tag::Strikethrough => self.inline().open(Mark::Strikethrough),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                let inline = self.inline();
                let start = inline.text.len();
                inline.open_links.push((dest_url.to_string(), start));
                inline.open(Mark::Link);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.flush_inline(),
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                if let Some(Container::Quote(blocks)) = self.containers.pop() {
                    self.push_block(Block::Quote(blocks));
                }
            }
            TagEnd::CodeBlock => {
                if let Some((language, mut code)) = self.code.take() {
                    if code.ends_with('\n') {
                        code.pop();
                    }
                    let highlights = highlight::highlight(language.as_deref(), &code);
                    self.push_block(Block::Code {
                        language,
                        code,
                        highlights,
                    });
                }
            }
            TagEnd::Item => {
                self.flush_inline();
                if let Some(Container::Item(blocks)) = self.containers.pop()
                    && let Some(Container::List { items, .. }) = self.containers.last_mut()
                {
                    items.push(blocks);
                }
            }
            TagEnd::List(_) => {
                self.flush_inline();
                if let Some(Container::List { start, items }) = self.containers.pop() {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::TableCell => {
                let cell = self.inline.take().unwrap_or_default().finish();
                if let Some(table) = self.table.as_mut() {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.header = std::mem::take(&mut table.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.push_block(Block::Table {
                        header: table.header,
                        rows: table.rows,
                    });
                }
            }
            TagEnd::Emphasis => self.inline().close(Mark::Emphasis),
            TagEnd::Strong => self.inline().close(Mark::Strong),
            TagEnd::Strikethrough => self.inline().close(Mark::Strikethrough),
            TagEnd::Link | TagEnd::Image => {
                let inline = self.inline();
                inline.close(Mark::Link);
                if let Some((url, start)) = inline.open_links.pop()
                    && start < inline.text.len()
                {
                    let end = inline.text.len();
                    inline.links.push((start..end, url));
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Document {
        // Close anything left open, e.g. while a reply is still streaming.
        if let Some((language, code)) = self.code.take() {
            let highlights = highlight::highlight(language.as_deref(), &code);
            self.push_block(Block::Code {
                language,
                code,
                highlights,
            });
        }
        self.flush_inline();
        while self.containers.len() > 1 {
            match self.containers.pop() {
                Some(Container::Quote(blocks)) => self.push_block(Block::Quote(blocks)),
                Some(Container::Item(blocks)) => {
                    if let Some(Container::List { items, .. }) = self.containers.last_mut() {
                        items.push(blocks);
                    }
                }
                Some(Container::List { start, items }) => {
                    self.push_block(Block::List { start, items })
                }
                _ => {}
            }
        }
        match self.containers.pop() {
            Some(Container::Root(blocks)) => Document { blocks },
            _ => Document::default(),
        }
    }
}

pub fn parse(source: &str) -> Document {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let mut builder = Builder::new();
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(tag) => builder.start(tag),
            Event::End(tag) => builder.end(tag),
            Event::Text(text) => builder.text(&text),
            Event::Code(text) | Event::InlineMath(text) => {
                builder.inline().push_marked(&text, Mark::Code)
            }
            Event::DisplayMath(text) => {
                builder.flush_inline();
                builder.push_block(Block::Code {
                    language: None,
                    code: text.to_string(),
                    highlights: Vec::new(),
                });
            }
            Event::Html(text) | Event::InlineHtml(text) => builder.text(&text),
            Event::SoftBreak => builder.text(" "),
            Event::HardBreak => builder.text("\n"),
            Event::Rule => {
                builder.flush_inline();
                builder.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => builder.text(if checked { "☑ " } else { "☐ " }),
            Event::FootnoteReference(label) => builder.text(&format!("[{label}]")),
        }
    }
    builder.finish()
}

impl Document {
    /// The document as plain prose for reading aloud. Code blocks are left
    /// out, since spoken code is rarely useful.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        append_plain(&self.blocks, &mut out);
        out.trim().to_string()
    }
}

fn append_plain(blocks: &[Block], out: &mut String) {
    for block in blocks {
        match block {
            Block::Paragraph(inline)
            | Block::Heading {
                content: inline, ..
            } => {
                out.push_str(&inline.text);
                out.push_str("\n\n");
            }
            Block::List { items, .. } => {
                for item in items {
                    append_plain(item, out);
                }
            }
            Block::Quote(blocks) => append_plain(blocks, out),
            Block::Table { header, rows } => {
                for cells in std::iter::once(header).chain(rows) {
                    let cells: Vec<&str> = cells.iter().map(|cell| cell.text.as_str()).collect();
                    out.push_str(&cells.join(", "));
                    out.push_str(".\n");
                }
                out.push('\n');
            }
            Block::Code { .. } | Block::Rule => {}
        }
    }
}

/// Parsed documents keyed by source text, so each message is parsed and
/// highlighted once rather than every frame. Entries not requested since
/// the last [`DocumentCache::sweep`] are dropped then.
#[derive(Default)]
pub struct DocumentCache {
    documents: HashMap<u64, Rc<Document>>,
    used: HashSet<u64>,
}

impl DocumentCache {
    pub fn get(&mut self, source: &str) -> Rc<Document> {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let key = hasher.finish();
        self.used.insert(key);
        self.documents
            .entry(key)
            .or_insert_with(|| Rc::new(parse(source)))
            .clone()
    }

    pub fn sweep(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.documents.retain(|key, _| used.contains(key));
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph(document: &Document) -> &Inline {
        match &document.blocks[0] {
            Block::Paragraph(inline) => inline,
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn splits_overlapping_marks() {
        let document = parse("a **bold *both*** c");
        let inline = paragraph(&document);
        assert_eq!(inline.text, "a bold both c");
        let strong = Marks {
            strong: true,
            ..Marks::default()
        };
        let both = Marks {
            strong: true,
            emphasis: true,
            ..Marks::default()
        };
        assert_eq!(inline.marks, vec![(2..7, strong), (7..11, both)]);
    }

    #[test]
    fn records_links() {
        let document = parse("see [docs](https://example.com) now");
        let inline = paragraph(&document);
        assert_eq!(
            inline.links,
            vec![(4..8, "https://example.com".to_string())]
        );
    }

    #[test]
    fn parses_fenced_code() {
        let document = parse("```rust\nfn main() {}\n```");
        let Block::Code { language, code, .. } = &document.blocks[0] else {
            panic!("expected code");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(code, "fn main() {}");
    }

    #[test]
    fn keeps_unclosed_code_while_streaming() {
        let document = parse("Here:\n```py\nprint(1)");
        assert!(
            matches!(document.blocks.last(), Some(Block::Code { code, .. }) if code == "print(1)")
        );
    }

    #[test]
    fn parses_tight_and_nested_lists() {
        let document = parse("1. one\n2. two\n   - inner\n");
        let Block::List { start, items } = &document.blocks[0] else {
            panic!("expected a list");
        };
        assert_eq!(*start, Some(1));
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1][1], Block::List { start: None, .. }));
    }

    #[test]
    fn parses_tables() {
        let document = parse("| a | b |\n|---|---|\n| 1 | 2 |\n");
        let Block::Table { header, rows } = &document.blocks[0] else {
            panic!("expected a table");
        };
        assert_eq!(header.len(), 2);
        assert_eq!(rows[0][1].text, "2");
    }

    #[test]
    fn plain_text_skips_code() {
        let document = parse("# Title\n\nSome **bold** text.\n\n```sh\nls\n```\n- item");
        assert_eq!(document.plain_text(), "Title\n\nSome bold text.\n\nitem");
    }
}
