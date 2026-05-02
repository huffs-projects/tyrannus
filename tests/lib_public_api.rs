//! Integration coverage for the tyrannus library surface (`tests/` harness).

use tyrannus::{
    apply_cursor_move_in_layout, layout_document, reduce_edit,
    CursorMove, Document, EditOp, EditorState, LayoutConfig, viewport_line_range,
};

#[test]
fn viewport_line_range_via_lib_crate_bounds() {
    assert_eq!(viewport_line_range(2, 4, 20), 2..6);
}

#[test]
fn apply_cursor_move_in_layout_against_precomputed_laid_document() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('z')));
    let w = 24u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::End);
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::Home);
}
