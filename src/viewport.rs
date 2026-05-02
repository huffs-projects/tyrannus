//! Viewport line range, region merge, layout cache, and render scheduler scaffolding.
//!
//! Full-document layout is still computed in one shot on cache miss; chunked
//! `layout_document_range` is deferred until the layout engine can process ranges.

use std::collections::VecDeque;
use std::ops::Range;

use crate::document::Document;
use crate::layout::{layout_document, LaidOutDocument, LayoutConfig};

/// Visible slice in **visual line index** space (`LaidOutLine.row` is dense `0..lines.len()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub top_index: usize,
    pub bottom_exclusive: usize,
    pub width: u16,
}

impl Viewport {
    pub fn line_range(&self) -> Range<usize> {
        self.top_index..self.bottom_exclusive
    }
}

/// Union of visible and dirty ranges, sorted and coalesced.
pub fn merge_regions(visible: &[Range<usize>], dirty: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut v: Vec<Range<usize>> = visible.iter().chain(dirty.iter()).cloned().collect();
    if v.is_empty() {
        return Vec::new();
    }
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in v {
        if let Some(last) = out.last_mut() {
            if r.start <= last.end {
                last.end = last.end.max(r.end);
            } else {
                out.push(r);
            }
        } else {
            out.push(r);
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderTask {
    pub range: Range<usize>,
}

#[derive(Debug)]
pub struct RenderScheduler {
    pub last_frame_processed: usize,
    pub queue: VecDeque<RenderTask>,
    pub budget_per_frame: usize,
    pub max_queue_tasks: usize,
}

impl Default for RenderScheduler {
    fn default() -> Self {
        Self {
            last_frame_processed: 0,
            queue: VecDeque::new(),
            budget_per_frame: 4096,
            max_queue_tasks: 2048,
        }
    }
}

impl RenderScheduler {
    /// Pop tasks until at most `budget` visual-line indices would be processed; split a task if needed.
    pub fn drain_within_budget(&mut self, budget: usize) -> usize {
        let mut processed = 0usize;
        while processed < budget {
            let Some(task) = self.queue.pop_front() else {
                break;
            };
            let size = task.range.end.saturating_sub(task.range.start);
            if size == 0 {
                continue;
            }
            if processed + size <= budget {
                processed += size;
            } else {
                let take = budget.saturating_sub(processed);
                let mid = task.range.start + take;
                if mid < task.range.end {
                    self.queue.push_front(RenderTask {
                        range: mid..task.range.end,
                    });
                }
                processed = budget;
                break;
            }
        }
        processed
    }
}

/// Enqueue merged visible + dirty visual-line ranges for later draining (see [`RenderScheduler::drain_within_budget`]).
pub fn schedule_render(
    scheduler: &mut RenderScheduler,
    viewport: &Viewport,
    dirty: &[Range<usize>],
) {
    let vr = viewport.line_range();
    let merged = merge_regions(std::slice::from_ref(&vr), dirty);
    for r in merged {
        scheduler.queue.push_back(RenderTask { range: r });
    }
    while scheduler.queue.len() > scheduler.max_queue_tasks {
        let _ = scheduler.queue.pop_back();
    }
}

pub fn viewport_line_range(
    scroll_top: usize,
    view_height: usize,
    total_lines: usize,
) -> Range<usize> {
    if total_lines == 0 {
        return 0..0;
    }
    let end = (scroll_top + view_height).min(total_lines);
    scroll_top..end
}

pub fn clamp_scroll(scroll_top: usize, view_height: usize, total_lines: usize) -> usize {
    if total_lines == 0 || view_height == 0 {
        return 0;
    }
    if total_lines <= view_height {
        return 0;
    }
    let max_scroll = total_lines - view_height;
    scroll_top.min(max_scroll)
}

/// Keep `cursor_row` inside `[scroll_top, scroll_top + view_height)`.
pub fn scroll_to_reveal_row(cursor_row: usize, view_height: usize, scroll_top: usize) -> usize {
    if view_height == 0 {
        return scroll_top;
    }
    if cursor_row < scroll_top {
        return cursor_row;
    }
    if cursor_row >= scroll_top.saturating_add(view_height) {
        return cursor_row + 1 - view_height;
    }
    scroll_top
}

/// Cached [`LaidOutDocument`] keyed by `Document::generation` and terminal width.
#[derive(Debug, Default)]
pub struct LayoutCache {
    inner: Option<(LaidOutDocument, u16, u64)>,
    pub dirty_ranges: Vec<Range<usize>>,
    pub scheduler: RenderScheduler,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutMemoryStats {
    pub line_count: usize,
    pub cell_count: usize,
    pub approx_bytes: usize,
    pub queued_tasks: usize,
}

impl LayoutCache {
    /// Rebuild layout when `generation` or `width` changes.
    pub fn sync(&mut self, doc: &Document, width: u16, cfg: &LayoutConfig) -> &LaidOutDocument {
        let need = match &self.inner {
            None => true,
            Some((_, w, g)) => *w != width || *g != doc.generation,
        };
        if need {
            let laid = layout_document(doc, width.max(1), cfg);
            let n = laid.lines.len();
            self.dirty_ranges = std::iter::once(0..n).collect();
            self.inner = Some((laid, width, doc.generation));

            self.scheduler.queue.clear();
            schedule_render(
                &mut self.scheduler,
                &Viewport {
                    top_index: 0,
                    bottom_exclusive: n,
                    width,
                },
                &self.dirty_ranges,
            );
            self.scheduler.last_frame_processed = 0;
        } else {
            self.dirty_ranges.clear();
        }
        &self.inner.as_ref().expect("sync always populates").0
    }

    pub fn laid(&self) -> &LaidOutDocument {
        &self.inner.as_ref().expect("call sync first").0
    }

    pub fn process_frame(&mut self, viewport: &Viewport) -> usize {
        let near_pad = viewport
            .bottom_exclusive
            .saturating_sub(viewport.top_index)
            .max(8);
        let near_start = viewport.top_index.saturating_sub(near_pad);
        let near_end = viewport.bottom_exclusive.saturating_add(near_pad);
        // Prioritize visible and near-viewport rows first.
        self.scheduler.queue.push_front(RenderTask {
            range: near_start..near_end,
        });
        self.scheduler.queue.push_front(RenderTask {
            range: viewport.top_index..viewport.bottom_exclusive,
        });
        while self.scheduler.queue.len() > self.scheduler.max_queue_tasks {
            let _ = self.scheduler.queue.pop_back();
        }
        let processed = self
            .scheduler
            .drain_within_budget(self.scheduler.budget_per_frame.max(1));
        self.scheduler.last_frame_processed = processed;
        processed
    }

    pub fn memory_stats(&self) -> LayoutMemoryStats {
        let Some((laid, _, _)) = &self.inner else {
            return LayoutMemoryStats::default();
        };
        let cell_count = laid.lines.iter().map(|l| l.cells.len()).sum::<usize>();
        let approx_bytes = cell_count
            .saturating_mul(std::mem::size_of::<crate::layout::LaidCell>())
            .saturating_add(
                laid.lines
                    .len()
                    .saturating_mul(std::mem::size_of::<crate::layout::LaidOutLine>()),
            );
        LayoutMemoryStats {
            line_count: laid.lines.len(),
            cell_count,
            approx_bytes,
            queued_tasks: self.scheduler.queue.len(),
        }
    }
}
