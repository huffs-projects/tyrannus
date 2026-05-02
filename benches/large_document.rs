//! Phase 7 smoke benchmark: hot paths the editor calls every frame on a large doc.
//!
//! Exercises:
//!   - cold full-document layout
//!   - per-keystroke `LayoutCache::sync` (full relayout under the current model)
//!   - `flatten_document_chars` + `cursor_to_gap_index` (used by selection-extending moves)
//!   - the per-frame body-span build loop equivalent (scrolled deep into the document)

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

use tyrannus::{
    cursor_to_gap_index, layout_document, reduce_edit, Block, Document, EditOp, EditorState,
    Inline, LaidOutDocument, LayoutCache, LayoutConfig,
};

const PARAGRAPH_TEXT: &str =
    "The quick brown fox jumps over the lazy dog while a punctilious tyrant\
     observes from the parapet, taking notes on every uncoordinated bound.";
const NUM_PARAGRAPHS: usize = 10_000;
const FRAME_WIDTH: u16 = 80;
const INNER_H: usize = 50;
const SCROLL_TOP: usize = 1_000;

fn build_large_doc() -> Document {
    let blocks: Vec<Block> = (0..NUM_PARAGRAPHS)
        .map(|_| Block::Paragraph(vec![Inline::Text(PARAGRAPH_TEXT.to_string())]))
        .collect();
    Document::with_blocks(blocks)
}

/// Mirrors the per-row loop inside `build_ui_model` in `src/main.rs` without pulling in
/// the binary crate or a real ratatui frame: we touch every cell + cursor predicate that
/// the painter would touch and force the result to escape via `black_box`.
fn render_visible_cells(laid: &LaidOutDocument, scroll_top: usize, inner_h: usize) -> usize {
    let mut total_cells = 0usize;
    for screen_row in 0..inner_h {
        let global_row = scroll_top + screen_row;
        let Some(line) = laid.lines.get(global_row) else {
            continue;
        };
        for (i, (_c, ch, gidx)) in line.cells.iter().enumerate() {
            let cursor_here = i == 0;
            let in_sel = *gidx % 7 == 0;
            black_box((cursor_here, in_sel, *ch));
            total_cells += 1;
        }
    }
    total_cells
}

fn bench_layout_cold(c: &mut Criterion) {
    let doc = build_large_doc();
    let cfg = LayoutConfig::default();
    c.bench_function("layout_document_cold_10k_paragraphs_w80", |b| {
        b.iter(|| {
            let laid = layout_document(black_box(&doc), black_box(FRAME_WIDTH), black_box(&cfg));
            black_box(laid);
        });
    });
}

fn bench_cache_sync_after_edit(c: &mut Criterion) {
    let cfg = LayoutConfig::default();
    let mut group = c.benchmark_group("layout_cache_sync");
    group.bench_function(
        BenchmarkId::new("after_single_insert", NUM_PARAGRAPHS),
        |b| {
            b.iter_batched(
                || {
                    let mut doc = build_large_doc();
                    let mut state = EditorState::default();
                    state.cursor.normalize(&doc);
                    let mut cache = LayoutCache::default();
                    cache.sync(&doc, FRAME_WIDTH, &cfg);
                    let _ = reduce_edit(&mut doc, &mut state, EditOp::InsertChar('x'));
                    (doc, cache)
                },
                |(doc, mut cache)| {
                    cache.sync(black_box(&doc), black_box(FRAME_WIDTH), black_box(&cfg));
                    black_box(cache.laid().lines.len());
                },
                criterion::BatchSize::SmallInput,
            );
        },
    );
    group.finish();
}

fn bench_flatten_and_gap(c: &mut Criterion) {
    let mut doc = build_large_doc();
    let mut state = EditorState::default();
    state.cursor.normalize(&doc);
    let _ = reduce_edit(&mut doc, &mut state, EditOp::InsertChar('z'));

    let mut group = c.benchmark_group("flatten_and_gap");
    group.bench_function("flatten_document_chars", |b| {
        b.iter(|| {
            let flat = tyrannus::flatten_document_chars(black_box(&doc));
            black_box(flat.len());
        });
    });
    group.bench_function("cursor_to_gap_index", |b| {
        b.iter(|| {
            let g = cursor_to_gap_index(black_box(&doc), black_box(&state.cursor));
            black_box(g);
        });
    });
    group.finish();
}

fn bench_visible_frame(c: &mut Criterion) {
    let doc = build_large_doc();
    let cfg = LayoutConfig::default();
    let laid = layout_document(&doc, FRAME_WIDTH, &cfg);
    let total_rows = laid.lines.len();
    let scroll_top = SCROLL_TOP.min(total_rows.saturating_sub(INNER_H));

    c.bench_function("frame_body_spans_50_rows_scrolled_1k", |b| {
        b.iter(|| {
            let n =
                render_visible_cells(black_box(&laid), black_box(scroll_top), black_box(INNER_H));
            black_box(n);
        });
    });
}

criterion_group!(
    benches,
    bench_layout_cold,
    bench_cache_sync_after_edit,
    bench_flatten_and_gap,
    bench_visible_frame,
);
criterion_main!(benches);
