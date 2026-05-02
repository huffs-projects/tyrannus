use std::collections::VecDeque;

use unicode_width::UnicodeWidthChar;

use crate::*;

// --- Phase 1: blocks, style, structure, cursor moves ---

#[test]
fn heading_layout_and_cursor_roundtrip() {
    let doc = Document::with_blocks(vec![Block::Heading {
        level: 2,
        content: vec![Inline::text_str("Title")],
    }]);
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 0),
        ..Default::default()
    };
    st.cursor.normalize(&doc);
    let w = 40u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    let s: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(s, "Title");
    let rc = cursor_to_row_col(&doc, &laid, &st.cursor).expect("heading start maps");
    let back = cursor_from_row_col(&doc, &laid, rc.0, rc.1).expect("roundtrip");
    assert_eq!(back, st.cursor);
}

#[test]
fn code_block_gaps_sequential() {
    let doc = Document::with_blocks(vec![Block::CodeBlock {
        lang: None,
        text: "abc".to_string(),
    }]);
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 0),
        ..Default::default()
    };
    st.cursor.normalize(&doc);
    let w = 10u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    for (g, ch) in (0u8..3).map(|g| (g, ['a', 'b', 'c'][g as usize])) {
        st.cursor = gap_index_to_cursor(&doc, g as usize).expect("gap");
        let rc = cursor_to_row_col(&doc, &laid, &st.cursor);
        assert!(rc.is_some(), "gap {g} should map; char {ch}");
    }
    for (i, g) in (0..3).enumerate() {
        st.cursor = gap_index_to_cursor(&doc, g).unwrap();
        let gi = cursor_to_gap_index(&doc, &st.cursor);
        assert_eq!(gi, Some(g), "at char index {i}");
    }
}

#[test]
fn code_block_newline_inserts_newline() {
    let mut doc = Document::with_blocks(vec![Block::CodeBlock {
        lang: None,
        text: "a".to_string(),
    }]);
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 1),
        ..Default::default()
    };
    st.cursor.normalize(&doc);
    assert!(reduce_edit(&mut doc, &mut st, EditOp::NewLine));
    let text = match &doc.blocks[0] {
        Block::CodeBlock { text, .. } => text.as_str(),
        _ => panic!("expected code block"),
    };
    assert!(text.contains('\n'), "expected newline in code");
}

#[test]
fn toggle_bold_wraps_at_cursor() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in ['a', 'b'] {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    st.cursor = Cursor::new(0, vec![0], 1);
    st.cursor.normalize(&doc);
    assert!(reduce_edit(&mut doc, &mut st, EditOp::ToggleBold));
    let Block::Paragraph(inlines) = &doc.blocks[0] else {
        panic!();
    };
    assert!(inlines.iter().any(|il| matches!(il, Inline::Bold(_))));
    let s: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(s, "ab");
}

#[test]
fn toggle_italic_wraps_at_cursor() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in ['x', 'y'] {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    st.cursor = Cursor::new(0, vec![0], 1);
    st.cursor.normalize(&doc);
    assert!(reduce_edit(&mut doc, &mut st, EditOp::ToggleItalic));
    let Block::Paragraph(inlines) = &doc.blocks[0] else {
        panic!();
    };
    assert!(inlines.iter().any(|il| matches!(il, Inline::Italic(_))));
    let s: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(s, "xy");
}

#[test]
fn insert_block_adds_block() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(
        &mut doc,
        &mut st,
        EditOp::InsertBlock(0, Block::Paragraph(vec![Inline::text_str("new")]),)
    ));
    assert_eq!(doc.blocks.len(), 2);
}

#[test]
fn delete_block_drops_leading() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(
        &mut doc,
        &mut st,
        EditOp::InsertBlock(1, Block::Paragraph(vec![Inline::text_str("second")])),
    ));
    assert!(reduce_edit(&mut doc, &mut st, EditOp::DeleteBlock(0)));
    assert_eq!(doc.blocks.len(), 1);
    let text: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(text, "second");
}

#[test]
fn move_cursor_left_right_by_reduce() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "hi".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let w = 32u16;
    let g_end = cursor_to_gap_index(&doc, &st.cursor).expect("end gap");
    let _ = reduce(
        &mut doc,
        &mut st,
        EditOp::MoveCursor(CursorMove::Left),
        w,
    );
    let g1 = cursor_to_gap_index(&doc, &st.cursor).expect("left once");
    assert_eq!(g_end.saturating_sub(1), g1);
    let _ = reduce(
        &mut doc,
        &mut st,
        EditOp::MoveCursor(CursorMove::Right),
        w,
    );
    let g2 = cursor_to_gap_index(&doc, &st.cursor).expect("back to end");
    assert_eq!(g2, g_end);
}

// --- Phase 2: paragraph newline merge ---

#[test]
fn multi_step_insert_built_text() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('a')));
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('b')));
    let text_ab: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(text_ab, "ab");
}

#[test]
fn paragraph_newline_split_at_cursor() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in ['a', 'b'] {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    st.cursor = Cursor::new(0, vec![0], 1);
    st.cursor.normalize(&doc);
    let before_text: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(before_text, "ab");
    assert!(reduce_edit(&mut doc, &mut st, EditOp::NewLine));
    assert_eq!(doc.blocks.len(), 2);
    let t0 = match &doc.blocks[0] {
        Block::Paragraph(il) => crate::cursor::get_text_at_path(il, &[0]).unwrap(),
        _ => panic!(),
    };
    let t1 = match &doc.blocks[1] {
        Block::Paragraph(il) => crate::cursor::get_text_at_path(il, &[0]).unwrap(),
        _ => panic!(),
    };
    assert_eq!(t0, "a");
    assert_eq!(t1, "b");
}

#[test]
fn selection_ordered_range() {
    let s = Selection { anchor: 3, head: 1 };
    assert_eq!(selection_ordered(&s), (1, 3));
    let t = Selection { anchor: 2, head: 2 };
    assert_eq!(selection_ordered(&t), (2, 2));
}

#[test]
fn global_indices_continuous_across_wrap() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "hello world".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let laid = layout_document(&doc, 10, &LayoutConfig::default());
    let mut indices: Vec<usize> = Vec::new();
    for line in &laid.lines {
        for (_, _, gidx) in &line.cells {
            indices.push(*gidx);
        }
    }
    assert_eq!(indices.len(), 11);
    for (i, &g) in indices.iter().enumerate() {
        assert_eq!(g, i);
    }
}

#[test]
fn replace_selection_with_char() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in ['a', 'b', 'c'] {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    st.selection = Some(Selection { anchor: 0, head: 2 });
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('x')));
    let s: String = flatten_document_chars(&doc)
        .iter()
        .map(|(_, c)| *c)
        .collect();
    assert_eq!(s, "xc");
}

#[test]
fn insert_and_backspace() {
    let mut doc = Document::new();
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 0),
        ..Default::default()
    };
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('x')));
    assert_eq!(
        crate::cursor::get_text_at_path(
            match &doc.blocks[0] {
                Block::Paragraph(il) => il,
                _ => panic!(),
            },
            &[0],
        ),
        Some("x")
    );
    st.cursor = Cursor::new(0, vec![0], 1);
    st.cursor.normalize(&doc);
    assert!(reduce_edit(&mut doc, &mut st, EditOp::Backspace));
    assert_eq!(
        crate::cursor::get_text_at_path(
            match &doc.blocks[0] {
                Block::Paragraph(il) => il,
                _ => panic!(),
            },
            &[0],
        ),
        Some("")
    );
}

#[test]
fn newline_splits_paragraph() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('a')));
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('b')));
    st.cursor.offset = 1;
    assert!(reduce_edit(&mut doc, &mut st, EditOp::NewLine));
    assert_eq!(doc.blocks.len(), 2);
    let t0 = match &doc.blocks[0] {
        Block::Paragraph(il) => crate::cursor::get_text_at_path(il, &[0]).unwrap(),
        _ => panic!(),
    };
    let t1 = match &doc.blocks[1] {
        Block::Paragraph(il) => crate::cursor::get_text_at_path(il, &[0]).unwrap(),
        _ => panic!(),
    };
    assert_eq!(t0, "a");
    assert_eq!(t1, "b");
}

#[test]
fn layout_wraps_narrow() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "hello world".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let laid = layout_document(&doc, 10, &LayoutConfig::default());
    let nonempty: Vec<_> = laid.lines.iter().filter(|l| !l.cells.is_empty()).collect();
    assert!(
        nonempty.len() >= 2,
        "expected wrap into 2+ content lines, got {}",
        nonempty.len()
    );
}

#[test]
fn cursor_row_col_roundtrip() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('z')));
    let w = 40u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    let rc = cursor_to_row_col(&doc, &laid, &st.cursor).expect("cursor maps");
    let back = cursor_from_row_col(&doc, &laid, rc.0, rc.1).expect("roundtrip");
    assert_eq!(back, st.cursor);
}

#[test]
fn cursor_row_col_empty_paragraph_maps() {
    let doc = Document::new();
    let mut st = EditorState::default();
    st.cursor.normalize(&doc);
    let laid = layout_document(&doc, 40, &LayoutConfig::default());
    assert!(
        laid.lines.iter().any(|l| l.empty_start_cursor.is_some()),
        "expected a visual line for the empty paragraph to carry empty_start_cursor"
    );
    let rc = cursor_to_row_col(&doc, &laid, &st.cursor).expect("cursor maps on empty line");
    let back = cursor_from_row_col(&doc, &laid, rc.0, rc.1).expect("roundtrip");
    assert_eq!(back, st.cursor);
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn merge_regions_unions_overlaps() {
    let dirty = vec![4..8];
    let m = merge_regions(&[10..12, 2..5], &dirty);
    assert_eq!(m, vec![2..8, 10..12]);
}

#[test]
fn clamp_scroll_respects_bounds() {
    assert_eq!(clamp_scroll(95, 10, 100), 90);
    assert_eq!(clamp_scroll(0, 10, 5), 0);
    assert_eq!(clamp_scroll(5, 10, 100), 5);
}

#[test]
fn scroll_to_reveal_row_moves_viewport() {
    assert_eq!(scroll_to_reveal_row(50, 10, 0), 41);
    assert_eq!(scroll_to_reveal_row(3, 10, 10), 3);
    assert_eq!(scroll_to_reveal_row(15, 10, 10), 10);
}

#[test]
fn layout_cache_skips_relayout_when_fresh() {
    let doc = Document::new();
    let mut cache = LayoutCache::default();
    let w = 40u16;
    let cfg = LayoutConfig::default();
    cache.sync(&doc, w, &cfg);
    let n = cache.laid().lines.len();
    cache.sync(&doc, w, &cfg);
    assert_eq!(cache.laid().lines.len(), n);
}

#[test]
fn document_generation_bumps_on_edit() {
    let mut doc = Document::new();
    let g0 = doc.generation;
    let mut st = EditorState::default();
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('a')));
    assert_ne!(doc.generation, g0);
}

#[test]
fn cursor_remains_mappable_across_widths() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "the quick brown fox jumps over the lazy dog".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    for w in [80u16, 40, 20, 10, 5, 3, 2, 1] {
        let laid = layout_document(&doc, w, &LayoutConfig::default());
        let rc = cursor_to_row_col(&doc, &laid, &st.cursor);
        assert!(
            rc.is_some(),
            "cursor should map at width {w}; got None for laid {} lines",
            laid.lines.len()
        );
    }
}

#[test]
fn backspace_through_many_blank_lines_keeps_cursor_mappable() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    // Build a run of empty paragraphs.
    for _ in 0..8 {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::NewLine));
    }

    // Walk back up by deleting paragraph boundaries. Cursor must remain drawable.
    for _ in 0..8 {
        let laid = layout_document(&doc, 40, &LayoutConfig::default());
        let rc = cursor_to_row_col(&doc, &laid, &st.cursor);
        assert!(
            rc.is_some(),
            "cursor should map before backspace on blank lines"
        );
        assert!(
            reduce_edit(&mut doc, &mut st, EditOp::Backspace),
            "backspace should merge blank paragraphs"
        );
        let laid_after = layout_document(&doc, 40, &LayoutConfig::default());
        let rc_after = cursor_to_row_col(&doc, &laid_after, &st.cursor);
        assert!(
            rc_after.is_some(),
            "cursor should map after backspace on blank lines"
        );
    }
}

#[test]
fn newline_spam_then_backspace_spam_keeps_cursor_drawable() {
    let mut doc = Document::new();
    let mut st = EditorState::default();

    // Simulate key spam in the same sequence as the TUI loop would emit.
    let mut entered = 0usize;
    for _ in 0..32 {
        if reduce_edit(&mut doc, &mut st, EditOp::NewLine) {
            entered += 1;
        }
        let laid = layout_document(&doc, 32, &LayoutConfig::default());
        assert!(
            cursor_to_row_col(&doc, &laid, &st.cursor).is_some(),
            "cursor should stay drawable during newline spam"
        );
    }

    for _ in 0..entered {
        assert!(
            reduce_edit(&mut doc, &mut st, EditOp::Backspace),
            "backspace should merge after newline spam"
        );
        let laid = layout_document(&doc, 32, &LayoutConfig::default());
        assert!(
            cursor_to_row_col(&doc, &laid, &st.cursor).is_some(),
            "cursor should stay drawable during backspace spam"
        );
    }
}

#[test]
fn laid_line_row_matches_vector_index() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "hello world wide web".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let laid = layout_document(&doc, 8, &LayoutConfig::default());
    for (i, line) in laid.lines.iter().enumerate() {
        assert_eq!(line.row, i, "viewport indexing assumes row == line index");
    }
}

#[test]
fn scheduler_process_frame_respects_budget() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "abcdefghijklmnopqrstuvwxyz".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let mut cache = LayoutCache::default();
    cache.scheduler.budget_per_frame = 3;
    cache.sync(&doc, 6, &LayoutConfig::default());
    let processed = cache.process_frame(&Viewport {
        top_index: 0,
        bottom_exclusive: 4,
        width: 6,
    });
    assert!(processed <= 3);
}

#[test]
fn layout_memory_stats_reports_counts() {
    let doc = Document::new();
    let mut cache = LayoutCache::default();
    cache.sync(&doc, 40, &LayoutConfig::default());
    let stats = cache.memory_stats();
    assert!(stats.line_count >= 1);
    assert!(stats.approx_bytes > 0);
}

// --- Phase 3: Unicode and fullwidth / combining ---

/// Combining sequence: e + U+301 should layout and allow cursor mapping.
#[test]
fn combining_e_acute_lays_out_and_maps() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "e\u{301}hi".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    for w in [40u16, 8, 3] {
        let laid = layout_document(&doc, w, &LayoutConfig::default());
        st.cursor.normalize(&doc);
        let rc = cursor_to_row_col(&doc, &laid, &st.cursor);
        assert!(rc.is_some(), "cursor maps at width {w}");
    }
}

/// Fullwidth CJK: wide cells at narrow width must remain drawable.
#[test]
fn wide_cjk_mappable_narrow() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "x\u{5168}y".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    for w in [40u16, 2, 1] {
        let laid = layout_document(&doc, w, &LayoutConfig::default());
        st.cursor.normalize(&doc);
        let rc = cursor_to_row_col(&doc, &laid, &st.cursor);
        assert!(rc.is_some(), "wide char doc should map at width {w}");
    }
    let n = document_char_count(&doc);
    assert_eq!(n, 3, "three scalar chars in 'x全y'");
    for g in 0..=n {
        if let Some(c) = gap_index_to_cursor(&doc, g) {
            let _ = cursor_to_gap_index(&doc, &c);
        }
    }
}

// --- Phase 4: viewport edge cases and scheduler ---

#[test]
fn viewport_line_range_empty_total() {
    let r = viewport_line_range(0, 10, 0);
    assert_eq!(r, 0..0);
}

#[test]
fn viewport_line_range_clamped_end() {
    assert_eq!(viewport_line_range(0, 5, 20), 0..5);
    assert_eq!(viewport_line_range(18, 10, 20), 18..20);
}

#[test]
fn merge_regions_both_empty() {
    assert!(merge_regions(&[], &[]).is_empty());
}

#[test]
fn merge_regions_non_overlapping_stay_split() {
    let m = merge_regions(&[0..3, 12..14], std::slice::from_ref(&(5..9)));
    assert_eq!(m, vec![0..3, 5..9, 12..14]);
}

#[test]
fn scroll_to_reveal_preserves_when_view_height_zero() {
    assert_eq!(scroll_to_reveal_row(100, 0, 42), 42);
}

#[test]
fn scheduler_drain_splits_oversized_task() {
    let mut s = RenderScheduler {
        queue: VecDeque::from(vec![RenderTask { range: 0..50 }]),
        budget_per_frame: 4096,
        max_queue_tasks: 2048,
        ..Default::default()
    };
    let processed = s.drain_within_budget(15);
    assert_eq!(processed, 15);
    assert_eq!(s.queue.front().unwrap().range, 15..50);
}

#[test]
fn schedule_render_merges_viewport_and_dirty() {
    let mut s = RenderScheduler::default();
    let vp = Viewport {
        top_index: 0,
        bottom_exclusive: 6,
        width: 40,
    };
    schedule_render(&mut s, &vp, std::slice::from_ref(&(3..10)));
    assert!(!s.queue.is_empty());
    assert_eq!(
        s.queue.front().unwrap().range,
        0..10,
        "0..6 union 3..10 should coalesce to 0..10"
    );
}

// --- Phase 5: vertical cursor moves and layout introspection ---

/// Two blocks avoid wrap-boundary ambiguity: the caret between "hello" and "world" is both
/// end-of-row0 and start-of-row1 when a single paragraph soft-wraps, and `cursor_to_row_col`
/// reports the earlier match — use stacked paragraphs for a deterministic vertical hop.
#[test]
fn apply_cursor_down_moves_to_next_visual_line_between_blocks() {
    let doc = Document::with_blocks(vec![
        Block::Paragraph(vec![Inline::text_str("hello")]),
        Block::Paragraph(vec![Inline::text_str("world")]),
    ]);
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 0),
        ..Default::default()
    };
    st.cursor.normalize(&doc);
    let w = 40u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    assert_eq!(
        cursor_to_gap_index(&doc, &st.cursor),
        Some(0),
        "start of doc"
    );
    let (_, col0) = cursor_to_row_col(&doc, &laid, &st.cursor).expect("maps");
    apply_cursor_move(&doc, w, &mut st, CursorMove::Down);
    assert_eq!(
        cursor_to_gap_index(&doc, &st.cursor),
        Some(5),
        "caret should sit at gap before first char of second block"
    );
    let (row1, col1) = cursor_to_row_col(&doc, &laid, &st.cursor).expect("after Down");
    let first_nonempty_row = laid
        .lines
        .iter()
        .find(|l| !l.cells.is_empty())
        .expect("first block line")
        .row;
    assert!(
        row1 > first_nonempty_row,
        "Down should advance to the second paragraph's visual line"
    );
    assert_eq!(col0, col1);
}

#[test]
fn apply_cursor_down_follows_soft_wrap_within_paragraph() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "abcdefghijklmnop".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    st.cursor = Cursor::new(0, vec![0], 0);
    st.cursor.normalize(&doc);
    // inner width = w - 2*H_PAD = 8 - 2 = 6 cells per visual line
    let w = 8u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    let rows_with_cells = laid.lines.iter().filter(|l| !l.cells.is_empty()).count();
    assert!(
        rows_with_cells >= 2,
        "16 ASCII letters at inner width 6 should soft-wrap to 2+ rows; got {rows_with_cells}"
    );
    let first_row_cells = laid
        .lines
        .iter()
        .find(|l| !l.cells.is_empty())
        .map(|l| l.cells.len())
        .expect("first visual line");
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::Down);
    assert_eq!(
        cursor_to_gap_index(&doc, &st.cursor),
        Some(first_row_cells),
        "Down should move to the start of the second wrapped line (same visual column)"
    );
}

#[test]
fn layout_extra_letter_spacing_tightens_wrap_matches_vertical_moves() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "abcdefgh".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    st.cursor = Cursor::new(0, vec![0], 0);
    st.cursor.normalize(&doc);
    let w = 8u16;
    let cfg = LayoutConfig {
        extra_letter_spacing: 1,
        ..Default::default()
    };
    // inner 6; each char layout width 2 => 3 chars per line, 3 lines for 8 chars
    let laid = layout_document(&doc, w, &cfg);
    let rows_with_cells = laid.lines.iter().filter(|l| !l.cells.is_empty()).count();
    assert_eq!(rows_with_cells, 3);
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::Down);
    assert_eq!(cursor_to_gap_index(&doc, &st.cursor), Some(3));
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::Down);
    assert_eq!(cursor_to_gap_index(&doc, &st.cursor), Some(6));
}

#[test]
fn apply_cursor_up_reverses_down_between_blocks() {
    let doc = Document::with_blocks(vec![
        Block::Paragraph(vec![Inline::text_str("hello")]),
        Block::Paragraph(vec![Inline::text_str("world")]),
    ]);
    let mut st = EditorState {
        cursor: Cursor::new(0, vec![0], 0),
        ..Default::default()
    };
    st.cursor.normalize(&doc);
    let w = 40u16;
    let before = st.cursor.clone();
    apply_cursor_move(&doc, w, &mut st, CursorMove::Down);
    assert_ne!(
        cursor_to_gap_index(&doc, &st.cursor),
        cursor_to_gap_index(&doc, &before)
    );
    apply_cursor_move(&doc, w, &mut st, CursorMove::Up);
    assert_eq!(st.cursor, before);
}

#[test]
fn apply_cursor_home_and_end_snap_to_visual_row_bounds_wide_layout() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "abcdefghi".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let w = 80u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    assert_eq!(
        laid.lines.iter().filter(|l| !l.cells.is_empty()).count(),
        1,
        "wide layout keeps the paragraph on one visual row"
    );
    let n = document_char_count(&doc);
    st.cursor = gap_index_to_cursor(&doc, n / 2).unwrap();
    st.cursor.normalize(&doc);
    let gap_mid = cursor_to_gap_index(&doc, &st.cursor).unwrap();
    assert!(gap_mid > 0 && gap_mid < n);

    apply_cursor_move(&doc, w, &mut st, CursorMove::Home);
    assert_eq!(cursor_to_gap_index(&doc, &st.cursor), Some(0));

    apply_cursor_move(&doc, w, &mut st, CursorMove::End);
    assert_eq!(cursor_to_gap_index(&doc, &st.cursor), Some(n));
}

#[test]
fn document_char_starts_len_matches_flatten_count_for_mixed_blocks() {
    let doc = Document::with_blocks(vec![
        Block::Paragraph(vec![Inline::text_str("ab")]),
        Block::Heading {
            level: 2,
            content: vec![Inline::text_str("c")],
        },
        Block::CodeBlock {
            lang: None,
            text: "de".to_string(),
        },
    ]);
    let n = document_char_count(&doc);
    assert_eq!(
        crate::edit::document_char_starts(&doc).len(),
        n,
        "one start cursor per flattened character across block types"
    );
}

#[test]
fn layout_block_gap_inserts_exact_blank_visual_lines_between_blocks() {
    let doc = Document::with_blocks(vec![
        Block::Paragraph(vec![Inline::text_str("a")]),
        Block::Paragraph(vec![Inline::text_str("b")]),
    ]);
    let cfg0 = LayoutConfig::default();
    let cfg_gap = LayoutConfig {
        block_gap_lines: 2,
        ..LayoutConfig::default()
    };
    let laid0 = layout_document(&doc, 40, &cfg0);
    let laid1 = layout_document(&doc, 40, &cfg_gap);
    assert_eq!(laid1.lines.len(), laid0.lines.len() + 2);
}

#[test]
fn layout_line_gap_adds_blank_rows_between_wrapped_visual_lines() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "abcdefghi".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let w = 6u16;
    let cfg0 = LayoutConfig::default();
    let cfg_gap = LayoutConfig {
        line_gap_lines: 1,
        ..LayoutConfig::default()
    };
    let laid0 = layout_document(&doc, w, &cfg0);
    let laid1 = layout_document(&doc, w, &cfg_gap);
    let wraps = laid0
        .lines
        .iter()
        .filter(|l| !l.cells.is_empty())
        .count()
        .saturating_sub(1);
    assert!(
        wraps >= 2,
        "setup should produce multiple wrapped content rows; got {wraps}"
    );
    assert_eq!(
        laid1.lines.len(),
        laid0.lines.len() + wraps * cfg_gap.line_gap_lines
    );
}

#[test]
fn laid_out_document_total_rows_and_display_width() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar('x')));
    let laid = layout_document(&doc, 40, &LayoutConfig::default());
    assert_eq!(laid.total_rows(), laid.lines.len());
    let nonempty = laid
        .lines
        .iter()
        .find(|l| !l.cells.is_empty())
        .expect("single char yields one content line");
    let row_idx = nonempty.row;
    let expect_w: usize = nonempty.gutter
        + nonempty
            .cells
            .iter()
            .map(|(_, ch, _)| ch.width().unwrap_or(0))
            .sum::<usize>();
    assert_eq!(
        laid.display_width(row_idx, &LayoutConfig::default()),
        expect_w
    );
}

#[test]
fn apply_cursor_move_in_layout_matches_cached_layout() {
    let mut doc = Document::new();
    let mut st = EditorState::default();
    for ch in "abc".chars() {
        assert!(reduce_edit(&mut doc, &mut st, EditOp::InsertChar(ch)));
    }
    let w = 12u16;
    let laid = layout_document(&doc, w, &LayoutConfig::default());
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::End);
    apply_cursor_move_in_layout(&doc, &laid, &mut st, CursorMove::Home);
    assert_eq!(
        cursor_to_gap_index(&doc, &st.cursor),
        Some(0),
        "Home at line start resets to gap 0 for single-line ASCII"
    );
}
