//! AST-aware cursor: block index + path into inline tree + offset in leaf `Text`.

use crate::document::{Block, Document, Inline, InlineVec};

/// Indices from the root `InlineVec` of the current block downward until a `Text` leaf.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cursor {
    pub block_index: usize,
    /// Path of child indices: `inlines[path[0]]` then into nested vec at `path[1]`, etc.
    pub inline_path: Vec<usize>,
    /// Byte offset within the target `Text` leaf (UTF-8), or into `CodeBlock.text`.
    pub offset: usize,
}

impl Cursor {
    pub fn new(block_index: usize, inline_path: Vec<usize>, offset: usize) -> Self {
        Self {
            block_index,
            inline_path,
            offset,
        }
    }

    /// Clamp cursor so it points at valid UTF-8 boundary in existing document.
    pub fn normalize(&mut self, doc: &Document) {
        if doc.blocks.is_empty() {
            self.block_index = 0;
            self.inline_path.clear();
            self.offset = 0;
            return;
        }
        self.block_index = self.block_index.min(doc.blocks.len().saturating_sub(1));
        match &doc.blocks[self.block_index] {
            Block::Paragraph(inlines)
            | Block::Heading {
                content: inlines, ..
            } => {
                normalize_in_inline_tree(inlines, &mut self.inline_path, &mut self.offset);
            }
            Block::CodeBlock { text, .. } => {
                self.inline_path.clear();
                self.offset = self.offset.min(text.len());
                if !text.is_char_boundary(self.offset) {
                    self.offset = prev_char_boundary(text, self.offset);
                }
            }
        }
    }
}

fn prev_char_boundary(s: &str, i: usize) -> usize {
    if i == 0 || i > s.len() {
        return 0;
    }
    let mut j = i;
    while j > 0 && !s.is_char_boundary(j) {
        j -= 1;
    }
    j
}

fn normalize_in_inline_tree(inlines: &[Inline], path: &mut Vec<usize>, offset: &mut usize) {
    if inlines.is_empty() {
        path.clear();
        *offset = 0;
        return;
    }
    if path.is_empty() {
        path.push(0);
    }
    // Clamp first index
    if path[0] >= inlines.len() {
        path[0] = inlines.len() - 1;
        path.truncate(1);
    }
    let mut current: &[Inline] = inlines;
    let mut depth = 0;
    loop {
        let idx = path[depth];
        if idx >= current.len() {
            path.truncate(depth);
            if path.is_empty() {
                path.push(0);
            }
            depth = 0;
            current = inlines;
            continue;
        }
        match &current[idx] {
            Inline::Text(s) => {
                path.truncate(depth + 1);
                if !s.is_char_boundary(*offset) {
                    *offset = prev_char_boundary(s, *offset);
                }
                *offset = (*offset).min(s.len());
                return;
            }
            Inline::Bold(inner) | Inline::Italic(inner) => {
                if inner.is_empty() {
                    path.truncate(depth + 1);
                    *offset = 0;
                    return;
                }
                depth += 1;
                if path.len() <= depth {
                    path.push(0);
                } else if path[depth] >= inner.len() {
                    path[depth] = inner.len() - 1;
                }
                current = inner.as_slice();
            }
            Inline::Link { text, .. } => {
                if text.is_empty() {
                    path.truncate(depth + 1);
                    *offset = 0;
                    return;
                }
                depth += 1;
                if path.len() <= depth {
                    path.push(0);
                } else if path[depth] >= text.len() {
                    path[depth] = text.len() - 1;
                }
                current = text.as_slice();
            }
        }
    }
}

/// Walk `inlines` following `path`; returns mutable reference to leaf `String` if path ends at `Text`.
pub fn text_leaf_mut<'a>(
    inlines: &'a mut Vec<Inline>,
    path: &[usize],
) -> Option<(&'a mut String, usize)> {
    if path.is_empty() {
        return None;
    }
    let mut slice: &mut [Inline] = inlines.as_mut_slice();
    let last = path.len() - 1;
    for (d, &idx) in path.iter().enumerate() {
        if idx >= slice.len() {
            return None;
        }
        if d == last {
            return match &mut slice[idx] {
                Inline::Text(s) => Some((s, idx)),
                _ => None,
            };
        }
        slice = match &mut slice[idx] {
            Inline::Bold(v) | Inline::Italic(v) => v.as_mut_slice(),
            Inline::Link { text, .. } => text.as_mut_slice(),
            Inline::Text(_) => return None,
        };
    }
    None
}

/// Read-only: byte length of text leaf at path.
pub fn text_leaf_len(inlines: &[Inline], path: &[usize]) -> Option<usize> {
    if path.is_empty() {
        return None;
    }
    let mut slice = inlines;
    let last = path.len() - 1;
    for (d, &idx) in path.iter().enumerate() {
        if idx >= slice.len() {
            return None;
        }
        if d == last {
            return match &slice[idx] {
                Inline::Text(s) => Some(s.len()),
                _ => None,
            };
        }
        slice = match &slice[idx] {
            Inline::Bold(v) | Inline::Italic(v) => v.as_slice(),
            Inline::Link { text, .. } => text.as_slice(),
            Inline::Text(_) => return None,
        };
    }
    None
}

pub fn block_inline_mut(block: &mut Block) -> Option<&mut InlineVec> {
    match block {
        Block::Paragraph(v) | Block::Heading { content: v, .. } => Some(v),
        Block::CodeBlock { .. } => None,
    }
}

pub fn block_inline_slice(block: &Block) -> Option<&[Inline]> {
    match block {
        Block::Paragraph(v) | Block::Heading { content: v, .. } => Some(v.as_slice()),
        Block::CodeBlock { .. } => None,
    }
}

pub fn get_text_at_path<'a>(inlines: &'a [Inline], path: &[usize]) -> Option<&'a str> {
    if path.is_empty() {
        return None;
    }
    let mut slice = inlines;
    let last = path.len() - 1;
    for (d, &idx) in path.iter().enumerate() {
        let inline = slice.get(idx)?;
        if d == last {
            return match inline {
                Inline::Text(s) => Some(s.as_str()),
                _ => None,
            };
        }
        slice = match inline {
            Inline::Bold(v) | Inline::Italic(v) => v.as_slice(),
            Inline::Link { text, .. } => text.as_slice(),
            Inline::Text(_) => return None,
        };
    }
    None
}
