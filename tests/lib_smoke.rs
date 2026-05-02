use tyrannus::{reduce_edit, Document, EditOp, EditorState, LayoutConfig, layout_document};

/// External crate can call the public editor API and layout.
#[test]
fn new_document_inserts_and_layouts() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(
        &mut doc,
        &mut st,
        EditOp::InsertChar('a')
    ));
    let laid = layout_document(&doc, 40, &LayoutConfig::default());
    assert!(!laid.lines.is_empty());
}
