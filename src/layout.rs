//! Full-document layout: blocks to wrapped visual lines and cursor mapping.

use unicode_width::UnicodeWidthChar;

use crate::cursor::Cursor;
use crate::document::{Block, Document, Inline};
use crate::edit::CursorMove;

/// Horizontal padding inside the terminal (each side).
pub const H_PAD: usize = 1;

#[derive(Clone, Debug)]
pub struct LayoutConfig {
    /// Blank lines inserted between visual wrapped lines within a block.
    pub line_gap_lines: usize,
    /// Blank lines inserted between consecutive top-level blocks (not doubled per block).
    pub block_gap_lines: usize,
    pub code_margin: usize,
    /// Extra display columns after each space in the body (must match the painter).
    pub extra_word_spacing: usize,
    /// Extra display columns after each non-space character (must match the painter).
    pub extra_letter_spacing: usize,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            line_gap_lines: 0,
            // Single-spaced by default: no extra blank rows between top-level blocks.
            block_gap_lines: 0,
            code_margin: 1,
            extra_word_spacing: 0,
            extra_letter_spacing: 0,
        }
    }
}

#[inline]
fn cell_layout_width(ch: char, cfg: &LayoutConfig) -> usize {
    ch.width().unwrap_or(0)
        + if ch == ' ' {
            cfg.extra_word_spacing
        } else {
            cfg.extra_letter_spacing
        }
}

/// One displayed character: cursor before `ch`, stable document-order index (same as flatten order).
pub type LaidCell = (Cursor, char, usize);

/// One visual row: leading blank columns (`gutter`) then `cells` (cursor before each char).
#[derive(Clone, Debug)]
pub struct LaidOutLine {
    pub row: usize,
    pub gutter: usize,
    pub prefix: String,
    pub cells: Vec<LaidCell>,
    /// When [`LaidOutLine::cells`] is empty but this row is the start of a block with no characters
    /// (e.g. empty paragraph), this is the canonical [`Cursor`] after [`Cursor::normalize`].
    pub empty_start_cursor: Option<Cursor>,
}

#[derive(Clone, Debug)]
pub struct LaidOutDocument {
    pub lines: Vec<LaidOutLine>,
}

impl LaidOutDocument {
    pub fn total_rows(&self) -> usize {
        self.lines.len()
    }

    pub fn display_width(&self, row: usize, cfg: &LayoutConfig) -> usize {
        self.lines
            .iter()
            .find(|l| l.row == row)
            .map(|l| {
                l.gutter
                    + l
                        .cells
                        .iter()
                        .map(|(_, ch, _)| cell_layout_width(*ch, cfg))
                        .sum::<usize>()
            })
            .unwrap_or(0)
    }
}

pub fn layout_document(doc: &Document, width: u16, cfg: &LayoutConfig) -> LaidOutDocument {
    let inner = width.saturating_sub(H_PAD as u16 * 2) as usize;
    let inner = inner.max(1);

    let mut lines: Vec<LaidOutLine> = Vec::new();
    let mut row = 0usize;
    let mut next_global: usize = 0;

    let push_blank = |lines: &mut Vec<LaidOutLine>, row: &mut usize| {
        lines.push(LaidOutLine {
            row: *row,
            gutter: 0,
            prefix: String::new(),
            cells: Vec::new(),
            empty_start_cursor: None,
        });
        *row += 1;
    };

    for (bi, block) in doc.blocks.iter().enumerate() {
        if bi > 0 {
            for _ in 0..cfg.block_gap_lines {
                push_blank(&mut lines, &mut row);
            }
        }
        match block {
            Block::Paragraph(inlines) => {
                wrap_inline_stream(
                    doc,
                    bi,
                    inlines,
                    inner,
                    0,
                    "",
                    "",
                    &mut next_global,
                    &mut lines,
                    &mut row,
                    cfg,
                );
            }
            Block::Heading { content, .. } => {
                wrap_inline_stream(
                    doc,
                    bi,
                    content,
                    inner,
                    0,
                    "",
                    "",
                    &mut next_global,
                    &mut lines,
                    &mut row,
                    cfg,
                );
            }
            Block::CodeBlock { text, .. } => {
                wrap_code_stream(
                    doc,
                    bi,
                    text,
                    inner,
                    0,
                    cfg.code_margin,
                    "",
                    "",
                    &mut next_global,
                    &mut lines,
                    &mut row,
                    cfg,
                );
            }
        }
    }

    if lines.is_empty() {
        push_blank(&mut lines, &mut row);
    }

    LaidOutDocument { lines }
}

pub fn flatten_inlines_for_edit(
    block_index: usize,
    inlines: &[Inline],
) -> Vec<(Cursor, char)> {
    flatten_inlines(block_index, inlines)
}

fn flatten_inlines(block_index: usize, inlines: &[Inline]) -> Vec<(Cursor, char)> {
    let mut out = Vec::new();
    walk_inlines(block_index, inlines, &mut Vec::new(), &mut out);
    out
}

fn walk_inlines(
    block_index: usize,
    inlines: &[Inline],
    path: &mut Vec<usize>,
    out: &mut Vec<(Cursor, char)>,
) {
    for (i, inline) in inlines.iter().enumerate() {
        path.push(i);
        match inline {
            Inline::Text(s) => {
                for (byte_off, ch) in s.char_indices() {
                    out.push((
                        Cursor {
                            block_index,
                            inline_path: path.clone(),
                            offset: byte_off,
                        },
                        ch,
                    ));
                }
            }
            Inline::Bold(inner) | Inline::Italic(inner) => {
                walk_inlines(block_index, inner, path, out);
            }
            Inline::Link { text, .. } => {
                walk_inlines(block_index, text, path, out);
            }
        }
        path.pop();
    }
}

/// All `(cursor, char)` in document order — same sequence as layout global indices.
pub fn flatten_document_chars(doc: &Document) -> Vec<(Cursor, char)> {
    let mut v = Vec::new();
    for (bi, block) in doc.blocks.iter().enumerate() {
        match block {
            Block::Paragraph(inlines) => {
                v.extend(flatten_inlines(bi, inlines));
            }
            Block::Heading { content, .. } => {
                v.extend(flatten_inlines(bi, content));
            }
            Block::CodeBlock { text, .. } => {
                for (byte_off, ch) in text.char_indices() {
                    v.push((
                        Cursor {
                            block_index: bi,
                            inline_path: Vec::new(),
                            offset: byte_off,
                        },
                        ch,
                    ));
                }
            }
        }
    }
    v
}

#[allow(clippy::too_many_arguments)]
fn wrap_inline_stream(
    doc: &Document,
    block_index: usize,
    inlines: &[Inline],
    content_width: usize,
    first_gutter: usize,
    first_prefix: &str,
    cont_prefix: &str,
    next_global: &mut usize,
    lines: &mut Vec<LaidOutLine>,
    row: &mut usize,
    cfg: &LayoutConfig,
) {
    let stream = flatten_inlines(block_index, inlines);
    let empty_start_cursor = if stream.is_empty() {
        let mut c = Cursor {
            block_index,
            inline_path: vec![],
            offset: 0,
        };
        c.normalize(doc);
        Some(c)
    } else {
        None
    };
    let mut cur = LaidOutLine {
        row: *row,
        gutter: first_gutter,
        prefix: first_prefix.to_string(),
        cells: Vec::new(),
        empty_start_cursor: None,
    };
    let mut used = 0usize;

    for (curs, ch) in stream {
        let w = cell_layout_width(ch, cfg);
        let limit = content_width;
        if used + w > limit && !cur.cells.is_empty() {
            cur.row = *row;
            lines.push(cur);
            *row += 1;
            for _ in 0..cfg.line_gap_lines {
                lines.push(LaidOutLine {
                    row: *row,
                    gutter: 0,
                    prefix: String::new(),
                    cells: Vec::new(),
                    empty_start_cursor: None,
                });
                *row += 1;
            }
            cur = LaidOutLine {
                row: *row,
                gutter: first_gutter,
                prefix: cont_prefix.to_string(),
                cells: Vec::new(),
                empty_start_cursor: None,
            };
            used = 0;
        }
        cur.cells.push((curs, ch, *next_global));
        *next_global += 1;
        used += w;
    }
    cur.row = *row;
    cur.empty_start_cursor = empty_start_cursor;
    lines.push(cur);
    *row += 1;
}

#[allow(clippy::too_many_arguments)]
fn wrap_code_stream(
    doc: &Document,
    block_index: usize,
    text: &str,
    content_width: usize,
    first_gutter: usize,
    code_margin: usize,
    first_prefix: &str,
    cont_prefix: &str,
    next_global: &mut usize,
    lines: &mut Vec<LaidOutLine>,
    row: &mut usize,
    cfg: &LayoutConfig,
) {
    let margin = code_margin.min(content_width);
    let stream: Vec<(Cursor, char)> = text
        .char_indices()
        .map(|(byte_off, ch)| {
            (
                Cursor {
                    block_index,
                    inline_path: Vec::new(),
                    offset: byte_off,
                },
                ch,
            )
        })
        .collect();

    let empty_start_cursor = if text.is_empty() {
        let mut c = Cursor {
            block_index,
            inline_path: vec![],
            offset: 0,
        };
        c.normalize(doc);
        Some(c)
    } else {
        None
    };
    let mut cur = LaidOutLine {
        row: *row,
        gutter: first_gutter + margin,
        prefix: format!("{first_prefix}{}", " ".repeat(margin)),
        cells: Vec::new(),
        empty_start_cursor: None,
    };
    let mut used = 0usize;

    for (curs, ch) in stream {
        let w = cell_layout_width(ch, cfg);
        if used + w > content_width.saturating_sub(margin) && !cur.cells.is_empty() {
            cur.row = *row;
            lines.push(cur);
            *row += 1;
            for _ in 0..cfg.line_gap_lines {
                lines.push(LaidOutLine {
                    row: *row,
                    gutter: 0,
                    prefix: String::new(),
                    cells: Vec::new(),
                    empty_start_cursor: None,
                });
                *row += 1;
            }
            cur = LaidOutLine {
                row: *row,
                gutter: first_gutter + margin,
                prefix: format!("{cont_prefix}{}", " ".repeat(margin)),
                cells: Vec::new(),
                empty_start_cursor: None,
            };
            used = 0;
        }
        cur.cells.push((curs, ch, *next_global));
        *next_global += 1;
        used += w;
    }
    cur.row = *row;
    cur.empty_start_cursor = empty_start_cursor;
    lines.push(cur);
    *row += 1;
}

pub fn move_cursor_in_layout(
    doc: &Document,
    laid: &LaidOutDocument,
    cursor: &mut Cursor,
    m: CursorMove,
) {
    let (mut r, mut c) = cursor_to_row_col(doc, laid, cursor).unwrap_or((0, 0));
    let max_row = laid.lines.len().saturating_sub(1);
    match m {
        CursorMove::Up => {
            r = r.saturating_sub(1);
        }
        CursorMove::Down => {
            if r < max_row {
                r += 1;
            }
        }
        CursorMove::Left => {
            if c > 0 {
                c -= 1;
            } else if r > 0 {
                r -= 1;
                if let Some(pl) = laid.lines.iter().find(|l| l.row == r) {
                    c = if pl.cells.is_empty() {
                        pl.gutter
                    } else {
                        pl.gutter + pl.cells.len() - 1
                    };
                }
            } else {
                crate::edit::move_cursor_prev_char(doc, cursor);
                return;
            }
        }
        CursorMove::Right => {
            let line = laid.lines.iter().find(|l| l.row == r);
            let max_c = line.map(|l| l.gutter + l.cells.len()).unwrap_or(0);
            if c < max_c {
                c += 1;
            } else if r < max_row {
                r += 1;
                c = 0;
            } else {
                crate::edit::move_cursor_next_char(doc, cursor);
                return;
            }
        }
        CursorMove::Home => {
            if let Some(line) = laid.lines.iter().find(|l| l.row == r) {
                c = line.gutter;
            }
        }
        CursorMove::End => {
            if let Some(line) = laid.lines.iter().find(|l| l.row == r) {
                c = line.gutter + line.cells.len();
            }
        }
    }
    if let Some(nc) = cursor_from_row_col(doc, laid, r, c) {
        *cursor = nc;
    }
    cursor.normalize(doc);
}

/// Display column is 0-based: `0..gutter` are virtual leading columns; `gutter..` index content.
pub fn cursor_to_row_col(
    _doc: &Document,
    laid: &LaidOutDocument,
    cursor: &Cursor,
) -> Option<(usize, usize)> {
    for line in &laid.lines {
        let g = line.gutter;
        for (i, (c, _, _)) in line.cells.iter().enumerate() {
            if c == cursor {
                return Some((line.row, g + i));
            }
        }
        if let Some((c_last, ch, _)) = line.cells.last() {
            let end = advance_cursor_after_char(c_last, *ch);
            if &end == cursor {
                return Some((line.row, g + line.cells.len()));
            }
        }
        if line.cells.is_empty() {
            if let Some(c0) = &line.empty_start_cursor {
                if c0 == cursor {
                    return Some((line.row, g));
                }
            }
        }
    }
    None
}

pub fn advance_cursor_after_char(before: &Cursor, ch: char) -> Cursor {
    let mut c = before.clone();
    c.offset += ch.len_utf8();
    c
}

pub fn cursor_from_row_col(
    _doc: &Document,
    laid: &LaidOutDocument,
    row: usize,
    col: usize,
) -> Option<Cursor> {
    let line = laid.lines.iter().find(|l| l.row == row)?;
    let g = line.gutter;
    if col < g {
        if line.cells.is_empty() {
            return None;
        }
        return line.cells.first().map(|(c, _, _)| c.clone());
    }
    if line.cells.is_empty() {
        if col == g {
            return line.empty_start_cursor.clone();
        }
        return None;
    }
    let idx = col - g;
    if idx >= line.cells.len() {
        return line
            .cells
            .last()
            .map(|(c, ch, _)| advance_cursor_after_char(c, *ch));
    }
    line.cells.get(idx).map(|(c, _, _)| c.clone())
}
