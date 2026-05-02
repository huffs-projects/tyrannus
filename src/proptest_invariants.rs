//! Property tests: safe random edit sequences must not panic and keep basic invariants.
use proptest::prelude::*;

use crate::{
    cursor_to_gap_index, document_char_count, gap_index_to_cursor, reduce, Block, Cursor, CursorMove,
    Document, EditOp, EditorState, Inline, LayoutConfig, cursor_to_row_col, layout_document,
};

fn layout_width() -> u16 {
    24
}

fn arb_op() -> impl Strategy<Value = EditOp> {
    prop_oneof![
        40 => (b' '..=b'~').prop_map(|b| EditOp::InsertChar(b as char)),
        2 => Just(EditOp::NewLine),
        2 => Just(EditOp::Backspace),
        2 => Just(EditOp::ToggleBold),
        2 => Just(EditOp::ToggleItalic),
        1 => Just(EditOp::MoveCursor(CursorMove::Left)),
        1 => Just(EditOp::MoveCursor(CursorMove::Right)),
        1 => Just(EditOp::MoveCursor(CursorMove::Up)),
        1 => Just(EditOp::MoveCursor(CursorMove::Down)),
        1 => Just(EditOp::MoveCursor(CursorMove::Home)),
        1 => Just(EditOp::MoveCursor(CursorMove::End)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Random navigation, typing, light formatting (bold/italic toggle), and deletes should not
    /// panic; cursor positions remain consistent with the flattened document when non-empty.
    #[test]
    fn random_edits_maintain_gap_invariants(
        ops in proptest::collection::vec(arb_op(), 0..48),
    ) {
        let mut doc = Document::new();
        let mut st = EditorState::default();
        let w = layout_width();
        for op in ops {
            let _ = reduce(&mut doc, &mut st, op, w);
            // Keep the cursor canonical after each op so random sequences stay mappable.
            st.cursor.normalize(&doc);
        }
        st.cursor.normalize(&doc);
        let n = document_char_count(&doc);
        if n == 0 {
            if let Some(g) = cursor_to_gap_index(&doc, &st.cursor) {
                assert_eq!(g, 0, "empty doc: only gap 0 is valid");
            }
        } else {
            st.cursor.normalize(&doc);
            if let Some(g) = cursor_to_gap_index(&doc, &st.cursor) {
                assert!(g <= n, "gap index {} exceeds char count {}", g, n);
                let c2 = gap_index_to_cursor(&doc, g)
                    .expect("gap should map to cursor when gap index is valid");
                st.cursor.normalize(&doc);
                let g_back = cursor_to_gap_index(&doc, &c2);
                assert!(g_back.is_some());
            }
        }
        let laid = layout_document(&doc, w, &LayoutConfig::default());
        let _ = cursor_to_row_col(&doc, &laid, &st.cursor);
    }
}

/// Non-empty block types still flatten and layout after quick mutations.
#[test]
fn proptest_smoke_mixed_block_doc() {
    let mut doc = Document::with_blocks(vec![
        Block::Heading {
            level: 1,
            content: vec![Inline::text_str("H")],
        },
        Block::CodeBlock {
            lang: None,
            text: "c".to_string(),
        },
    ]);
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 0),
        ..Default::default()
    };
    st.cursor.normalize(&doc);
    let w = 40u16;
    let _ = reduce(&mut doc, &mut st, EditOp::MoveCursor(CursorMove::Right), w);
    let _ = reduce(&mut doc, &mut st, EditOp::MoveCursor(CursorMove::Down), w);
    st.cursor.normalize(&doc);
    let flat = document_char_count(&doc);
    assert!(flat >= 1);
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    let _ = cursor_to_row_col(&doc, &laid, &st.cursor);
}
