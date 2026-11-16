use std::{ops::Range, sync::{Arc, Mutex}};

use gpui::{App, Pixels, Point, SharedString, accesskit::{NodeId, TextDirection, TextPosition, TextSelection}};

use super::{
    accessibility::AccessibleDocument,
    inline::InlineState,
    node::{BlockState, InlineNode},
    state::TextViewState,
};

pub(super) enum AccessibleOwner {
    Inline { state: Arc<Mutex<InlineState>>, text: SharedString },
    InlineObject { node: InlineNode },
    BlockObject { state: BlockState },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct AccessibleSelection {
    pub(super) generation: u64,
    pub(super) anchor: usize,
    pub(super) focus: usize,
}

pub(super) struct PublishedRun {
    pub(super) id: NodeId,
    pub(super) range: Range<usize>,
    pub(super) offsets: Vec<usize>,
    pub(super) atomic: bool,
}

pub(super) struct PublishedText {
    pub(super) root: NodeId,
    pub(super) generation: u64,
    pub(super) runs: Vec<PublishedRun>,
    pub(super) links: Vec<(NodeId, SharedString)>,
}

impl PublishedText {
    pub(super) fn selection(&self, selection: AccessibleSelection) -> Option<TextSelection> {
        if selection.generation != self.generation {
            return None;
        }
        Some(TextSelection {
            anchor: self.position(selection.anchor)?,
            focus: self.position(selection.focus)?,
        })
    }

    fn position(&self, offset: usize) -> Option<TextPosition> {
        let run = self.runs.iter().find(|run| run.range.contains(&offset))
            .or_else(|| self.runs.last().filter(|run| run.range.end == offset))?;
        Some(TextPosition {
            node: run.id,
            character_index: if run.atomic {
                if offset == run.range.start {
                    0
                } else if offset == run.range.end {
                    run.offsets.len().checked_sub(1)?
                } else {
                    return None;
                }
            } else {
                run.offsets.binary_search(&offset).ok()?
            },
        })
    }

    pub(super) fn resolve(&self, selection: TextSelection) -> Option<AccessibleSelection> {
        let resolve = |position: TextPosition| {
            let run = self.runs.iter().find(|run| run.id == position.node)?;
            run.offsets.get(position.character_index).copied()
        };
        let mut resolved = AccessibleSelection {
            generation: self.generation,
            anchor: resolve(selection.anchor)?,
            focus: resolve(selection.focus)?,
        };
        if selection.anchor != selection.focus {
            let anchor = self.runs.iter().position(|run| run.id == selection.anchor.node)?;
            let focus = self.runs.iter().position(|run| run.id == selection.focus.node)?;
            let first = &self.runs[anchor];
            let last = &self.runs[focus];
            if first.atomic && last.atomic && first.range == last.range {
                let reversed = anchor > focus
                    || (anchor == focus && selection.anchor.character_index > selection.focus.character_index);
                resolved.anchor = if reversed { first.range.end } else { first.range.start };
                resolved.focus = if reversed { first.range.start } else { first.range.end };
            }
        }
        Some(resolved)
    }
}

impl AccessibleDocument {
    pub(super) fn current_selection(&self, state: &TextViewState, cx: &App) -> Option<AccessibleSelection> {
        if !state.is_selectable() || !state.has_current_copy_document() {
            return None;
        }
        if state.is_all_selected() {
            return Some(AccessibleSelection { generation: self.generation, anchor: 0, focus: self.text.len() });
        }
        if state.preserve_inline_selection
            && let Some(selection) = state.accessibility_selection
            && selection.generation == self.generation
        {
            return Some(selection);
        }
        if !state.has_selection(cx) {
            return None;
        }
        let points = state.selection_points(cx);
        let mut start = None::<usize>;
        let mut end = None::<usize>;
        for text in &self.texts {
            let Some(owner) = text.owner.and_then(|owner| self.owners.get(owner)) else {
                continue;
            };
            let selected = match owner {
                AccessibleOwner::Inline { state, .. } => state.lock().ok().and_then(|state| state.selection)
                    .and_then(|selection| {
                        let source = text.owner_offset..text.owner_offset + text.range.len();
                        let first = selection.start.max(source.start);
                        let last = selection.end.min(source.end);
                        (first <= last && selection.start <= source.end && selection.end >= source.start)
                            .then_some(text.range.start + first - source.start..text.range.start + last - source.start)
                    }),
                AccessibleOwner::InlineObject { node } => node.accessibility_selected().then_some(text.range.clone()),
                AccessibleOwner::BlockObject { state } => (state.accessibility_selected() == Some(true)).then_some(text.range.clone()),
            };
            if let Some(selected) = selected {
                start = Some(start.map_or(selected.start, |start| start.min(selected.start)));
                end = Some(end.map_or(selected.end, |end| end.max(selected.end)));
            }
        }
        let (mut anchor, mut focus) = (start?, end?);
        if let Some((anchor_point, focus_point)) = points {
            let reversed = match (self.position_at_point(anchor_point), self.position_at_point(focus_point)) {
                (Some(anchor), Some(focus)) if anchor != focus => anchor > focus,
                _ => anchor_point.y > focus_point.y
                    || (anchor_point.y == focus_point.y && anchor_point.x > focus_point.x),
            };
            if reversed {
                std::mem::swap(&mut anchor, &mut focus);
            }
        }
        Some(AccessibleSelection { generation: self.generation, anchor, focus })
    }

    fn position_at_point(&self, point: Point<Pixels>) -> Option<usize> {
        let mut nearest = None;
        for text in &self.texts {
            for glyph in self.glyphs(text) {
                if point.y < glyph.bounds.top() || point.y > glyph.bounds.bottom() {
                    continue;
                }
                let (offset, distance) = match glyph.direction {
                    TextDirection::LeftToRight | TextDirection::RightToLeft => {
                        let reversed = glyph.direction == TextDirection::RightToLeft;
                        let before = (point.x < glyph.bounds.center().x) != reversed;
                        let edge = if before != reversed { glyph.bounds.left() } else { glyph.bounds.right() };
                        (if before { glyph.range.start } else { glyph.range.end },
                            (f32::from(point.x - edge)).abs())
                    }
                    TextDirection::TopToBottom | TextDirection::BottomToTop => {
                        let reversed = glyph.direction == TextDirection::BottomToTop;
                        let before = (point.y < glyph.bounds.center().y) != reversed;
                        let edge = if before != reversed { glyph.bounds.top() } else { glyph.bounds.bottom() };
                        (if before { glyph.range.start } else { glyph.range.end },
                            (f32::from(point.y - edge)).abs())
                    }
                };
                if nearest.is_none_or(|(_, previous)| distance < previous) {
                    nearest = Some((text.range.start + offset, distance));
                }
            }
        }
        nearest.map(|(offset, _)| offset)
    }

    pub(super) fn normalize_selection(&self, mut selection: AccessibleSelection) -> Option<AccessibleSelection> {
        if selection.generation != self.generation
            || selection.anchor > self.text.len() || selection.focus > self.text.len()
        {
            return None;
        }
        let snap = |offset, forward| match self.grapheme_boundaries.binary_search(&offset) {
            Ok(_) => offset,
            Err(index) => self.grapheme_boundaries[if forward { index } else { index.saturating_sub(1) }],
        };
        let collapsed = selection.anchor == selection.focus;
        let reversed = selection.anchor > selection.focus;
        selection.anchor = snap(selection.anchor, !collapsed && reversed);
        selection.focus = snap(selection.focus, !collapsed && !reversed);
        for text in &self.texts {
            let Some(owner) = text.owner.and_then(|index| self.owners.get(index)) else {
                continue;
            };
            if matches!(owner, AccessibleOwner::Inline { .. }) {
                continue;
            }
            if collapsed {
                if text.range.start < selection.anchor && selection.anchor < text.range.end {
                    selection.anchor = text.range.start;
                    selection.focus = text.range.start;
                }
            } else {
                if text.range.start < selection.anchor && selection.anchor < text.range.end {
                    selection.anchor = if reversed { text.range.end } else { text.range.start };
                }
                if text.range.start < selection.focus && selection.focus < text.range.end {
                    selection.focus = if reversed { text.range.start } else { text.range.end };
                }
            }
        }
        Some(selection)
    }

    pub(super) fn apply_selection(&self, selection: AccessibleSelection) {
        let first = selection.anchor.min(selection.focus);
        let last = selection.anchor.max(selection.focus);
        let mut ranges = vec![None::<Range<usize>>; self.owners.len()];
        for text in &self.texts {
            let Some(owner_index) = text.owner else { continue };
            let start = first.max(text.range.start);
            let end = last.min(text.range.end);
            if start < end || (first == last && text.range.start <= first && first <= text.range.end) {
                let start = text.owner_offset + start - text.range.start;
                let end = text.owner_offset + end - text.range.start;
                ranges[owner_index] = Some(ranges[owner_index].take()
                    .map_or(start..end, |range| range.start.min(start)..range.end.max(end)));
            }
        }
        for (owner_index, owner) in self.owners.iter().enumerate() {
            let range = ranges[owner_index].take();
            match owner {
                AccessibleOwner::Inline { state, text } => {
                    if let Ok(mut state) = state.lock() {
                        state.text = text.clone();
                        state.selection = range.map(|range| range.start.min(text.len())..range.end.min(text.len()))
                            .map(Into::into);
                        state.selection_rows = None;
                    }
                }
                AccessibleOwner::InlineObject { node } => node.set_accessibility_selected(range.is_some_and(|range| !range.is_empty())),
                AccessibleOwner::BlockObject { state } => state.set_accessibility_selected(range.is_some_and(|range| !range.is_empty())),
            }
        }
    }
}
