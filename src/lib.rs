//! Tyrannus — document model, editing, layout, selection overlay, and viewport cache (phases 1–5).

pub mod cursor;
pub mod document;
pub mod edit;
pub mod layout;
pub mod viewport;

pub use cursor::Cursor;
pub use document::{Block, Document, Inline};
pub use edit::{
    apply_cursor_move, apply_cursor_move_in_layout, cursor_to_gap_index, document_char_count,
    gap_index_to_cursor, reduce, reduce_edit, selection_ordered, CursorMove, EditOp, EditorState,
    Mode, Selection,
};
pub use layout::{
    advance_cursor_after_char, cursor_from_row_col, cursor_to_row_col, flatten_document_chars,
    layout_document, LaidCell, LaidOutDocument, LayoutConfig,
};
pub use viewport::{
    clamp_scroll, merge_regions, schedule_render, scroll_to_reveal_row, viewport_line_range,
    LayoutCache, LayoutMemoryStats, RenderScheduler, RenderTask, Viewport,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod proptest_invariants;
