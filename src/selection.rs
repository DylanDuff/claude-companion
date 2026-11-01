//! Text selection across the transcript. gpui 0.2.2 has no selectable text,
//! so every piece of rendered text registers its layout here each frame;
//! mouse handling hit-tests those layouts, and rendering paints the selected
//! part of each.

use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{Bounds, Hsla, Pixels, Point, SharedString, StyledText, TextLayout, TextRun};

/// A position in the transcript: a message, and a byte offset into that
/// message's selectable text (its text pieces laid end to end, see
/// [`Selectable::base`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPoint {
    pub message: usize,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: TextPoint,
    pub head: TextPoint,
}

impl Selection {
    pub fn at(point: TextPoint) -> Self {
        Self {
            anchor: point,
            head: point,
        }
    }

    pub fn range(&self) -> Range<TextPoint> {
        if self.anchor <= self.head {
            self.anchor..self.head
        } else {
            self.head..self.anchor
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

struct Piece {
    message: usize,
    /// Offset of this piece within its message's selectable text.
    base: usize,
    text: SharedString,
    layout: TextLayout,
}

/// The text pieces drawn in the current frame, in reading order, and the
/// areas drawn over text that must not start a selection (pinned code
/// headers). Rebuilt by every render and the prepaint that follows, and
/// hit-tested between frames, after layout, so every registered layout has
/// bounds.
#[derive(Clone, Default)]
pub struct TextRegistry(Rc<RefCell<Vec<Piece>>>, Rc<RefCell<Vec<Bounds<Pixels>>>>);

impl TextRegistry {
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
        self.1.borrow_mut().clear();
    }

    /// Marks an area drawn on top of text, so presses there don't select
    /// the text underneath.
    pub fn block(&self, bounds: Bounds<Pixels>) {
        self.1.borrow_mut().push(bounds);
    }

    /// The text point under `position`, if it's over visible text.
    pub fn hit(&self, position: Point<Pixels>) -> Option<TextPoint> {
        if self
            .1
            .borrow()
            .iter()
            .any(|bounds| bounds.contains(&position))
        {
            return None;
        }
        self.0.borrow().iter().find_map(|piece| {
            if !piece.layout.bounds().contains(&position) {
                return None;
            }
            let index = piece
                .layout
                .index_for_position(position)
                .unwrap_or_else(|closest| closest);
            Some(TextPoint {
                message: piece.message,
                offset: piece.base + index.min(piece.text.len()),
            })
        })
    }

    /// The text point nearest `position`, for extending a selection while
    /// dragging over gaps between pieces: the piece on that line if any,
    /// otherwise the end of the last piece above, or the very start.
    pub fn nearest(&self, position: Point<Pixels>) -> Option<TextPoint> {
        let pieces = self.0.borrow();
        let first = pieces.first()?;
        let mut nearest = TextPoint {
            message: first.message,
            offset: first.base,
        };
        for piece in pieces.iter() {
            let bounds = piece.layout.bounds();
            if position.y < bounds.top() {
                break;
            }
            let index = if position.y <= bounds.bottom() {
                piece
                    .layout
                    .index_for_position(position)
                    .unwrap_or_else(|closest| closest)
            } else {
                piece.text.len()
            };
            nearest = TextPoint {
                message: piece.message,
                offset: piece.base + index.min(piece.text.len()),
            };
            if position.y <= bounds.bottom() {
                break;
            }
        }
        Some(nearest)
    }

    /// The selected text, with pieces separated by newlines.
    pub fn text(&self, selection: Range<TextPoint>) -> String {
        let pieces = self.0.borrow();
        let parts: Vec<&str> = pieces
            .iter()
            .filter_map(|piece| {
                let range = local_range(&selection, piece.message, piece.base, piece.text.len())?;
                Some(&piece.text[range])
            })
            .collect();
        parts.join("\n")
    }
}

/// Where a message's text pieces register, and which part of each is
/// selected. Pieces take consecutive offsets in drawing order.
pub struct Selectable {
    message: usize,
    next_base: usize,
    registry: TextRegistry,
    selection: Option<Range<TextPoint>>,
    color: Hsla,
}

impl Selectable {
    pub fn new(
        message: usize,
        registry: TextRegistry,
        selection: Option<Range<TextPoint>>,
        color: Hsla,
    ) -> Self {
        Self {
            message,
            next_base: 0,
            registry,
            selection,
            color,
        }
    }

    /// The registry this message's text registers in.
    pub fn registry(&self) -> &TextRegistry {
        &self.registry
    }

    /// Builds `text` from `runs`, with the selected part highlighted, and
    /// registers it for hit-testing.
    pub fn text(&mut self, text: SharedString, runs: Vec<TextRun>) -> StyledText {
        let base = self.next_base;
        // One past the end, so offsets in adjacent pieces never touch.
        self.next_base += text.len() + 1;
        let selected = self
            .selection
            .as_ref()
            .and_then(|selection| local_range(selection, self.message, base, text.len()));
        let runs = match selected {
            Some(range) => highlight_runs(runs, range, self.color),
            None => runs,
        };
        let styled = StyledText::new(text.clone()).with_runs(runs);
        self.registry.0.borrow_mut().push(Piece {
            message: self.message,
            base,
            text,
            layout: styled.layout().clone(),
        });
        styled
    }
}

/// The part of a piece (at `base`, `len` bytes long, in `message`) inside
/// `selection`, in the piece's own offsets.
fn local_range(
    selection: &Range<TextPoint>,
    message: usize,
    base: usize,
    len: usize,
) -> Option<Range<usize>> {
    if message < selection.start.message || message > selection.end.message {
        return None;
    }
    let start = if message == selection.start.message {
        selection.start.offset.saturating_sub(base)
    } else {
        0
    };
    let end = if message == selection.end.message {
        selection.end.offset.saturating_sub(base).min(len)
    } else {
        len
    };
    let start = start.min(len);
    (start < end).then_some(start..end)
}

/// Splits runs at the edges of `range` and gives the runs inside it a
/// selection background.
fn highlight_runs(runs: Vec<TextRun>, range: Range<usize>, color: Hsla) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut offset = 0;
    for run in runs {
        let run_range = offset..offset + run.len;
        offset = run_range.end;
        let cuts = [
            run_range.start,
            range.start.clamp(run_range.start, run_range.end),
            range.end.clamp(run_range.start, run_range.end),
            run_range.end,
        ];
        for pair in cuts.windows(2) {
            if pair[1] > pair[0] {
                let selected = pair[0] >= range.start && pair[1] <= range.end;
                out.push(TextRun {
                    len: pair[1] - pair[0],
                    background_color: if selected {
                        Some(color)
                    } else {
                        run.background_color
                    },
                    ..run.clone()
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(message: usize, offset: usize) -> TextPoint {
        TextPoint { message, offset }
    }

    #[test]
    fn local_range_clips_to_piece() {
        let selection = point(1, 5)..point(1, 12);
        assert_eq!(local_range(&selection, 1, 0, 8), Some(5..8));
        assert_eq!(local_range(&selection, 1, 9, 10), Some(0..3));
        assert_eq!(local_range(&selection, 0, 0, 10), None);
    }

    #[test]
    fn local_range_spans_messages() {
        let selection = point(0, 4)..point(2, 3);
        assert_eq!(local_range(&selection, 1, 0, 6), Some(0..6));
        assert_eq!(local_range(&selection, 0, 0, 10), Some(4..10));
        assert_eq!(local_range(&selection, 2, 0, 10), Some(0..3));
    }

    #[test]
    fn highlight_splits_runs() {
        let run = |len| TextRun {
            len,
            font: gpui::font("Menlo"),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let out = highlight_runs(vec![run(5), run(5)], 3..7, gpui::white());
        let lens: Vec<usize> = out.iter().map(|run| run.len).collect();
        let selected: Vec<bool> = out
            .iter()
            .map(|run| run.background_color.is_some())
            .collect();
        assert_eq!(lens, [3, 2, 2, 3]);
        assert_eq!(selected, [false, true, true, false]);
    }

    #[test]
    fn selection_orders_its_ends() {
        let selection = Selection {
            anchor: point(2, 1),
            head: point(0, 3),
        };
        assert_eq!(selection.range(), point(0, 3)..point(2, 1));
    }
}
