use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    ops::Range,
    sync::{Arc, Mutex, OnceLock},
};

use gpui::{
    Bounds, Pixels, SharedString, TextAlign, TextLayout, accesskit::TextDirection, point, size,
};
use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

use super::inline::InlineState;

#[derive(Clone, Debug)]
pub(super) struct AccessibleGlyph {
    pub(super) range: Range<usize>,
    pub(super) bounds: Bounds<Pixels>,
    pub(super) direction: TextDirection,
}

pub(super) fn state_accessibility_id<T>(state: &Arc<Mutex<T>>) -> SharedString {
    static HASHER: OnceLock<RandomState> = OnceLock::new();
    let identity = HASHER.get_or_init(RandomState::new).hash_one(Arc::as_ptr(state));
    format!("text-object-{identity:016x}").into()
}

pub(super) fn inline_accessibility_id(state: &Arc<Mutex<InlineState>>) -> SharedString {
    state_accessibility_id(state)
}

pub(super) fn record_inline_layout(
    state: &Arc<Mutex<InlineState>>,
    selection_source: Option<&(Arc<Mutex<InlineState>>, Range<usize>)>,
    text: &str,
    layout: &TextLayout,
    align: TextAlign,
    selection_bounds: Option<Bounds<Pixels>>,
    visible: Bounds<Pixels>,
) {
    if !is_visible(layout.bounds(), visible) {
        return;
    }
    let (target, range) = match selection_source {
        Some((source, range)) => (source, range.clone()),
        None => (state, 0..text.len()),
    };
    let Ok(mut target) = target.lock() else {
        return;
    };
    if target.text.get(range.clone()) != Some(text) {
        return;
    }
    let bidi = BidiInfo::new(&target.text, None);
    let mut glyphs = layout_glyphs(text, layout, align, visible, &bidi, range.start);
    if let Some(row) = selection_bounds {
        for glyph in &mut glyphs {
            glyph.bounds = Bounds::new(
                point(glyph.bounds.left(), row.top()),
                size(glyph.bounds.size.width, row.size.height),
            );
        }
    }
    target.accessibility_glyphs.retain(|glyph| {
        if glyph.range.is_empty() {
            glyph.range.start < range.start || glyph.range.start > range.end
        } else {
            glyph.range.end <= range.start || glyph.range.start >= range.end
        }
    });
    target.accessibility_glyphs.extend(glyphs.into_iter().map(|mut glyph| {
        glyph.range = range.start + glyph.range.start..range.start + glyph.range.end;
        glyph
    }));
}

pub(super) fn record_fragment_line_break(
    state: &Arc<Mutex<InlineState>>,
    offset: usize,
    row: Bounds<Pixels>,
    visible: Bounds<Pixels>,
) {
    let Ok(mut state) = state.lock() else {
        return;
    };
    if state.text.as_bytes().get(offset) != Some(&b'\n') {
        return;
    }
    let Some(last) = state.accessibility_glyphs.iter()
        .filter(|glyph| glyph.range.end == offset && !glyph.range.is_empty())
        .max_by_key(|glyph| glyph.range.start) else {
        return;
    };
    let direction = last.direction;
    let x = if direction == TextDirection::RightToLeft {
        last.bounds.left()
    } else {
        last.bounds.right()
    };
    let caret = Bounds::new(point(x, row.top()), size(Pixels::ZERO, row.size.height));
    if is_visible(caret, visible) {
        state.accessibility_glyphs.retain(|glyph| glyph.range != (offset..offset + 1));
        state.accessibility_glyphs.push(AccessibleGlyph {
            range: offset..offset + 1,
            bounds: caret,
            direction,
        });
    }
}

pub(super) fn record_empty_flow_line(
    state: &Arc<Mutex<InlineState>>,
    offset: usize,
    bounds: Bounds<Pixels>,
    visible: Bounds<Pixels>,
) {
    if !is_visible(bounds, visible) {
        return;
    }
    let Ok(mut state) = state.lock() else {
        return;
    };
    let end = match state.text.as_bytes().get(offset) {
        Some(b'\n') => offset + 1,
        None if offset == state.text.len() => offset,
        _ => return,
    };
    let bidi = BidiInfo::new(&state.text, None);
    let direction = direction_at(&bidi, offset.saturating_sub(1));
    state.accessibility_glyphs.retain(|glyph| glyph.range != (offset..end));
    state.accessibility_glyphs.push(AccessibleGlyph {
        range: offset..end,
        bounds,
        direction,
    });
}

fn layout_glyphs(
    text: &str,
    layout: &TextLayout,
    align: TextAlign,
    visible: Bounds<Pixels>,
    bidi: &BidiInfo<'_>,
    source_start: usize,
) -> Vec<AccessibleGlyph> {
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    if !f32::from(line_height).is_finite() || line_height <= Pixels::ZERO {
        return Vec::new();
    }
    let glyphs = super::inline::glyph_boxes(layout, align, bounds.size.width);
    let mut output = Vec::new();
    for (start, grapheme) in text.grapheme_indices(true) {
        let range = start..start + grapheme.len();
        let first = glyphs.partition_point(|glyph| glyph.text.end <= range.start);
        let mut hits = glyphs[first..].iter().take_while(|glyph| glyph.text.start < range.end);
        let Some(first) = hits.next() else {
            continue;
        };
        let glyph_bounds = |glyph: &super::inline::GlyphBox| {
            Bounds::new(
                bounds.origin + point(glyph.left.min(glyph.right), line_height * glyph.row as f32),
                size(glyph.right.max(glyph.left) - glyph.left.min(glyph.right), line_height),
            )
        };
        let mut grapheme_bounds = glyph_bounds(first);
        for glyph in hits {
            grapheme_bounds = grapheme_bounds.union(&glyph_bounds(glyph));
        }
        if is_visible(grapheme_bounds, visible) {
            output.push(AccessibleGlyph {
                range,
                bounds: grapheme_bounds,
                direction: direction_at(bidi, source_start + start),
            });
        }
    }
    let mut line_start = 0;
    let mut row = 0;
    for line in layout.line_layouts() {
        let line_end = line_start + line.len();
        row += line.wrap_boundaries.len();
        if text.as_bytes().get(line_end) == Some(&b'\n') || line.len() == 0 {
            let last = glyphs.iter().rev().find(|glyph| glyph.text.end == line_end);
            let direction = direction_at(bidi, source_start + last.map_or(line_start, |glyph| glyph.text.start));
            let x = last.map_or(bounds.left(), |glyph| {
                bounds.left() + if direction == TextDirection::RightToLeft { glyph.left } else { glyph.right }
            });
            let caret = Bounds::new(
                point(x, bounds.top() + line_height * row as f32),
                size(Pixels::ZERO, line_height),
            );
            if is_visible(caret, visible) {
                if let Some(previous) = output.iter_mut().find(|glyph| {
                    glyph.range.start < line_end && glyph.range.end > line_end
                }) {
                    previous.bounds = previous.bounds.union(&caret);
                } else {
                    output.push(AccessibleGlyph {
                        range: line_end..line_end + usize::from(text.as_bytes().get(line_end) == Some(&b'\n')),
                        bounds: caret,
                        direction,
                    });
                }
            }
        }
        line_start = line_end.saturating_add(1);
        row += 1;
    }
    output.sort_by_key(|glyph| (glyph.range.start, glyph.range.end));
    output
}

fn direction_at(bidi: &BidiInfo<'_>, index: usize) -> TextDirection {
    if bidi.levels.get(index).is_some_and(|level| level.is_rtl()) {
        TextDirection::RightToLeft
    } else {
        TextDirection::LeftToRight
    }
}

fn is_visible(bounds: Bounds<Pixels>, visible: Bounds<Pixels>) -> bool {
    bounds.top() < visible.bottom()
        && bounds.bottom() > visible.top()
        && bounds.left() <= visible.right()
        && bounds.right() >= visible.left()
}
