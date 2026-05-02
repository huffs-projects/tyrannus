//! `EditOp` reducer and editor state.

use std::cmp::Ordering;

use crate::cursor::{self, Cursor};
use crate::document::{Block, Document, Inline};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Insert,
}

/// Gap indices in `0..=document_char_count`: caret before char `i`, or after last char.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

#[derive(Clone, Debug, Default)]
pub struct EditorState {
    pub cursor: Cursor,
    pub mode: Mode,
    pub selection: Option<Selection>,
    /// First visible visual line index (`LaidOutLine.row` space).
    pub scroll_top: usize,
}

#[inline]
pub fn selection_ordered(sel: &Selection) -> (usize, usize) {
    if sel.anchor <= sel.head {
        (sel.anchor, sel.head)
    } else {
        (sel.head, sel.anchor)
    }
}

/// Number of characters in flatten order (global indices are `0..n`).
pub fn document_char_count(doc: &Document) -> usize {
    crate::layout::flatten_document_chars(doc).len()
}

/// Map caret to gap index (`0`..`= char_count`).
pub fn cursor_to_gap_index(doc: &Document, cursor: &Cursor) -> Option<usize> {
    let flat = crate::layout::flatten_document_chars(doc);
    let n = flat.len();
    for (i, (c, _)) in flat.iter().enumerate() {
        if c == cursor {
            return Some(i);
        }
    }
    for (i, (c, ch)) in flat.iter().enumerate() {
        let after = crate::layout::advance_cursor_after_char(c, *ch);
        if &after == cursor {
            return Some(i + 1);
        }
    }
    if n == 0 {
        return Some(0);
    }
    None
}

/// Gap `g`: before char `g`, or after the last char when `g == n` and `n > 0`.
pub fn gap_index_to_cursor(doc: &Document, g: usize) -> Option<Cursor> {
    let flat = crate::layout::flatten_document_chars(doc);
    let n = flat.len();
    if g < n {
        Some(flat[g].0.clone())
    } else if g == n && n > 0 {
        let (c, ch) = &flat[n - 1];
        Some(crate::layout::advance_cursor_after_char(c, *ch))
    } else if g == 0 && n == 0 {
        Some(Cursor::new(0, vec![0], 0))
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CursorMove {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditOp {
    InsertChar(char),
    Backspace,
    NewLine,
    InsertBlock(usize, Block),
    DeleteBlock(usize),
    ToggleBold,
    ToggleItalic,
    MoveCursor(CursorMove),
}

fn cmp_cursor(a: &Cursor, b: &Cursor) -> Ordering {
    a.block_index
        .cmp(&b.block_index)
        .then_with(|| a.inline_path.cmp(&b.inline_path))
        .then_with(|| a.offset.cmp(&b.offset))
}

/// Every cursor that points immediately *before* a character in document order.
pub fn document_char_starts(doc: &Document) -> Vec<Cursor> {
    let mut v = Vec::new();
    for (bi, b) in doc.blocks.iter().enumerate() {
        match b {
            Block::Paragraph(il) | Block::Heading { content: il, .. } => {
                v.extend(
                    crate::layout::flatten_inlines_for_edit(bi, il)
                        .into_iter()
                        .map(|(c, _)| c),
                );
            }
            Block::CodeBlock { text, .. } => {
                for (o, _) in text.char_indices() {
                    v.push(Cursor {
                        block_index: bi,
                        inline_path: Vec::new(),
                        offset: o,
                    });
                }
            }
        }
    }
    v
}

pub fn move_cursor_prev_char(doc: &Document, cursor: &mut Cursor) {
    let starts = document_char_starts(doc);
    for w in starts.windows(2) {
        if &w[1] == cursor {
            *cursor = w[0].clone();
            return;
        }
    }
    // cursor might be "end" position — find preceding start and stay, or move to last start <= cursor
    let mut best: Option<Cursor> = None;
    for s in &starts {
        if cmp_cursor(s, cursor) == Ordering::Less {
            best = Some(s.clone());
        }
    }
    if let Some(b) = best {
        // if cursor is strictly after `b` in same leaf, move within leaf
        if cursor.block_index == b.block_index
            && cursor.inline_path == b.inline_path
            && cursor.offset > b.offset
        {
            if let Some(s) = prev_utf8_offset(doc, cursor) {
                *cursor = s;
                return;
            }
        }
        *cursor = b;
    }
}

pub fn move_cursor_next_char(doc: &Document, cursor: &mut Cursor) {
    let starts = document_char_starts(doc);
    for w in starts.windows(2) {
        if &w[0] == cursor {
            *cursor = w[1].clone();
            return;
        }
    }
    for s in &starts {
        if s == cursor {
            if let Some(a) = advance_in_same_leaf(doc, cursor) {
                *cursor = a;
                return;
            }
        }
    }
    if let Some(a) = advance_in_same_leaf(doc, cursor) {
        *cursor = a;
    }
}

fn prev_utf8_offset(doc: &Document, cursor: &Cursor) -> Option<Cursor> {
    let t = leaf_str(doc, cursor)?;
    if cursor.offset == 0 {
        return None;
    }
    let mut i = cursor.offset;
    while i > 0 {
        i -= 1;
        if t.is_char_boundary(i) {
            let mut c = cursor.clone();
            c.offset = i;
            return Some(c);
        }
    }
    None
}

fn advance_in_same_leaf(doc: &Document, cursor: &Cursor) -> Option<Cursor> {
    let t = leaf_str(doc, cursor)?;
    let rest = t.get(cursor.offset..)?;
    let ch = rest.chars().next()?;
    let mut c = cursor.clone();
    c.offset += ch.len_utf8();
    if c.offset > t.len() {
        c.offset = t.len();
    }
    Some(c)
}

fn leaf_str<'a>(doc: &'a Document, cursor: &Cursor) -> Option<&'a str> {
    let b = doc.blocks.get(cursor.block_index)?;
    match b {
        Block::Paragraph(il) | Block::Heading { content: il, .. } => {
            crate::cursor::get_text_at_path(il, &cursor.inline_path)
        }
        Block::CodeBlock { text, .. } => Some(text.as_str()),
    }
}

/// Apply structural/text edit. Returns whether the document changed. Does not handle `MoveCursor`.
fn take_non_empty_selection_range(state: &mut EditorState) -> Option<(usize, usize)> {
    let sel = state.selection.take()?;
    let (lo, hi) = selection_ordered(&sel);
    if lo < hi {
        Some((lo, hi))
    } else {
        state.selection = Some(sel);
        None
    }
}

fn insert_char(doc: &mut Document, state: &mut EditorState, ch: char) -> bool {
    state.cursor.normalize(doc);
    if let Some(s) = inline_mut(doc, &state.cursor) {
        let off = state.cursor.offset.min(s.len());
        s.insert(off, ch);
        let clen = ch.len_utf8();
        state.cursor.offset += clen;
        return true;
    }
    if let Some(s) = code_mut(doc, &state.cursor) {
        let off = state.cursor.offset.min(s.len());
        s.insert(off, ch);
        state.cursor.offset += ch.len_utf8();
        return true;
    }
    false
}

fn backspace(doc: &mut Document, state: &mut EditorState) -> bool {
    state.cursor.normalize(doc);
    if let Some(s) = inline_mut(doc, &state.cursor) {
        if state.cursor.offset > 0 {
            let off = state.cursor.offset;
            let prev = prev_char_boundary(s, off);
            if prev >= off {
                return false;
            }
            s.drain(prev..off);
            state.cursor.offset = prev;
            return true;
        }
    }
    if let Some(s) = code_mut(doc, &state.cursor) {
        if state.cursor.offset > 0 {
            let off = state.cursor.offset;
            let prev = prev_char_boundary(s, off);
            if prev >= off {
                return false;
            }
            s.drain(prev..off);
            state.cursor.offset = prev;
            return true;
        }
    }
    merge_with_previous_block(doc, state)
}

/// Deletes characters with global indices `[lo, hi)`. Cursor ends at gap `lo`.
fn delete_char_gap_range(
    doc: &mut Document,
    state: &mut EditorState,
    lo: usize,
    hi: usize,
) -> bool {
    let mut end = hi;
    while end > lo {
        let c = match gap_index_to_cursor(doc, end) {
            Some(c) => c,
            None => return false,
        };
        state.cursor = c;
        if !backspace(doc, state) {
            return false;
        }
        end -= 1;
    }
    state.cursor = match gap_index_to_cursor(doc, lo) {
        Some(c) => c,
        None => return false,
    };
    true
}

fn prev_char_boundary(s: &str, i: usize) -> usize {
    if i == 0 || i > s.len() {
        return 0;
    }
    let mut prev = 0;
    for (idx, _) in s.char_indices() {
        if idx < i {
            prev = idx;
        } else {
            break;
        }
    }
    prev
}

fn newline(doc: &mut Document, state: &mut EditorState) -> bool {
    state.cursor.normalize(doc);
    let bi = state.cursor.block_index;
    let path = state.cursor.inline_path.clone();
    let off = state.cursor.offset;
    let Some(block) = doc.blocks.get_mut(bi) else {
        return false;
    };
    match block {
        Block::Paragraph(il) => {
            let Some(after) = split_paragraph_il(il, &path, off) else {
                return false;
            };
            doc.blocks.insert(bi + 1, Block::Paragraph(after));
            state.cursor = Cursor::new(bi + 1, vec![0], 0);
            true
        }
        Block::Heading { content, .. } => {
            let Some(after) = split_paragraph_il(content, &path, off) else {
                return false;
            };
            doc.blocks.insert(bi + 1, Block::Paragraph(after));
            state.cursor = Cursor::new(bi + 1, vec![0], 0);
            true
        }
        Block::CodeBlock { text, .. } => {
            let off = off.min(text.len());
            if !text.is_char_boundary(off) {
                return false;
            }
            text.insert(off, '\n');
            state.cursor.offset += 1;
            true
        }
    }
}

/// Split root-level `Text` at `path[0]`; left stays in `il`, returns new paragraph inlines for block below.
fn split_paragraph_il(il: &mut Vec<Inline>, path: &[usize], off: usize) -> Option<Vec<Inline>> {
    if path.len() != 1 {
        return None;
    }
    let i = path[0];
    let Inline::Text(s) = il.get_mut(i)? else {
        return None;
    };
    if !s.is_char_boundary(off) {
        return None;
    }
    let b = s[off..].to_string();
    *s = s[..off].to_string();
    let mut rest: Vec<Inline> = il.drain((i + 1)..).collect();
    let mut after = Vec::new();
    if !b.is_empty() {
        after.push(Inline::Text(b));
    }
    after.append(&mut rest);
    if after.is_empty() {
        after.push(Inline::empty_text());
    }
    Some(after)
}

fn merge_with_previous_block(doc: &mut Document, state: &mut EditorState) -> bool {
    let bi = state.cursor.block_index;
    if bi == 0 {
        return false;
    }

    // Join position is "end of previous block before merge". This avoids landing
    // on appended empty inline nodes, which can produce visually equivalent but
    // non-canonical cursor paths (and make cursor mapping flaky on empty lines).
    let mut join_cursor = Cursor::new(bi - 1, vec![usize::MAX], usize::MAX);
    join_cursor.normalize(doc);

    let cur = doc.blocks.remove(bi);
    if let Block::Paragraph(mut b) = cur {
        if let Block::Paragraph(a) = &mut doc.blocks[bi - 1] {
            a.append(&mut b);
            state.cursor = join_cursor;
            state.cursor.normalize(doc);
            return true;
        }
        doc.blocks.insert(bi, Block::Paragraph(b));
        return true;
    }
    doc.blocks.insert(bi, cur);
    true
}

fn toggle_wrap(doc: &mut Document, state: &mut EditorState, bold: bool) -> bool {
    state.cursor.normalize(doc);
    if state.cursor.inline_path.len() != 1 {
        return false;
    }
    let i = state.cursor.inline_path[0];
    let bi = state.cursor.block_index;
    let off = state.cursor.offset;
    let il = match doc.blocks.get_mut(bi) {
        Some(Block::Paragraph(il)) => il,
        Some(Block::Heading { content, .. }) => content,
        _ => return false,
    };
    let Some(Inline::Text(s)) = il.get_mut(i) else {
        return false;
    };
    if !s.is_char_boundary(off) {
        return false;
    }
    let before_empty = s[..off].is_empty();
    let before = s[..off].to_string();
    let after = s[off..].to_string();
    let mid = if bold {
        Inline::Bold(vec![Inline::empty_text()])
    } else {
        Inline::Italic(vec![Inline::empty_text()])
    };
    let mut rebuilt: Vec<Inline> = Vec::new();
    if !before.is_empty() {
        rebuilt.push(Inline::Text(before));
    }
    rebuilt.push(mid);
    if !after.is_empty() {
        rebuilt.push(Inline::Text(after));
    }
    let mid_idx = if before_empty { i } else { i + 1 };
    il.remove(i);
    for (j, el) in rebuilt.into_iter().enumerate() {
        il.insert(i + j, el);
    }
    state.cursor.inline_path = vec![mid_idx, 0];
    state.cursor.offset = 0;
    true
}

pub fn reduce_edit(doc: &mut Document, state: &mut EditorState, op: EditOp) -> bool {
    let r = match op {
        EditOp::MoveCursor(_) => {
            return false;
        }
        EditOp::InsertChar(ch) => {
            state.cursor.normalize(doc);
            if let Some((lo, hi)) = take_non_empty_selection_range(state) {
                if !delete_char_gap_range(doc, state, lo, hi) {
                    return false;
                }
                if !insert_char(doc, state, ch) {
                    return false;
                }
            } else if !insert_char(doc, state, ch) {
                return false;
            }
            true
        }
        EditOp::Backspace => {
            state.cursor.normalize(doc);
            if let Some((lo, hi)) = take_non_empty_selection_range(state) {
                delete_char_gap_range(doc, state, lo, hi)
            } else {
                backspace(doc, state)
            }
        }
        EditOp::NewLine => {
            state.cursor.normalize(doc);
            if let Some((lo, hi)) = take_non_empty_selection_range(state) {
                if !delete_char_gap_range(doc, state, lo, hi) {
                    return false;
                }
                newline(doc, state)
            } else {
                newline(doc, state)
            }
        }
        EditOp::InsertBlock(i, block) => {
            state.selection = None;
            let i = i.min(doc.blocks.len());
            doc.blocks.insert(i, block);
            state.cursor = Cursor::new(i, vec![0], 0);
            true
        }
        EditOp::DeleteBlock(i) => {
            state.selection = None;
            if i >= doc.blocks.len() {
                false
            } else {
                let _ = doc.blocks.remove(i);
                if doc.blocks.is_empty() {
                    doc.blocks.push(Block::Paragraph(vec![Inline::empty_text()]));
                }
                let ni = i.min(doc.blocks.len().saturating_sub(1));
                state.cursor = Cursor::new(ni, vec![0], 0);
                state.cursor.normalize(doc);
                true
            }
        }
        EditOp::ToggleBold => toggle_wrap(doc, state, true),
        EditOp::ToggleItalic => toggle_wrap(doc, state, false),
    };
    if r {
        doc.bump_generation();
    }
    r
}

fn inline_mut<'a>(doc: &'a mut Document, c: &Cursor) -> Option<&'a mut String> {
    let b = doc.blocks.get_mut(c.block_index)?;
    match b {
        Block::Paragraph(il) | Block::Heading { content: il, .. } => {
            cursor::text_leaf_mut(il, &c.inline_path).map(|(s, _)| s)
        }
        _ => None,
    }
}

fn code_mut<'a>(doc: &'a mut Document, c: &Cursor) -> Option<&'a mut String> {
    let b = doc.blocks.get_mut(c.block_index)?;
    match b {
        Block::CodeBlock { text, .. } => Some(text),
        _ => None,
    }
}

/// Apply move using an existing layout (avoids a second `layout_document` when a cache is available).
pub fn apply_cursor_move_in_layout(
    doc: &Document,
    laid: &crate::layout::LaidOutDocument,
    state: &mut EditorState,
    m: CursorMove,
) {
    crate::layout::move_cursor_in_layout(doc, laid, &mut state.cursor, m);
}

/// Apply move by laying out the document once at `layout_width`.
pub fn apply_cursor_move(
    doc: &Document,
    layout_width: u16,
    state: &mut EditorState,
    m: CursorMove,
) {
    let laid =
        crate::layout::layout_document(doc, layout_width, &crate::layout::LayoutConfig::default());
    apply_cursor_move_in_layout(doc, &laid, state, m);
}

/// Convenience: edit + optional move (move uses `layout_width`). Returns `true` when
/// the op was a cursor move or a successful text edit.
pub fn reduce(
    doc: &mut Document,
    state: &mut EditorState,
    op: EditOp,
    layout_width: u16,
) -> bool {
    match &op {
        EditOp::MoveCursor(m) => {
            apply_cursor_move(doc, layout_width, state, m.clone());
            true
        }
        _ => reduce_edit(doc, state, op),
    }
}
