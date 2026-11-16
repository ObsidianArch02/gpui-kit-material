use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
    sync::atomic::{AtomicU64, Ordering},
};

use gpui::{A11ySubtreeBuilder, Role, SharedString, accesskit::{Action, Node, NodeId, Rect, TextDirection}};
use unicode_segmentation::UnicodeSegmentation;

use super::{
    accessibility_layout::{AccessibleGlyph, inline_accessibility_id},
    accessibility_selection::{AccessibleOwner, AccessibleSelection, PublishedRun, PublishedText},
    document::ParsedDocument,
    node::{BlockNode, NodeContext, Paragraph},
};

static DOCUMENT_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(super) struct AccessibleDocument {
    pub(super) generation: u64,
    nodes: Vec<(Node, Vec<usize>)>,
    roots: Vec<usize>,
    pub(super) text: String,
    pub(super) texts: Vec<AccessibleText>,
    pub(super) owners: Vec<AccessibleOwner>,
    pub(super) grapheme_boundaries: Vec<usize>,
    block_ranges: Vec<(Range<usize>, usize)>,
    flow: usize,
    objects: Vec<AccessibleObject>,
}

pub(super) struct AccessibleText {
    node: usize,
    pub(super) range: Range<usize>,
    pub(super) owner: Option<usize>,
    pub(super) owner_offset: usize,
    flow: usize,
}

struct AccessibleObject {
    node: usize,
    identity: SharedString,
}

#[derive(Clone, PartialEq)]
struct TextKind {
    url: Option<String>,
}

impl TextKind {
    fn plain() -> Self {
        Self { url: None }
    }
}

impl AccessibleDocument {
    pub(super) fn new(document: &ParsedDocument, context: &NodeContext) -> Self {
        let mut this = Self {
            generation: DOCUMENT_GENERATION.fetch_add(1, Ordering::Relaxed),
            nodes: Vec::new(),
            roots: Vec::new(),
            text: String::new(),
            texts: Vec::new(),
            owners: Vec::new(),
            grapheme_boundaries: Vec::new(),
            block_ranges: Vec::new(),
            flow: 0,
            objects: Vec::new(),
        };
        for (index, block) in document.blocks.iter().enumerate() {
            let start = this.text.len();
            let roots = this.blocks(std::slice::from_ref(block), context);
            this.roots.extend(roots);
            this.block_ranges.push((start..this.text.len(), index));
        }
        this.grapheme_boundaries = this.text.grapheme_indices(true).map(|(index, _)| index)
            .chain(std::iter::once(this.text.len())).collect();
        this
    }

    pub(super) fn block_at(&self, offset: usize) -> Option<usize> {
        self.block_ranges.iter().find(|(range, _)| range.contains(&offset))
            .or_else(|| self.block_ranges.last().filter(|(range, _)| range.end == offset))
            .map(|(_, index)| *index)
    }

    pub(super) fn clear_geometry(&self) {
        for owner in &self.owners {
            if let AccessibleOwner::Inline { state, .. } = owner
                && let Ok(mut state) = state.lock()
            {
                state.accessibility_glyphs.clear();
            }
        }
    }

    pub(super) fn publish(
        &self,
        builder: &mut A11ySubtreeBuilder,
        scale: f32,
        selection: Option<AccessibleSelection>,
        selectable: bool,
    ) -> PublishedText {
        let mut publication = PublishedText {
            root: builder.parent_id(),
            generation: self.generation,
            runs: Vec::new(),
            links: Vec::new(),
        };
        let visual_roots = builder.parent_node().children().to_vec();
        let mut visual_nodes = HashMap::new();
        let mut visual_order = Vec::new();
        collect_visual_nodes(builder, &visual_roots, &mut visual_nodes, &mut visual_order);
        let mut identities = HashMap::<String, VecDeque<NodeId>>::new();
        for &id in &visual_order {
            let node = &visual_nodes[&id];
            if let Some(identity) = node.author_id() {
                identities.entry(identity.to_string()).or_default().push_back(id);
            }
        }
        let mut ids = (0..self.nodes.len())
            .map(|index| builder.synthetic_node_id(("text-view-node", self.generation, index)))
            .collect::<Vec<_>>();
        let mut adopted = HashSet::new();
        let mut run_parents = HashMap::new();
        let mut object_bounds = HashMap::new();
        for object in &self.objects {
            let Some(id) = identities.get_mut(object.identity.as_ref()).and_then(VecDeque::pop_front) else {
                continue;
            };
            ids[object.node] = id;
            adopted.insert(id);
            let representative = object_representative(id, &visual_nodes).unwrap_or(id);
            run_parents.insert(object.node, representative);
            if let Some(bounds) = visual_nodes.get(&representative).and_then(|node| node.bounds())
                .or_else(|| visual_nodes.get(&id).and_then(|node| node.bounds()))
            {
                object_bounds.insert(object.node, bounds);
            }
            if let Some(node) = builder.node_mut(id) {
                if representative != id {
                    node.clear_label();
                    node.clear_value();
                } else if node.role() == Role::GenericContainer {
                    node.set_role(self.nodes[object.node].0.role());
                    if let Some(value) = self.nodes[object.node].0.value() {
                        node.set_value(value);
                    }
                    if let Some(label) = self.nodes[object.node].0.label() {
                        node.set_label(label);
                    }
                }
            }
        }
        for (id, node) in &visual_nodes {
            let children = node.children().iter().copied()
                .filter(|child| !adopted.contains(child)).collect::<Vec<_>>();
            if children.len() != node.children().len() {
                builder.set_node_children(*id, children);
            }
        }
        for (index, (template, children)) in self.nodes.iter().enumerate() {
            if adopted.contains(&ids[index]) {
                continue;
            }
            let mut node = template.clone();
            node.set_children(children.iter().map(|&child| ids[child]).collect::<Vec<_>>());
            if node.role() == Role::Link {
                node.add_action(Action::Click);
                if let Some(url) = node.url() {
                    publication.links.push((ids[index], SharedString::from(url.to_string())));
                }
            }
            builder.push_child(ids[index], node);
        }
        let mut line_runs: Vec<(NodeId, Option<Rect>, usize, bool)> = Vec::new();
        for text in &self.texts {
            if let Some(&parent) = run_parents.get(&text.node) {
                let native_runs = native_text_runs(builder, parent);
                if !native_runs.is_empty() {
                    let mut native_offset = 0usize;
                    let matches_value = native_runs.iter().map(|(_, node)| {
                        node.value().unwrap_or_default()
                    }).collect::<String>() == &self.text[text.range.clone()];
                    for (id, node) in native_runs {
                        let lengths = node.character_lengths();
                        let (range, offsets) = if matches_value {
                            let start = text.range.start + native_offset;
                            let mut offsets = vec![start];
                            for &length in lengths {
                                native_offset += usize::from(length);
                                offsets.push(text.range.start + native_offset);
                            }
                            (start..text.range.start + native_offset, offsets)
                        } else {
                            let mut offsets = vec![text.range.start; lengths.len() + 1];
                            if let Some(last) = offsets.last_mut() {
                                *last = text.range.end;
                            }
                            (text.range.clone(), offsets)
                        };
                        publication.runs.push(PublishedRun {
                            id,
                            range,
                            offsets,
                            atomic: !matches_value,
                        });
                        line_runs.push((id, None, text.flow, true));
                    }
                    continue;
                }
            }
            let value = &self.text[text.range.clone()];
            let object_bounds = object_bounds.get(&text.node).copied();
            let glyphs = self.glyphs(text);
            let glyphs = glyphs.iter().map(|glyph| ((glyph.range.start, glyph.range.end), glyph))
                .collect::<HashMap<_, _>>();
            let word_starts = value.split_word_bound_indices().map(|(index, _)| index).collect::<HashSet<_>>();
            let units = text_units(value);
            let mut start = 0;
            let mut previous_geometry: Option<&AccessibleGlyph> = None;
            for index in 0..units.len() {
                let unit = &units[index];
                let geometry = glyphs.get(&(unit.start, unit.end)).copied();
                let split = index > start && (
                    index - start == usize::from(u8::MAX)
                    || !same_geometry_line(previous_geometry, geometry)
                    || value[units[index - 1].clone()].ends_with('\n')
                );
                if split {
                    self.publish_run(builder, text, &units[start..index], &glyphs, &word_starts, scale,
                        ids[text.node], run_parents.get(&text.node).copied(), &mut publication,
                        &mut line_runs, object_bounds);
                    start = index;
                }
                previous_geometry = geometry;
            }
            if start < units.len() {
                self.publish_run(builder, text, &units[start..], &glyphs, &word_starts, scale,
                    ids[text.node], run_parents.get(&text.node).copied(), &mut publication,
                    &mut line_runs, object_bounds);
            }
        }
        let mut text_rows = Vec::<(usize, Rect)>::new();
        for (_, bounds, flow, object) in &line_runs {
            if !object && let Some(bounds) = bounds
                && !text_rows.iter().any(|(row_flow, row)| *row_flow == *flow && same_line_rect(*row, *bounds))
            {
                text_rows.push((*flow, *bounds));
            }
        }
        for (_, bounds, flow, object) in &mut line_runs {
            if *object && let Some(object_bounds) = bounds {
                if let Some((_, row)) = text_rows
                    .iter()
                    .filter(|(row_flow, row)| {
                        *row_flow == *flow
                            && row.y1.min(object_bounds.y1) > row.y0.max(object_bounds.y0)
                    })
                    .max_by(|(_, a), (_, b)| {
                        let overlap_a = a.y1.min(object_bounds.y1) - a.y0.max(object_bounds.y0);
                        let overlap_b = b.y1.min(object_bounds.y1) - b.y0.max(object_bounds.y0);
                        overlap_a.total_cmp(&overlap_b)
                    })
                {
                    *bounds = Some(*row);
                }
            }
        }
        for pair in line_runs.windows(2) {
            if let (Some(left), Some(right)) = (pair[0].1, pair[1].1)
                && pair[0].2 == pair[1].2
                && same_line_rect(left, right)
            {
                if let Some(node) = builder.node_mut(pair[0].0) {
                    node.set_next_on_line(pair[1].0);
                }
                if let Some(node) = builder.node_mut(pair[1].0) {
                    node.set_previous_on_line(pair[0].0);
                }
            }
        }
        for id in &ids {
            if adopted.contains(id) { continue }
            let bounds = builder.node(*id).into_iter().flat_map(|node| node.children())
                .filter_map(|child| builder.node(*child).and_then(|node| node.bounds()))
                .reduce(|left, right| Rect::new(left.x0.min(right.x0), left.y0.min(right.y0),
                    left.x1.max(right.x1), left.y1.max(right.y1)));
            if let Some(bounds) = bounds
                && let Some(node) = builder.node_mut(*id)
            {
                node.set_bounds(bounds);
            }
        }
        let mut children = self.roots.iter().map(|&index| ids[index]).collect::<Vec<_>>();
        children.extend(visual_roots.into_iter().filter(|id| !adopted.contains(id)));
        let root = builder.parent_node();
        root.set_children(children);
        root.clear_text_selection();
        if selectable {
            root.add_action(Action::SetTextSelection);
            root.add_action(Action::Focus);
            if let Some(selection) = selection.and_then(|selection| publication.selection(selection)) {
                root.set_text_selection(selection);
            }
        }
        publication
    }

    fn publish_run(
        &self,
        builder: &mut A11ySubtreeBuilder,
        text: &AccessibleText,
        units: &[Range<usize>],
        glyphs: &HashMap<(usize, usize), &AccessibleGlyph>,
        word_starts: &HashSet<usize>,
        scale: f32,
        parent: NodeId,
        adopted_parent: Option<NodeId>,
        publication: &mut PublishedText,
        line_runs: &mut Vec<(NodeId, Option<Rect>, usize, bool)>,
        object_bounds: Option<Rect>,
    ) {
        let start = units[0].start;
        let end = units[units.len() - 1].end;
        let global_start = text.range.start + start;
        let global_end = text.range.start + end;
        let id = builder.synthetic_node_id(("text-view-run", self.generation, global_start));
        let mut node = Node::new(Role::TextRun);
        node.set_value(&self.text[global_start..global_end]);
        node.set_character_lengths(units.iter().map(|unit| (unit.end - unit.start) as u8)
            .collect::<Vec<_>>());
        node.set_word_starts(units.iter().enumerate()
            .filter(|(_, unit)| word_starts.contains(&unit.start))
            .map(|(index, _)| index as u8).collect::<Vec<_>>());
        let geometry = units.iter().map(|unit| glyphs.get(&(unit.start, unit.end)).copied())
            .collect::<Option<Vec<_>>>();
        let mut line_bounds = object_bounds;
        if let Some(bounds) = object_bounds {
            node.set_bounds(bounds);
        }
        if let Some(glyphs) = geometry {
            let bounds = glyphs.iter().fold(None::<Rect>, |bounds, glyph| {
                let rect = physical_rect(glyph, scale);
                Some(bounds.map_or(rect, |bounds| Rect::new(
                    bounds.x0.min(rect.x0), bounds.y0.min(rect.y0),
                    bounds.x1.max(rect.x1), bounds.y1.max(rect.y1),
                )))
            });
            if let Some(bounds) = bounds {
                let direction = glyphs[0].direction;
                node.set_bounds(bounds);
                node.set_text_direction(direction);
                node.set_character_positions(glyphs.iter().map(|glyph| {
                    let rect = physical_rect(glyph, scale);
                    (match direction {
                        TextDirection::LeftToRight => rect.x0 - bounds.x0,
                        TextDirection::RightToLeft => bounds.x1 - rect.x1,
                        TextDirection::TopToBottom => rect.y0 - bounds.y0,
                        TextDirection::BottomToTop => bounds.y1 - rect.y1,
                    }) as f32
                }).collect::<Vec<_>>());
                node.set_character_widths(glyphs.iter().map(|glyph| {
                    let rect = physical_rect(glyph, scale);
                    (match direction {
                        TextDirection::LeftToRight | TextDirection::RightToLeft => rect.x1 - rect.x0,
                        TextDirection::TopToBottom | TextDirection::BottomToTop => rect.y1 - rect.y0,
                    }) as f32
                }).collect::<Vec<_>>());
                line_bounds = Some(bounds);
            }
        }
        line_runs.push((id, line_bounds, text.flow, object_bounds.is_some()));
        builder.push_child(id, node);
        let parent = adopted_parent.unwrap_or(parent);
        if let Some(node) = builder.node(parent) {
            let mut children = node.children().to_vec();
            children.push(id);
            builder.set_node_children(parent, children);
        }
        publication.runs.push(PublishedRun {
            id,
            range: global_start..global_end,
            offsets: units.iter().map(|unit| text.range.start + unit.start)
                .chain(std::iter::once(global_end)).collect(),
            atomic: false,
        });
    }

    pub(super) fn glyphs(&self, text: &AccessibleText) -> Vec<AccessibleGlyph> {
        let Some(AccessibleOwner::Inline { state, .. }) = text.owner.and_then(|index| self.owners.get(index)) else {
            return Vec::new();
        };
        let Ok(state) = state.lock() else {
            return Vec::new();
        };
        let end = text.owner_offset + text.range.len();
        let boundary = self.text.get(text.range.clone()) == Some("\n");
        state.accessibility_glyphs.iter().filter(|glyph| {
            glyph.range.start >= text.owner_offset && glyph.range.end <= end
        }).map(|glyph| AccessibleGlyph {
            range: if boundary && glyph.range.is_empty() && glyph.range.start == text.owner_offset {
                0..1
            } else {
                glyph.range.start - text.owner_offset..glyph.range.end - text.owner_offset
            },
            bounds: glyph.bounds,
            direction: glyph.direction,
        }).collect()
    }

    fn push(&mut self, node: Node, children: Vec<usize>) -> usize {
        let index = self.nodes.len();
        self.nodes.push((node, children));
        index
    }

    fn container(&mut self, role: Role, children: Vec<usize>) -> usize {
        self.push(Node::new(role), children)
    }

    fn owner(&mut self, owner: AccessibleOwner) -> usize {
        let index = self.owners.len();
        self.owners.push(owner);
        index
    }

    fn blocks(&mut self, blocks: &[BlockNode], context: &NodeContext) -> Vec<usize> {
        let mut output = Vec::new();
        for block in blocks {
            match block {
                BlockNode::Root { children, .. } | BlockNode::Surface { children, .. } => {
                    output.extend(self.blocks(children, context));
                }
                BlockNode::Blockquote { children, .. } => {
                    let children = self.blocks(children, context);
                    output.push(self.container(Role::Blockquote, children));
                }
                BlockNode::List { children, ordered, start, .. } => {
                    let mut items = Vec::new();
                    for (position, item) in children.iter().enumerate() {
                        let marker = if *ordered {
                            format!("{}. ", super::utils::ordered_list_ordinal(*start, position))
                        } else {
                            "\u{2022} ".to_string()
                        };
                        let marker = self.text_nodes(&marker, &[], None, 0);
                        let marker = self.container(Role::ListMarker, marker);
                        let mut content = self.blocks(std::slice::from_ref(item), context);
                        for &index in &content {
                            self.nodes[index].0.set_position_in_set(position + 1);
                            self.nodes[index].0.set_size_of_set(children.len());
                        }
                        if let Some(&index) = content.first() {
                            self.nodes[index].1.insert(0, marker);
                        }
                        items.append(&mut content);
                    }
                    let mut node = Node::new(Role::List);
                    node.set_size_of_set(items.len());
                    output.push(self.push(node, items));
                }
                BlockNode::ListItem { children, checked, .. } => {
                    let children = self.blocks(children, context);
                    let mut node = Node::new(Role::ListItem);
                    if let Some(checked) = checked {
                        node.set_toggled(if *checked { gpui::Toggled::True } else { gpui::Toggled::False });
                    }
                    output.push(self.push(node, children));
                }
                BlockNode::Paragraph(paragraph) => {
                    let children = self.paragraph(paragraph, "\n", context);
                    output.push(self.container(Role::Paragraph, children));
                }
                BlockNode::Heading { level, children, .. } => {
                    let children = self.paragraph(children, "\n", context);
                    let mut node = Node::new(Role::Heading);
                    node.set_level(usize::from(*level));
                    output.push(self.push(node, children));
                }
                BlockNode::Table(table) => {
                    let mut rows = Vec::new();
                    for (row_index, row) in table.children.iter().enumerate() {
                        let mut cells = Vec::new();
                        for (column_index, cell) in row.children.iter().enumerate() {
                            let separator = if column_index + 1 == row.children.len() { "\n" } else { "\t" };
                            let children = self.paragraph(&cell.children, separator, context);
                            let mut node = Node::new(Role::Cell);
                            node.set_row_index(row_index);
                            node.set_column_index(column_index);
                            cells.push(self.push(node, children));
                        }
                        rows.push(self.container(Role::Row, cells));
                    }
                    let mut node = Node::new(Role::Table);
                    node.set_row_count(rows.len());
                    node.set_column_count(table.children.iter().map(|row| row.children.len()).max().unwrap_or(0));
                    output.push(self.push(node, rows));
                }
                BlockNode::CodeBlock(code) => {
                    self.flow += 1;
                    let text = code.code();
                    let owner = self.owner(AccessibleOwner::Inline {
                        state: code.accessibility_state(), text: text.clone(),
                    });
                    let mut children = self.text_nodes(&text, &[], Some(owner), 0);
                    children.extend(self.text_nodes("\n", &[], Some(owner), text.len()));
                    output.push(self.container(Role::Code, children));
                }
                BlockNode::Custom { node, state } => {
                    self.flow += 1;
                    let owner = self.owner(AccessibleOwner::BlockObject { state: state.clone() });
                    output.push(self.object(node.accessibility_name(), Role::Label,
                        state.accessibility_id(), owner));
                    output.extend(self.text_nodes("\n", &[], None, 0));
                }
                BlockNode::Break { .. } => output.extend(self.text_nodes("\n", &[], None, 0)),
                BlockNode::HorizontalRule { .. } => output.push(self.container(Role::Splitter, Vec::new())),
                BlockNode::Definition { .. } | BlockNode::Unknown => {}
            }
        }
        output
    }

    fn object(&mut self, label: &str, role: Role, identity: SharedString, owner: usize) -> usize {
        let mut node = Node::new(role);
        if role == Role::Label {
            node.set_value(label);
        } else {
            node.set_label(label);
        }
        let index = self.push(node, Vec::new());
        let start = self.text.len();
        self.text.push_str(label);
        self.texts.push(AccessibleText {
            node: index, range: start..self.text.len(), owner: Some(owner), owner_offset: 0, flow: self.flow,
        });
        self.objects.push(AccessibleObject { node: index, identity });
        index
    }

    fn paragraph(&mut self, paragraph: &Paragraph, separator: &str, context: &NodeContext) -> Vec<usize> {
        self.flow += 1;
        let mut output = Vec::new();
        let mut text = String::new();
        let mut spans = Vec::new();
        for child in &paragraph.children {
            if child.as_custom().is_some() || child.as_image().is_some() {
                if !text.is_empty() {
                    let owner = self.owner(AccessibleOwner::Inline {
                        state: child.state.clone(), text: SharedString::from(text.clone()),
                    });
                    output.extend(self.text_nodes(&text, &spans, Some(owner), 0));
                    text.clear();
                    spans.clear();
                }
                let (label, role) = if let Some(custom) = child.as_custom() {
                    (custom.accessibility_name(), Role::Label)
                } else if let Some(image) = child.as_image() {
                    (image.alt.as_deref().or(image.title.as_deref()).unwrap_or_default(), Role::Image)
                } else {
                    unreachable!()
                };
                let owner = self.owner(AccessibleOwner::InlineObject { node: child.clone() });
                output.push(self.object(label, role, inline_accessibility_id(&child.state), owner));
                continue;
            }
            let start = text.len();
            text.push_str(child.text());
            for (range, mark) in child.mark_runs() {
                let Some(link) = &mark.link else { continue };
                if range.start > range.end || range.end > child.text().len()
                    || !child.text().is_char_boundary(range.start)
                    || !child.text().is_char_boundary(range.end)
                {
                    continue;
                }
                let link = link.identifier.as_ref().and_then(|id| context.link_refs.get(id)).unwrap_or(link);
                spans.push((start + range.start..start + range.end,
                    TextKind { url: Some(link.url.to_string()) }));
            }
        }
        let owner = self.owner(AccessibleOwner::Inline {
            state: paragraph.state.clone(), text: SharedString::from(text.clone()),
        });
        if !text.is_empty() {
            output.extend(self.text_nodes(&text, &spans, Some(owner), 0));
        }
        output.extend(self.text_nodes(separator, &[], Some(owner), text.len()));
        output
    }

    fn text_nodes(&mut self, text: &str, spans: &[(Range<usize>, TextKind)], owner: Option<usize>, owner_offset: usize) -> Vec<usize> {
        let mut output = Vec::new();
        let mut start = 0;
        let mut kind = TextKind::plain();
        for (offset, _) in text.grapheme_indices(true) {
            let next = spans.iter().rev().find(|(range, _)| range.contains(&offset))
                .map(|(_, kind)| kind.clone()).unwrap_or_else(TextKind::plain);
            if next != kind && start < offset {
                output.push(self.text_group(&text[start..offset], &kind, owner, owner_offset + start));
                start = offset;
            }
            kind = next;
        }
        if start < text.len() {
            output.push(self.text_group(&text[start..], &kind, owner, owner_offset + start));
        }
        output
    }

    fn text_group(&mut self, text: &str, kind: &TextKind, owner: Option<usize>, owner_offset: usize) -> usize {
        let mut node = Node::new(Role::Label);
        node.set_value(text);
        let node = self.push(node, Vec::new());
        let start = self.text.len();
        self.text.push_str(text);
        self.texts.push(AccessibleText { node, range: start..self.text.len(), owner, owner_offset, flow: self.flow });
        if let Some(url) = &kind.url {
            let mut link = Node::new(Role::Link);
            link.set_url(url.clone());
            self.push(link, vec![node])
        } else {
            node
        }
    }
}

fn collect_visual_nodes(builder: &A11ySubtreeBuilder, roots: &[NodeId], nodes: &mut HashMap<NodeId, Node>, order: &mut Vec<NodeId>) {
    for &id in roots {
        if nodes.contains_key(&id) { continue }
        if let Some(node) = builder.node(id) {
            nodes.insert(id, node.clone());
            order.push(id);
            collect_visual_nodes(builder, node.children(), nodes, order);
        }
    }
}

fn native_text_runs(builder: &A11ySubtreeBuilder, root: NodeId) -> Vec<(NodeId, Node)> {
    let mut runs = Vec::new();
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = builder.node(id) else {
            continue;
        };
        if node.role() == Role::TextRun {
            if let Some(value) = node.value()
                && !value.is_empty()
                && node.character_lengths().iter().map(|length| usize::from(*length)).sum::<usize>() == value.len()
            {
                runs.push((id, node.clone()));
            }
        } else {
            pending.extend(node.children().iter().rev().copied());
        }
    }
    runs
}

fn object_representative(id: NodeId, nodes: &HashMap<NodeId, Node>) -> Option<NodeId> {
    let node = nodes.get(&id)?;
    if node.role() == Role::TextRun {
        return None;
    }
    if node.role() != Role::GenericContainer {
        return Some(id);
    }
    node.children().iter().find_map(|&child| object_representative(child, nodes))
}

fn text_units(text: &str) -> Vec<Range<usize>> {
    let mut units = Vec::new();
    for (start, grapheme) in text.grapheme_indices(true) {
        if u8::try_from(grapheme.len()).is_ok() {
            units.push(start..start + grapheme.len());
        } else {
            units.extend(grapheme.char_indices().map(|(offset, ch)| start + offset..start + offset + ch.len_utf8()));
        }
    }
    units
}

fn same_geometry_line(left: Option<&AccessibleGlyph>, right: Option<&AccessibleGlyph>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.direction == right.direction
            && left.bounds.top() == right.bounds.top()
            && left.bounds.bottom() == right.bounds.bottom(),
        _ => false,
    }
}

fn physical_rect(glyph: &AccessibleGlyph, scale: f32) -> Rect {
    physical_bounds(glyph.bounds, scale)
}

fn physical_bounds(bounds: gpui::Bounds<gpui::Pixels>, scale: f32) -> Rect {
    Rect::new(
        f64::from(f32::from(bounds.left()) * scale),
        f64::from(f32::from(bounds.top()) * scale),
        f64::from(f32::from(bounds.right()) * scale),
        f64::from(f32::from(bounds.bottom()) * scale),
    )
}

fn same_line_rect(left: Rect, right: Rect) -> bool {
    (left.y0 - right.y0).abs() <= 1.0 && (left.y1 - right.y1).abs() <= 1.0
}
