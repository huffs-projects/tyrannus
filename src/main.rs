//! Tyrannus TUI — minimal event loop (phase 0+).
#![allow(
    clippy::too_many_arguments,
    clippy::manual_is_multiple_of,
)]

use std::fs;
use std::fs::OpenOptions;
use std::io::{self, stdout, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::{Frame, Terminal};
use toml::Value;

mod config;
mod theme_presets;

use crate::config::{
    load_app_config_from_path, theme_color_in, AppConfig, CursorConfig, CursorStyleConfig, Theme,
    ThemeRole, TypographyConfig,
};
use tyrannus::{
    apply_cursor_move_in_layout, clamp_scroll, cursor_to_gap_index, cursor_to_row_col, reduce_edit,
    scroll_to_reveal_row, selection_ordered, Block as DocBlock, CursorMove, Document, EditOp,
    EditorState, Inline, LayoutCache, LayoutConfig, Selection,
};

struct TermGuard {
    mouse_captured: bool,
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut out = stdout();
        if self.mouse_captured {
            let _ = execute!(out, DisableMouseCapture);
        }
        let _ = execute!(out, LeaveAlternateScreen);
    }
}

/// Best-effort terminal restore used by both the panic hook and Drop.
/// Idempotent: safe to call when raw mode or alt-screen is not active.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let mut out = stdout();
    let _ = execute!(out, DisableMouseCapture);
    let _ = execute!(out, LeaveAlternateScreen);
}

/// Install a panic hook that restores the terminal *before* the default hook
/// prints the panic message. Required because panics inside `terminal.draw`
/// closures can leave the alt-screen + raw mode active even with `Drop`.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayMode {
    None,
    CommandPalette,
    Menu,
    RecentDocuments,
    WritingFolder,
    NewDocumentFilename,
    Configuration,
    Help,
}

#[derive(Clone, Debug)]
struct UiState {
    overlay: OverlayMode,
    new_document_filename_input: String,
    palette_query: String,
    palette_selected: usize,
    menu_selected: usize,
    list_selected: usize,
    start_menu_title_lines: Vec<String>,
    /// True when crossterm `EnableMouseCapture` succeeded at startup.
    mouse_enabled: bool,
    /// Toggles verbose diagnostic status details.
    status_details: bool,
    /// Hides bordered frame, title, and status line (toggle with Ctrl+K).
    chrome_hidden: bool,
    /// Save result dialog content; painted above menu or editor until dismissed (Esc / Enter).
    save_feedback: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            overlay: OverlayMode::Menu,
            new_document_filename_input: String::new(),
            palette_query: String::new(),
            palette_selected: 0,
            menu_selected: 0,
            list_selected: 0,
            start_menu_title_lines: Vec::new(),
            mouse_enabled: true,
            status_details: false,
            chrome_hidden: false,
            save_feedback: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppCommand {
    NewDocument,
    RecentDocuments,
    WritingFolder,
    Configuration,
    ShowHelp,
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemapAction {
    Quit,
    ToggleChromeHidden,
    ToggleStatusDetails,
    OpenPalette,
    OpenMenu,
    OpenHelp,
    Command(AppCommand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
}

#[derive(Clone, Debug, Default)]
struct RuntimeKeymap {
    bindings: Vec<(KeyBinding, RemapAction)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverlayAction {
    Execute(AppCommand),
    OpenSelectedRecent,
    OpenSelectedWriting,
    RetryConfiguration,
    CreateConfiguration,
    ConfirmNewDocumentBasename,
}

#[derive(Clone, Debug, Default)]
struct AppState {
    current_document_path: Option<PathBuf>,
    recent_documents: Vec<PathBuf>,
    writing_folder_entries: Vec<PathBuf>,
    /// When set and the Writing folder overlay is shown, replaces the usual empty-folder message.
    writing_folder_overlay_error: Option<String>,
    status_message: Option<String>,
    recovery_path: PathBuf,
    edit_counter: usize,
}

const SNAPSHOT_EVERY_EDITS: usize = 40;

#[derive(Clone, Debug)]
struct UiModel {
    body_lines: Vec<Line<'static>>,
    status: Line<'static>,
}

/// Inner body rect for laying out wrapped text — full area when chrome is hidden,
/// otherwise bordered block above a one-cell status strip.
fn body_inner_rect(area: Rect, chrome_hidden: bool) -> Rect {
    if chrome_hidden {
        return area;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    Block::default().borders(Borders::ALL).inner(chunks[0])
}

/// Wraps [`body_inner_rect`] for the layout pipeline: returns at least `(1, 1)`
/// even when the terminal is too small to show the editor (the painter switches
/// to a "terminal too small" hint in that case; see [`paint_too_small`]).
fn body_inner_dims(area: Rect, chrome_hidden: bool) -> (u16, usize) {
    let inner = body_inner_rect(area, chrome_hidden);
    (inner.width.max(1), (inner.height as usize).max(1))
}

fn area_is_usable(area: Rect, chrome_hidden: bool) -> bool {
    let inner = body_inner_rect(area, chrome_hidden);
    inner.width >= 1 && inner.height >= 1
}

fn main() -> ExitCode {
    install_panic_hook();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // `TermGuard` has already dropped by here, so the alt-screen is gone
            // and we can write a clean stderr line instead of a Rust backtrace.
            let mut err_out = io::stderr().lock();
            let _ = writeln!(err_out, "tyrannus: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> io::Result<()> {
    theme_presets::validate_bundled_presets()
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    let app_config = load_app_config_from_path(default_configuration_path().as_path())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    let theme = app_config.theme.clone();

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Mouse capture is best-effort: some terminals (or remote sessions) refuse
    // it. Continue without scroll-wheel support if the request fails.
    let mouse_captured = execute!(stdout, EnableMouseCapture).is_ok();
    let _guard = TermGuard { mouse_captured };

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut doc = Document::new();
    let mut state = EditorState::default();
    state.cursor.normalize(&doc);
    let mut cache = LayoutCache::default();
    let cfg = LayoutConfig {
        line_gap_lines: app_config.spacing.line_gap_lines,
        block_gap_lines: app_config.spacing.paragraph_gap_lines,
        code_margin: app_config.spacing.code_margin,
        extra_word_spacing: app_config.typography.extra_word_spacing,
        extra_letter_spacing: app_config.typography.extra_letter_spacing,
    };
    let mut app_state = AppState {
        recovery_path: default_recovery_path(),
        ..AppState::default()
    };
    let runtime_keymap = parse_runtime_keymap(&app_config)?;
    let mut ui_state = UiState {
        mouse_enabled: mouse_captured,
        status_details: app_config.ui.status_details_default,
        ..UiState::default()
    };
    // Always launch into the start menu regardless of future default changes.
    ui_state.overlay = OverlayMode::Menu;
    ui_state.menu_selected = 0;
    ui_state.start_menu_title_lines =
        load_start_menu_title_lines(default_configuration_path().as_path(), &app_config);
    maybe_restore_recovery_snapshot(
        &mut doc,
        &mut state,
        &mut cache,
        &cfg,
        &mut app_state,
    );

    loop {
        let mut frame_area: Rect = Rect::new(0, 0, 0, 0);
        terminal.draw(|f| {
            frame_area = f.area();
            if !area_is_usable(frame_area, ui_state.chrome_hidden) {
                paint_too_small(f, &theme);
                return;
            }
            if ui_state.overlay == OverlayMode::Menu {
                paint_start_menu(f, &ui_state, &theme);
                paint_save_feedback_modal(f, &ui_state, &theme);
                return;
            }
            let (inner_w, inner_h) = body_inner_dims(frame_area, ui_state.chrome_hidden);
            cache.sync(&doc, inner_w, &cfg);
            let model = build_ui_model(
                f,
                &doc,
                &state,
                cache.laid(),
                &cache.memory_stats(),
                inner_h,
                &ui_state,
                &app_state,
                &theme,
                &app_config.cursor,
                &app_config.typography,
                &app_config.editor,
            );
            paint_editor(
                f,
                model,
                &ui_state,
                &app_state,
                &theme,
                &app_config.cursor,
                &app_config.typography,
                &app_config.editor,
                app_config.paths.writing_folder.as_path(),
            );
        })?;

        let ev = match event::read() {
            Ok(ev) => ev,
            // Signals can interrupt the read syscall (e.g. SIGWINCH delivered
            // racily on some terminals); spin once instead of bailing out.
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        let (inner_w, inner_h) = body_inner_dims(frame_area, ui_state.chrome_hidden);
        cache.sync(&doc, inner_w, &cfg);

        let mut cursor_moved = false;
        let mut scroll_only = false;
        let mut should_quit = false;

        match ev {
            Event::Resize(_, _) => {
                let _ = event::poll(std::time::Duration::from_millis(5));
                cursor_moved = true;
            }
            Event::Mouse(me) => match me.kind {
                MouseEventKind::ScrollUp => {
                    scroll_only = true;
                    state.scroll_top = state.scroll_top.saturating_sub(3);
                }
                MouseEventKind::ScrollDown => {
                    scroll_only = true;
                    state.scroll_top = state.scroll_top.saturating_add(3);
                }
                _ => {}
            },
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if ui_state.save_feedback.is_some() {
                    if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
                        ui_state.save_feedback = None;
                    }
                    continue;
                }
                let toggle_chrome = key.code == KeyCode::Char('k')
                    && key.modifiers.contains(KeyModifiers::CONTROL);
                if toggle_chrome {
                    apply_toggle_chrome_hidden(&mut ui_state, &mut app_state);
                } else if let Some(action) =
                    handle_overlay_key(key.code, key.modifiers, &mut ui_state, &app_state)
                {
                    if execute_overlay_action(
                        action,
                        &mut doc,
                        &mut state,
                        &mut cache,
                        &cfg,
                        &mut ui_state,
                        &mut app_state,
                        app_config.paths.writing_folder.as_path(),
                    ) {
                        should_quit = true;
                        cursor_moved = true;
                    }
                } else {
                    if let Some(remap) =
                        lookup_remap_action(&runtime_keymap, key.code, key.modifiers)
                    {
                        match remap {
                            RemapAction::Quit => {
                                should_quit = true;
                            }
                            RemapAction::OpenPalette => ui_state.overlay = OverlayMode::CommandPalette,
                            RemapAction::OpenMenu => ui_state.overlay = OverlayMode::Menu,
                            RemapAction::OpenHelp => ui_state.overlay = OverlayMode::Help,
                            RemapAction::ToggleChromeHidden => {
                                apply_toggle_chrome_hidden(&mut ui_state, &mut app_state);
                            }
                            RemapAction::ToggleStatusDetails => {
                                ui_state.status_details = !ui_state.status_details;
                                app_state.status_message = Some(if ui_state.status_details {
                                    "Status details enabled".to_string()
                                } else {
                                    "Status details hidden".to_string()
                                });
                            }
                            RemapAction::Command(cmd) => {
                                execute_command(
                                    cmd,
                                    &mut doc,
                                    &mut state,
                                    &mut cache,
                                    &cfg,
                                    &mut ui_state,
                                    &mut app_state,
                                    app_config.paths.writing_folder.as_path(),
                                );
                            }
                        }
                        if should_quit {
                            break;
                        }
                        continue;
                    }
                    #[allow(clippy::collapsible_match)]
                    match key.code {
                        KeyCode::Char('q')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            should_quit = true;
                        }
                        KeyCode::Char('p')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            ui_state.overlay = OverlayMode::CommandPalette;
                        }
                        KeyCode::Char('m')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            ui_state.overlay = OverlayMode::Menu;
                        }
                        KeyCode::Char('s')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            save_current_document(&doc, &mut ui_state, &mut app_state);
                        }
                        _ if key_opens_help(key.code, key.modifiers) => {
                            ui_state.overlay = OverlayMode::Help;
                        }
                        KeyCode::Esc => {
                            ui_state.overlay = OverlayMode::Menu;
                        }
                        KeyCode::F(2) => {
                            ui_state.status_details = !ui_state.status_details;
                            app_state.status_message = Some(if ui_state.status_details {
                                "Status details enabled".to_string()
                            } else {
                                "Status details hidden".to_string()
                            });
                        }
                        KeyCode::Char(c) => {
                            if reduce_edit(&mut doc, &mut state, EditOp::InsertChar(c)) {
                                clear_status(&mut app_state);
                                record_edit_and_snapshot(&doc, &mut app_state);
                                cursor_moved = true;
                            }
                        }
                        KeyCode::Backspace => {
                            if reduce_edit(&mut doc, &mut state, EditOp::Backspace) {
                                clear_status(&mut app_state);
                                record_edit_and_snapshot(&doc, &mut app_state);
                                cursor_moved = true;
                            }
                        }
                        KeyCode::Enter => {
                            if reduce_edit(&mut doc, &mut state, EditOp::NewLine) {
                                clear_status(&mut app_state);
                                record_edit_and_snapshot(&doc, &mut app_state);
                                cursor_moved = true;
                            }
                        }
                        KeyCode::Tab => {
                            if app_config.editor.hard_tabs {
                                if reduce_edit(&mut doc, &mut state, EditOp::InsertChar('\t')) {
                                    clear_status(&mut app_state);
                                    record_edit_and_snapshot(&doc, &mut app_state);
                                    cursor_moved = true;
                                }
                            } else {
                                for _ in 0..app_config.editor.tab_width {
                                    if reduce_edit(&mut doc, &mut state, EditOp::InsertChar(' ')) {
                                        clear_status(&mut app_state);
                                        record_edit_and_snapshot(&doc, &mut app_state);
                                        cursor_moved = true;
                                    }
                                }
                            }
                        }
                        KeyCode::PageUp => {
                            scroll_only = true;
                            let step = inner_h.saturating_sub(1).max(1);
                            state.scroll_top = state.scroll_top.saturating_sub(step);
                        }
                        KeyCode::PageDown => {
                            scroll_only = true;
                            let step = inner_h.saturating_sub(1).max(1);
                            state.scroll_top = state.scroll_top.saturating_add(step);
                        }
                        KeyCode::Left => {
                            let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                            apply_move_with_selection(
                                &doc,
                                cache.laid(),
                                &mut state,
                                CursorMove::Left,
                                extend,
                            );
                            cursor_moved = true;
                        }
                        KeyCode::Right => {
                            let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                            apply_move_with_selection(
                                &doc,
                                cache.laid(),
                                &mut state,
                                CursorMove::Right,
                                extend,
                            );
                            cursor_moved = true;
                        }
                        KeyCode::Up => {
                            let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                            apply_move_with_selection(
                                &doc,
                                cache.laid(),
                                &mut state,
                                CursorMove::Up,
                                extend,
                            );
                            cursor_moved = true;
                        }
                        KeyCode::Down => {
                            let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                            apply_move_with_selection(
                                &doc,
                                cache.laid(),
                                &mut state,
                                CursorMove::Down,
                                extend,
                            );
                            cursor_moved = true;
                        }
                        KeyCode::Home => {
                            let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                            apply_move_with_selection(
                                &doc,
                                cache.laid(),
                                &mut state,
                                CursorMove::Home,
                                extend,
                            );
                            cursor_moved = true;
                        }
                        KeyCode::End => {
                            let extend = key.modifiers.contains(KeyModifiers::SHIFT);
                            apply_move_with_selection(
                                &doc,
                                cache.laid(),
                                &mut state,
                                CursorMove::End,
                                extend,
                            );
                            cursor_moved = true;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        if should_quit {
            break;
        }

        if area_is_usable(frame_area, ui_state.chrome_hidden) {
            cache.sync(&doc, inner_w, &cfg);
            let total = cache.laid().lines.len();
            state.scroll_top = clamp_scroll(state.scroll_top, inner_h, total);
            let viewport = tyrannus::Viewport {
                top_index: state.scroll_top,
                bottom_exclusive: (state.scroll_top + inner_h).min(total),
                width: inner_w,
            };
            let _ = cache.process_frame(&viewport);
            if cursor_moved && !scroll_only {
                if let Some((row, _)) = cursor_to_row_col(&doc, cache.laid(), &state.cursor) {
                    state.scroll_top = scroll_to_reveal_row(row, inner_h, state.scroll_top);
                }
                state.scroll_top = clamp_scroll(state.scroll_top, inner_h, total);
            }
        }
    }

    let _ = fs::remove_file(&app_state.recovery_path);

    Ok(())
}

fn apply_move_with_selection(
    doc: &Document,
    laid: &tyrannus::LaidOutDocument,
    state: &mut EditorState,
    m: CursorMove,
    extend: bool,
) {
    let old_gap = cursor_to_gap_index(doc, &state.cursor);
    if !extend {
        state.selection = None;
    }
    apply_cursor_move_in_layout(doc, laid, state, m);
    if extend {
        if let (Some(og), Some(ng)) = (old_gap, cursor_to_gap_index(doc, &state.cursor)) {
            match &mut state.selection {
                Some(sel) => sel.head = ng,
                None => {
                    state.selection = Some(Selection {
                        anchor: og,
                        head: ng,
                    });
                }
            }
        }
    }
}

fn build_ui_model(
    f: &Frame,
    doc: &Document,
    state: &EditorState,
    laid: &tyrannus::LaidOutDocument,
    mem_stats: &tyrannus::LayoutMemoryStats,
    inner_h: usize,
    ui_state: &UiState,
    app_state: &AppState,
    theme: &Theme,
    cursor_cfg: &CursorConfig,
    typography: &TypographyConfig,
    editor: &crate::config::EditorConfig,
) -> UiModel {
    let sel_range = state.selection.as_ref().and_then(|s| {
        let (lo, hi) = selection_ordered(s);
        (lo < hi).then_some((lo, hi))
    });

    let cursor_rc = cursor_to_row_col(doc, laid, &state.cursor);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for screen_row in 0..inner_h {
        let global_row = state.scroll_top + screen_row;
        let Some(ll) = laid.lines.get(global_row) else {
            lines.push(Line::from(vec![Span::raw(" ".repeat(H_PAD))]));
            continue;
        };
        let mut spans: Vec<Span> = Vec::new();
        spans.push(Span::raw(" ".repeat(H_PAD)));
        if ll.prefix.is_empty() {
            spans.push(Span::raw(" ".repeat(ll.gutter)));
        } else {
            spans.push(Span::styled(
                ll.prefix.clone(),
                Style::default().fg(theme_color_in(theme, ThemeRole::Accent)),
            ));
        }
        for (i, (_c, ch, gidx)) in ll.cells.iter().enumerate() {
            let is_cursor_here = cursor_rc
                .map(|(r, col)| r == ll.row && col == ll.gutter + i)
                .unwrap_or(false);
            let in_sel = sel_range
                .map(|(lo, hi)| *gidx >= lo && *gidx < hi)
                .unwrap_or(false);
            let st = resolve_cell_style(is_cursor_here, in_sel, theme, cursor_cfg);
            let mut display = if editor.show_invisibles && *ch == ' ' {
                "·".to_string()
            } else {
                ch.to_string()
            };
            if *ch == ' ' {
                display.push_str(&" ".repeat(typography.extra_word_spacing));
            } else {
                display.push_str(&" ".repeat(typography.extra_letter_spacing));
            }
            spans.push(Span::styled(display, st));
        }
        if cursor_rc == Some((ll.row, ll.gutter + ll.cells.len())) {
            spans.push(Span::styled(
                " ",
                resolve_cell_style(true, false, theme, cursor_cfg),
            ));
        }
        lines.push(Line::from(spans));
    }

    let mut status_spans = vec![
        Span::raw(" insert "),
        Span::raw(format!(
            " | {} ",
            app_state
                .current_document_path
                .as_ref()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .unwrap_or_else(|| "(unsaved)".to_string())
        )),
        Span::raw(format!(" | {}", overlay_mode_name(ui_state.overlay))),
        Span::raw(format!(
            " | {}",
            app_state.status_message.as_deref().unwrap_or("ready")
        )),
    ];
    if ui_state.status_details {
        status_spans.push(Span::raw(format!(
            " | {}×{}",
            f.area().width,
            f.area().height
        )));
        status_spans.push(Span::raw(format!(
            " | mem:{}L/{}C/{}K/{}Q",
            mem_stats.line_count,
            mem_stats.cell_count,
            mem_stats.approx_bytes / 1024,
            mem_stats.queued_tasks
        )));
        status_spans.push(Span::raw(if ui_state.mouse_enabled {
            " | mouse:on"
        } else {
            " | mouse:off"
        }));
    } else {
        status_spans.push(Span::raw(
            " | Ctrl+P palette | Ctrl+H help | Ctrl+S save | Ctrl+K hide chrome | F2 details",
        ));
    }
    let status = Line::from(status_spans);

    UiModel {
        body_lines: lines,
        status,
    }
}

fn paint_too_small(f: &mut Frame, theme: &Theme) {
    let area = f.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let msg = if area.width >= 24 {
        "terminal too small"
    } else if area.width >= 8 {
        "too small"
    } else {
        "..."
    };
    let line = Line::from(Span::styled(
        msg,
        Style::default()
            .fg(theme_color_in(theme, ThemeRole::Foreground))
            .bg(theme_color_in(theme, ThemeRole::Background)),
    ));
    let widget = Paragraph::new(line)
        .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background)));
    f.render_widget(widget, area);
}

fn paint_editor(
    f: &mut Frame,
    model: UiModel,
    ui_state: &UiState,
    app_state: &AppState,
    theme: &Theme,
    cursor_cfg: &CursorConfig,
    typography: &TypographyConfig,
    editor: &crate::config::EditorConfig,
    writing_root: &Path,
) {
    let area = f.area();
    let body_style = Style::default()
        .fg(theme_color_in(theme, ThemeRole::Foreground))
        .bg(theme_color_in(theme, ThemeRole::Background));

    if ui_state.chrome_hidden {
        let body = Paragraph::new(model.body_lines).style(body_style);
        f.render_widget(body, area);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let title = if ui_state.mouse_enabled {
            " tyrannus — PgUp/PgDn scroll, wheel, Ctrl+Q quit "
        } else {
            " tyrannus — PgUp/PgDn scroll, Ctrl+Q quit (no mouse) "
        };
        let body = Paragraph::new(model.body_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent))),
            )
            .style(body_style);
        f.render_widget(body, chunks[0]);

        let status = Paragraph::new(model.status).style(
            Style::default()
                .fg(theme_color_in(theme, ThemeRole::Foreground))
                .bg(theme_color_in(theme, ThemeRole::Background)),
        );
        f.render_widget(status, chunks[1]);
    }
    let _ = (cursor_cfg, typography, editor);
    paint_overlay(f, ui_state, app_state, theme, writing_root);
    paint_save_feedback_modal(f, ui_state, theme);
}

/// Centered bordered modal for save outcomes; dims the framebuffer beneath.
fn paint_save_feedback_modal(f: &mut Frame, ui_state: &UiState, theme: &Theme) {
    let Some(body) = ui_state.save_feedback.as_deref() else {
        return;
    };
    let frame = f.area();
    let dim = Paragraph::new("")
        .style(Style::default().bg(theme_color_in(theme, ThemeRole::CursorLine)));
    f.render_widget(dim, frame);

    let modal_w = 70.min(frame.width.max(24));
    let inner_w = (modal_w as usize).saturating_sub(4);
    let lines: Vec<Line> = body
        .split('\n')
        .flat_map(|para| wrap_modal_paragraph(para, inner_w.max(16)))
        .collect();
    let content_lines = lines.len().max(1) as u16;
    let h = content_lines
        .saturating_add(6)
        .min(frame.height.max(8))
        .max(8);
    let area = centered_rect(frame, modal_w, h);
    f.render_widget(Clear, area);
    let mut para_lines = vec![Line::from("")];
    para_lines.extend(lines);
    para_lines.push(Line::from(""));
    para_lines.push(Line::from("  Esc or Enter: dismiss"));
    let widget = Paragraph::new(para_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Save ")
            .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
            .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
    );
    f.render_widget(widget, area);
}

fn paint_start_menu(f: &mut Frame, ui_state: &UiState, theme: &Theme) {
    let full = f.area();
    f.render_widget(
        Block::default().style(Style::default().bg(theme_color_in(
            theme,
            ThemeRole::Background,
        ))),
        full,
    );

    let items = command_items();
    let mut lines: Vec<Line> = Vec::new();
    if !ui_state.start_menu_title_lines.is_empty() {
        for line in &ui_state.start_menu_title_lines {
            lines.push(Line::from(Span::styled(
                line.clone(),
                Style::default().fg(theme_color_in(theme, ThemeRole::Accent)),
            )));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "Start Menu",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for (idx, (name, _)) in items.iter().enumerate() {
        let selected = idx == ui_state.menu_selected;
        let style = resolve_start_menu_item_style(selected, theme);
        let prefix = if selected { "> " } else { "  " };
        lines.push(Line::from(Span::styled(format!("{prefix}{name}"), style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter: select  F1 or Ctrl+H: help",
        Style::default().fg(theme_color_in(theme, ThemeRole::Foreground)),
    )));

    let max_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let w = max_w.max(1).min(full.width);
    let h = (lines.len() as u16).max(1).min(full.height);
    let area = centered_rect(full, w, h);

    let widget = Paragraph::new(lines)
        .style(Style::default().fg(theme_color_in(theme, ThemeRole::Foreground)));
    f.render_widget(widget, area);
}

fn resolve_cell_style(
    is_cursor_here: bool,
    in_selection: bool,
    theme: &Theme,
    cursor_cfg: &CursorConfig,
) -> Style {
    if is_cursor_here {
        match cursor_cfg.style {
            CursorStyleConfig::Block => Style::default()
                .fg(theme_color_in(theme, ThemeRole::Foreground))
                .bg(theme_color_in(theme, ThemeRole::CursorLine)),
            CursorStyleConfig::Line => Style::default()
                .fg(theme_color_in(theme, ThemeRole::Foreground))
                .add_modifier(Modifier::UNDERLINED),
            CursorStyleConfig::Underline => Style::default()
                .fg(theme_color_in(theme, ThemeRole::Foreground))
                .bg(theme_color_in(theme, ThemeRole::Background))
                .add_modifier(Modifier::UNDERLINED),
        }
    } else if in_selection {
        Style::default()
            .fg(theme_color_in(theme, ThemeRole::ListSelectionForeground))
            .bg(theme_color_in(theme, ThemeRole::ListSelectionBackground))
    } else {
        Style::default()
            .fg(theme_color_in(theme, ThemeRole::Foreground))
            .bg(theme_color_in(theme, ThemeRole::Background))
    }
}

fn resolve_start_menu_item_style(is_selected: bool, theme: &Theme) -> Style {
    if is_selected {
        Style::default()
            .fg(theme_color_in(theme, ThemeRole::Accent))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme_color_in(theme, ThemeRole::Foreground))
    }
}

fn command_items() -> &'static [(&'static str, AppCommand)] {
    &[
        ("New Document", AppCommand::NewDocument),
        ("Recent Documents", AppCommand::RecentDocuments),
        ("Writing folder", AppCommand::WritingFolder),
        ("Configuration", AppCommand::Configuration),
        ("Show Help", AppCommand::ShowHelp),
        ("Save", AppCommand::Save),
    ]
}

fn parse_runtime_keymap(app_config: &AppConfig) -> io::Result<RuntimeKeymap> {
    let mut map = RuntimeKeymap::default();
    for (name, raw_binding) in &app_config.keymap.bindings {
        let Some(action) = remap_action_from_name(name) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown keymap action `{name}`"),
            ));
        };
        let binding = parse_key_binding(raw_binding)?;
        if map
            .bindings
            .iter()
            .any(|(existing, _)| existing.code == binding.code && existing.modifiers == binding.modifiers)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("duplicate key binding `{raw_binding}` in [keymap]"),
            ));
        }
        map.bindings.push((binding, action));
    }
    Ok(map)
}

fn remap_action_from_name(name: &str) -> Option<RemapAction> {
    match name {
        "quit" => Some(RemapAction::Quit),
        "open_palette" => Some(RemapAction::OpenPalette),
        "open_menu" => Some(RemapAction::OpenMenu),
        "open_help" => Some(RemapAction::OpenHelp),
        "toggle_chrome_hidden" => Some(RemapAction::ToggleChromeHidden),
        "toggle_status_details" => Some(RemapAction::ToggleStatusDetails),
        "new_document" => Some(RemapAction::Command(AppCommand::NewDocument)),
        "recent_documents" => Some(RemapAction::Command(AppCommand::RecentDocuments)),
        "writing_folder" => Some(RemapAction::Command(AppCommand::WritingFolder)),
        "document_browser" => Some(RemapAction::Command(AppCommand::WritingFolder)),
        "configuration" => Some(RemapAction::Command(AppCommand::Configuration)),
        "save" => Some(RemapAction::Command(AppCommand::Save)),
        _ => None,
    }
}

fn parse_key_binding(raw: &str) -> io::Result<KeyBinding> {
    let mut modifiers = KeyModifiers::empty();
    let mut code: Option<KeyCode> = None;
    for part in raw.split('+') {
        let token = part.trim().to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        match token.as_str() {
            "ctrl" | "control" => modifiers |= KeyModifiers::CONTROL,
            "shift" => modifiers |= KeyModifiers::SHIFT,
            "alt" => modifiers |= KeyModifiers::ALT,
            "esc" | "escape" => code = Some(KeyCode::Esc),
            "enter" | "return" => code = Some(KeyCode::Enter),
            "backspace" => code = Some(KeyCode::Backspace),
            "space" => code = Some(KeyCode::Char(' ')),
            "tab" => code = Some(KeyCode::Tab),
            "backtab" => code = Some(KeyCode::BackTab),
            "up" => code = Some(KeyCode::Up),
            "down" => code = Some(KeyCode::Down),
            "left" => code = Some(KeyCode::Left),
            "right" => code = Some(KeyCode::Right),
            "home" => code = Some(KeyCode::Home),
            "end" => code = Some(KeyCode::End),
            "pageup" | "pgup" => code = Some(KeyCode::PageUp),
            "pagedown" | "pgdown" => code = Some(KeyCode::PageDown),
            "f1" => code = Some(KeyCode::F(1)),
            "f2" => code = Some(KeyCode::F(2)),
            _ if token.len() == 1 => code = Some(KeyCode::Char(token.chars().next().unwrap_or(' '))),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid key token `{token}` in key binding `{raw}`"),
                ));
            }
        }
    }
    match code {
        Some(code) => Ok(KeyBinding { code, modifiers }),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("missing key code in binding `{raw}`"),
        )),
    }
}

fn lookup_remap_action(runtime_keymap: &RuntimeKeymap, code: KeyCode, modifiers: KeyModifiers) -> Option<RemapAction> {
    runtime_keymap
        .bindings
        .iter()
        .find(|(binding, _)| binding.code == code && binding.modifiers == modifiers)
        .map(|(_, action)| *action)
}

fn filtered_palette_items(query: &str) -> Vec<(&'static str, AppCommand)> {
    let q = query.to_ascii_lowercase();
    command_items()
        .iter()
        .copied()
        .filter(|(name, _)| q.is_empty() || name.to_ascii_lowercase().contains(&q))
        .collect()
}

/// F1, Ctrl+H, or Ctrl+Backspace (common terminals map Ctrl+H to the backspace key).
fn key_opens_help(code: KeyCode, modifiers: KeyModifiers) -> bool {
    code == KeyCode::F(1)
        || (modifiers.contains(KeyModifiers::CONTROL)
            && (code == KeyCode::Char('h') || code == KeyCode::Backspace))
}

fn handle_overlay_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    ui_state: &mut UiState,
    app_state: &AppState,
) -> Option<OverlayAction> {
    match ui_state.overlay {
        OverlayMode::None => return None,
        OverlayMode::Help => {
            if code == KeyCode::Esc || key_opens_help(code, modifiers) {
                ui_state.overlay = OverlayMode::Menu;
            }
        }
        OverlayMode::Menu => match code {
            KeyCode::Up => ui_state.menu_selected = ui_state.menu_selected.saturating_sub(1),
            KeyCode::Down => {
                let last = command_items().len().saturating_sub(1);
                ui_state.menu_selected = (ui_state.menu_selected + 1).min(last);
            }
            KeyCode::Enter => {
                let idx = ui_state
                    .menu_selected
                    .min(command_items().len().saturating_sub(1));
                return Some(OverlayAction::Execute(command_items()[idx].1));
            }
            _ => {}
        },
        OverlayMode::RecentDocuments => match code {
            KeyCode::Esc => ui_state.overlay = OverlayMode::Menu,
            KeyCode::Up => ui_state.list_selected = ui_state.list_selected.saturating_sub(1),
            KeyCode::Down => {
                let last = app_state.recent_documents.len().saturating_sub(1);
                ui_state.list_selected = (ui_state.list_selected + 1).min(last);
            }
            KeyCode::Enter => return Some(OverlayAction::OpenSelectedRecent),
            _ if key_opens_help(code, modifiers) => ui_state.overlay = OverlayMode::Help,
            _ => {}
        },
        OverlayMode::WritingFolder => match code {
            KeyCode::Esc => ui_state.overlay = OverlayMode::Menu,
            KeyCode::Up => ui_state.list_selected = ui_state.list_selected.saturating_sub(1),
            KeyCode::Down => {
                let last = app_state.writing_folder_entries.len().saturating_sub(1);
                ui_state.list_selected = (ui_state.list_selected + 1).min(last);
            }
            KeyCode::Enter => return Some(OverlayAction::OpenSelectedWriting),
            _ if key_opens_help(code, modifiers) => ui_state.overlay = OverlayMode::Help,
            _ => {}
        },
        OverlayMode::NewDocumentFilename => match code {
            KeyCode::Esc => {
                ui_state.overlay = OverlayMode::Menu;
            }
            KeyCode::Enter => return Some(OverlayAction::ConfirmNewDocumentBasename),
            _ if key_opens_help(code, modifiers) => ui_state.overlay = OverlayMode::Help,
            KeyCode::Backspace => {
                ui_state.new_document_filename_input.pop();
            }
            KeyCode::Char(c) => {
                if ui_state.new_document_filename_input.chars().count() < 256 {
                    ui_state.new_document_filename_input.push(c);
                }
            }
            _ => {}
        },
        OverlayMode::Configuration => match code {
            KeyCode::Esc => ui_state.overlay = OverlayMode::Menu,
            KeyCode::Enter | KeyCode::Char('r') | KeyCode::Char('R') => {
                return Some(OverlayAction::RetryConfiguration);
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                return Some(OverlayAction::CreateConfiguration);
            }
            _ if key_opens_help(code, modifiers) => ui_state.overlay = OverlayMode::Help,
            _ => {}
        },
        OverlayMode::CommandPalette => match code {
            KeyCode::Esc => ui_state.overlay = OverlayMode::Menu,
            KeyCode::Up => ui_state.palette_selected = ui_state.palette_selected.saturating_sub(1),
            KeyCode::Down => {
                let len = filtered_palette_items(&ui_state.palette_query).len();
                if len > 0 {
                    ui_state.palette_selected = (ui_state.palette_selected + 1).min(len - 1);
                }
            }
            KeyCode::Enter => {
                let items = filtered_palette_items(&ui_state.palette_query);
                if let Some((_, cmd)) = items.get(ui_state.palette_selected).copied() {
                    return Some(OverlayAction::Execute(cmd));
                }
            }
            KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => {
                ui_state.overlay = OverlayMode::None;
            }
            _ if key_opens_help(code, modifiers) => ui_state.overlay = OverlayMode::Help,
            KeyCode::Backspace => {
                ui_state.palette_query.pop();
                ui_state.palette_selected = 0;
            }
            KeyCode::Char(c) => {
                ui_state.palette_query.push(c);
                ui_state.palette_selected = 0;
            }
            _ => {}
        },
    }
    None
}

fn execute_overlay_action(
    action: OverlayAction,
    doc: &mut Document,
    state: &mut EditorState,
    cache: &mut LayoutCache,
    cfg: &LayoutConfig,
    ui_state: &mut UiState,
    app_state: &mut AppState,
    writing_root: &Path,
) -> bool {
    match action {
        OverlayAction::Execute(cmd) => {
            execute_command(cmd, doc, state, cache, cfg, ui_state, app_state, writing_root);
        }
        OverlayAction::OpenSelectedRecent => {
            if let Some(path) = app_state
                .recent_documents
                .get(ui_state.list_selected)
                .cloned()
            {
                open_document_path(path.as_path(), doc, state, cache, cfg, app_state);
            } else {
                app_state.status_message =
                    Some("No recent document selected. Use Up/Down then Enter.".to_string());
            }
            ui_state.overlay = OverlayMode::None;
        }
        OverlayAction::OpenSelectedWriting => {
            if let Some(path) = app_state
                .writing_folder_entries
                .get(ui_state.list_selected)
                .cloned()
            {
                open_document_path(path.as_path(), doc, state, cache, cfg, app_state);
            } else {
                app_state.status_message =
                    Some("No document selected. Use Up/Down then Enter.".to_string());
            }
            ui_state.overlay = OverlayMode::None;
        }
        OverlayAction::RetryConfiguration => {
            execute_command(
                AppCommand::Configuration,
                doc,
                state,
                cache,
                cfg,
                ui_state,
                app_state,
                writing_root,
            );
        }
        OverlayAction::CreateConfiguration => {
            match create_default_configuration(default_configuration_path().as_path()) {
                Ok(()) => execute_command(
                    AppCommand::Configuration,
                    doc,
                    state,
                    cache,
                    cfg,
                    ui_state,
                    app_state,
                    writing_root,
                ),
                Err(err) => {
                    app_state.status_message = Some(format!(
                        "Could not create configuration file: {}. Run scripts/install-config.sh from the tyrannus repo (install step), or fix permissions.",
                        err
                    ));
                    ui_state.overlay = OverlayMode::Configuration;
                }
            }
        }
        OverlayAction::ConfirmNewDocumentBasename => {
            match compose_new_document_path(writing_root, ui_state.new_document_filename_input.as_str())
            {
                Err(msg) => {
                    app_state.status_message = Some(msg);
                }
                Ok(path) => {
                    *doc = Document::new();
                    *state = EditorState::default();
                    state.cursor.normalize(doc);
                    cache.sync(doc, 1, cfg);
                    push_recent_document(app_state, path.clone());
                    app_state.current_document_path = Some(path.clone());
                    ui_state.new_document_filename_input.clear();
                    ui_state.overlay = OverlayMode::None;
                    app_state.status_message =
                        Some(format!("New document: {} (Ctrl+S to save.)", path.display()));
                }
            }
        }
    }
    false
}

fn execute_command(
    cmd: AppCommand,
    doc: &mut Document,
    state: &mut EditorState,
    cache: &mut LayoutCache,
    cfg: &LayoutConfig,
    ui_state: &mut UiState,
    app_state: &mut AppState,
    writing_root: &Path,
) {
    if matches!(cmd, AppCommand::Save) {
        save_current_document(doc, ui_state, app_state);
        return;
    }

    if matches!(cmd, AppCommand::NewDocument) {
        match require_writing_folder(writing_root) {
            Err(msg) => {
                app_state.status_message = Some(msg.clone());
                ui_state.save_feedback = Some(msg);
            }
            Ok(()) => {
                ui_state.new_document_filename_input.clear();
                ui_state.overlay = OverlayMode::NewDocumentFilename;
                app_state.status_message = Some(
                    "Enter basename. Extension optional (.md if omitted); .md/.txt/.toml only.".to_string(),
                );
            }
        }
        return;
    }

    ui_state.overlay = OverlayMode::None;
    ui_state.save_feedback = None;

    match cmd {
        AppCommand::NewDocument => {
            unreachable!("NewDocument handled before overlay reset")
        }
        AppCommand::ShowHelp => {
            ui_state.overlay = OverlayMode::Help;
            app_state.status_message = Some("Opened help.".to_string());
        }
        AppCommand::RecentDocuments => {
            ui_state.list_selected = 0;
            ui_state.overlay = OverlayMode::RecentDocuments;
            if app_state.recent_documents.is_empty() {
                app_state.status_message = Some(
                    "No recent documents yet. Open one from Writing folder first.".to_string(),
                );
            } else {
                app_state.status_message =
                    Some("Select a recent document and press Enter.".to_string());
            }
        }
        AppCommand::WritingFolder => {
            ui_state.list_selected = 0;
            ui_state.overlay = OverlayMode::WritingFolder;
            match require_writing_folder(writing_root) {
                Err(msg) => {
                    app_state.writing_folder_overlay_error = Some(msg.clone());
                    app_state.writing_folder_entries = vec![];
                    app_state.status_message = Some(msg);
                }
                Ok(()) => {
                    app_state.writing_folder_overlay_error = None;
                    app_state.writing_folder_entries = list_writing_folder_entries(writing_root);
                    if app_state.writing_folder_entries.is_empty() {
                        app_state.status_message = Some(format!(
                            "No .md/.txt/.toml files in {}. Add files or set [paths].writing_folder in config.",
                            writing_root.display()
                        ));
                    } else {
                        app_state.status_message =
                            Some("Select a document and press Enter to open.".to_string());
                    }
                }
            }
        }
        AppCommand::Configuration => {
            let cfg_path = default_configuration_path();
            if cfg_path.exists() {
                open_document_path(
                    cfg_path.as_path(),
                    doc,
                    state,
                    cache,
                    cfg,
                    app_state,
                );
            } else {
                ui_state.overlay = OverlayMode::Configuration;
                app_state.status_message = Some(format!(
                    "Configuration file not found at {}. Run scripts/install-config.sh (from the repo) or press C to create here. R: retry, Esc: close.",
                    cfg_path.display()
                ));
            }
        }
        AppCommand::Save => unreachable!("Save is handled before overlay reset"),
    }
}

fn paint_overlay(
    f: &mut Frame,
    ui_state: &UiState,
    app_state: &AppState,
    theme: &Theme,
    writing_root: &Path,
) {
    if ui_state.overlay == OverlayMode::None {
        return;
    }

    if ui_state.overlay != OverlayMode::Menu {
        let dim = Paragraph::new("")
            .style(Style::default().bg(theme_color_in(theme, ThemeRole::CursorLine)));
        f.render_widget(dim, f.area());
    }

    match ui_state.overlay {
        OverlayMode::None => {}
        OverlayMode::Help => {
            let area = centered_rect(f.area(), 76, 20);
            f.render_widget(Clear, area);
            let lines = vec![
                Line::from(" Help "),
                Line::from(""),
                Line::from(Span::styled(
                    "Navigation",
                    Style::default()
                        .fg(theme_color_in(theme, ThemeRole::Accent))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from("  Arrows / Home / End: move cursor"),
                Line::from("  Shift+Arrows/Home/End: extend selection"),
                Line::from("  PgUp/PgDn or wheel: scroll"),
                Line::from(""),
                Line::from(Span::styled(
                    "Actions",
                    Style::default()
                        .fg(theme_color_in(theme, ThemeRole::Accent))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from("  Ctrl+S: save   Ctrl+Q: quit"),
                Line::from(""),
                Line::from(Span::styled(
                    "UI",
                    Style::default()
                        .fg(theme_color_in(theme, ThemeRole::Accent))
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from("  Ctrl+P: command palette   Ctrl+M: menu"),
                Line::from("  Ctrl+K: hide/show frame & status (focus mode)"),
                Line::from("  F1 or Ctrl+H: help        F2: status detail toggle"),
                Line::from("Esc: return to main menu"),
            ];
            let widget = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
                    .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
            );
            f.render_widget(widget, area);
        }
        OverlayMode::Menu => {
            paint_start_menu(f, ui_state, theme);
        }
        OverlayMode::RecentDocuments => {
            let area = centered_rect(f.area(), 70, 16);
            f.render_widget(Clear, area);
            let mut lines: Vec<Line> = vec![Line::from(" Recent Documents "), Line::from("")];
            if app_state.recent_documents.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No recent documents yet",
                    Style::default().fg(theme_color_in(theme, ThemeRole::LinkMissing)),
                )));
                lines.push(Line::from("  Open a document from Writing folder first."));
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "  Tip: Press Esc, then Ctrl+M to open the main menu.",
                ));
            } else {
                for (idx, path) in app_state.recent_documents.iter().take(10).enumerate() {
                    let selected = idx == ui_state.list_selected;
                    let style = if selected {
                        Style::default()
                            .fg(theme_color_in(theme, ThemeRole::ListSelectionForeground))
                            .bg(theme_color_in(theme, ThemeRole::ListSelectionBackground))
                    } else {
                        Style::default().fg(theme_color_in(theme, ThemeRole::Foreground))
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{} {}", if selected { ">" } else { " " }, path.display()),
                        style,
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from("  Enter: open   Esc: main menu   F1 or Ctrl+H: help"));
            }
            let widget = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Recent Documents ")
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
                    .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
            );
            f.render_widget(widget, area);
        }
        OverlayMode::WritingFolder => {
            let area = centered_rect(f.area(), 76, 16);
            f.render_widget(Clear, area);
            let mut lines: Vec<Line> =
                vec![Line::from(" Writing folder "), Line::from("")];
            lines.push(Line::from(format!("  {}", writing_root.display())));
            lines.push(Line::from(""));
            if let Some(err) = app_state.writing_folder_overlay_error.as_deref() {
                for wl in wrap_modal_paragraph(err, 66) {
                    lines.push(Line::from(Span::styled(
                        format!("  {wl}"),
                        Style::default().fg(theme_color_in(theme, ThemeRole::LinkMissing)),
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "  Create the folder or set [paths].writing_folder — Esc: main menu",
                ));
            } else if app_state.writing_folder_entries.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No matching files here (.md/.txt/.toml).",
                    Style::default().fg(theme_color_in(theme, ThemeRole::LinkMissing)),
                )));
                lines.push(Line::from(
                    "  Add files or change [paths].writing_folder in your config.",
                ));
                lines.push(Line::from(""));
                lines.push(Line::from("  Esc: main menu   Ctrl+M: main menu"));
            } else {
                for (idx, path) in app_state.writing_folder_entries.iter().take(10).enumerate() {
                    let selected = idx == ui_state.list_selected;
                    let style = if selected {
                        Style::default()
                            .fg(theme_color_in(theme, ThemeRole::ListSelectionForeground))
                            .bg(theme_color_in(theme, ThemeRole::ListSelectionBackground))
                    } else {
                        Style::default().fg(theme_color_in(theme, ThemeRole::Foreground))
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{} {}", if selected { ">" } else { " " }, path.display()),
                        style,
                    )));
                }
                lines.push(Line::from(""));
                lines.push(Line::from("  Enter: open   Esc: main menu   F1 or Ctrl+H: help"));
            }
            let widget = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Writing folder ")
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
                    .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
            );
            f.render_widget(widget, area);
        }
        OverlayMode::Configuration => {
            let area = centered_rect(f.area(), 76, 11);
            f.render_widget(Clear, area);
            let widget = Paragraph::new(vec![
                Line::from(" Configuration "),
                Line::from(""),
                Line::from("Could not open configuration file."),
                Line::from("Prefer: scripts/install-config.sh from the repo."),
                Line::from("R/Enter: retry open"),
                Line::from("C: create default config here and open"),
                Line::from("Esc: return to main menu"),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Configuration ")
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
                    .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
            );
            f.render_widget(widget, area);
        }
        OverlayMode::CommandPalette => {
            let items = filtered_palette_items(&ui_state.palette_query);
            let height = (items.len().min(6) as u16) + 5;
            let area = centered_rect(f.area(), 60, height.max(8));
            f.render_widget(Clear, area);

            let mut lines: Vec<Line> = vec![Line::from(Span::raw(format!(
                "> {}",
                ui_state.palette_query
            )))];
            lines.push(Line::from(""));
            for (idx, (name, _)) in items.iter().take(6).enumerate() {
                let selected = idx == ui_state.palette_selected;
                let style = if selected {
                    Style::default()
                        .fg(theme_color_in(theme, ThemeRole::ListSelectionForeground))
                        .bg(theme_color_in(theme, ThemeRole::ListSelectionBackground))
                } else {
                    Style::default().fg(theme_color_in(theme, ThemeRole::Foreground))
                };
                lines.push(Line::from(Span::styled(
                    format!("{} {name}", if selected { ">" } else { " " }),
                    style,
                )));
            }
            if items.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No matches",
                    Style::default().fg(theme_color_in(theme, ThemeRole::LinkMissing)),
                )));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(
                "  Enter: run command   Esc: main menu   F1 or Ctrl+H: help",
            ));

            let widget = Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Command Palette ")
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
                    .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
            );
            f.render_widget(widget, area);
        }
        OverlayMode::NewDocumentFilename => {
            let area = centered_rect(f.area(), 70, 13);
            f.render_widget(Clear, area);
            let widget = Paragraph::new(vec![
                Line::from(" Enter a filename for your new document "),
                Line::from(""),
                Line::from(format!("  Folder: {}", writing_root.display())),
                Line::from(""),
                Line::from(format!(
                    "> {}",
                    ui_state.new_document_filename_input
                )),
                Line::from(""),
                Line::from(
                    "  Basename only. Extension optional (.md if omitted); .md, .txt, or .toml.",
                ),
                Line::from("  Enter: create   Esc: main menu"),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" New document ")
                    .border_style(Style::default().fg(theme_color_in(theme, ThemeRole::Accent)))
                    .style(Style::default().bg(theme_color_in(theme, ThemeRole::Background))),
            );
            f.render_widget(widget, area);
        }
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

fn overlay_mode_name(mode: OverlayMode) -> &'static str {
    match mode {
        OverlayMode::None => "editor",
        OverlayMode::CommandPalette => "palette",
        OverlayMode::Menu => "menu",
        OverlayMode::RecentDocuments => "recent-docs",
        OverlayMode::WritingFolder => "writing-folder",
        OverlayMode::NewDocumentFilename => "new-document-filename",
        OverlayMode::Configuration => "config",
        OverlayMode::Help => "help",
    }
}

/// Wrap a single paragraph into [`Line`]s for modal display (`max_width`: content columns inside borders).
fn wrap_modal_paragraph(para: &str, max_width: usize) -> Vec<Line<'static>> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let max_width = max_width.max(1);
    let s = para.trim_end();
    if s.is_empty() {
        return vec![Line::from("")];
    }
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut rest = s;

    while !rest.is_empty() {
        if rest.width() <= max_width {
            out.push(Line::from(rest.to_string()));
            break;
        }

        let graphemes: Vec<(usize, &str)> = rest.grapheme_indices(true).collect();
        let mut acc = 0usize;
        let mut fit_count = 0usize;
        let mut last_break_after = 0usize;

        for (idx, (_, g)) in graphemes.iter().enumerate() {
            let w = g.width();
            if acc + w > max_width {
                break;
            }
            acc += w;
            fit_count = idx + 1;
            if *g == " " {
                last_break_after = fit_count;
            }
        }

        let take = if last_break_after > 0 {
            last_break_after
        } else if fit_count > 0 {
            fit_count
        } else {
            1
        };

        let end_byte = if take >= graphemes.len() {
            rest.len()
        } else {
            graphemes[take].0
        };

        let (raw_line, next) = rest.split_at(end_byte);
        let line = raw_line.trim_end();
        if line.is_empty() {
            let g = graphemes
                .first()
                .expect("non-empty rest implies non-empty graphemes")
                .1;
            out.push(Line::from(g.to_string()));
            rest = &rest[g.len().min(rest.len())..];
            continue;
        }
        out.push(Line::from(line.to_string()));
        rest = next.trim_start();
    }
    out
}

fn clear_status(app_state: &mut AppState) {
    app_state.status_message = None;
}

fn apply_toggle_chrome_hidden(ui_state: &mut UiState, app_state: &mut AppState) {
    ui_state.chrome_hidden = !ui_state.chrome_hidden;
    if !ui_state.chrome_hidden {
        app_state.status_message =
            Some("Chrome shown (Ctrl+K to hide)".to_string());
    }
}

fn default_configuration_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/tyrannus/config.toml")
}

fn default_recovery_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/tyrannus/recovery.txt")
}

const DEFAULT_CONFIG_TOML: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/contrib/default-config.toml"));

fn create_default_configuration(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        return Ok(());
    }
    fs::write(path, DEFAULT_CONFIG_TOML)
}

fn document_to_plain_text(doc: &Document) -> String {
    let mut out = String::new();
    for (i, block) in doc.blocks.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match block {
            DocBlock::Paragraph(il) | DocBlock::Heading { content: il, .. } => {
                for inline in il {
                    if let Inline::Text(s) = inline {
                        out.push_str(s);
                    }
                }
            }
            DocBlock::CodeBlock { text, .. } => out.push_str(text),
        }
    }
    out
}

fn save_current_document(doc: &Document, ui_state: &mut UiState, app_state: &mut AppState) {
    let Some(path) = app_state.current_document_path.as_ref() else {
        let msg =
            "No file path. Open a document from Writing folder in the main menu (Ctrl+M) first."
                .to_string();
        app_state.status_message = Some(msg.clone());
        ui_state.save_feedback = Some(msg);
        return;
    };
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if !parent.exists() {
            let msg = format!(
                "Cannot save: directory does not exist: {} (create it first).",
                parent.display()
            );
            app_state.status_message = Some(msg.clone());
            ui_state.save_feedback = Some(msg);
            return;
        }
        if !parent.is_dir() {
            let msg = format!("Cannot save: {} is not a directory.", parent.display());
            app_state.status_message = Some(msg.clone());
            ui_state.save_feedback = Some(msg);
            return;
        }
    }
    let text = document_to_plain_text(doc);
    let msg = match fs::write(path, text.as_str()) {
        Ok(()) => format!("Saved {}", path.display()),
        Err(err) => format!("Could not save {}: {err}", path.display()),
    };
    app_state.status_message = Some(msg.clone());
    ui_state.save_feedback = Some(msg);
}

fn record_edit_and_snapshot(doc: &Document, app_state: &mut AppState) {
    app_state.edit_counter = app_state.edit_counter.saturating_add(1);
    if app_state.edit_counter % SNAPSHOT_EVERY_EDITS != 0 {
        return;
    }
    if let Some(parent) = app_state.recovery_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let text = document_to_plain_text(doc);
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&app_state.recovery_path)
    {
        let _ = f.write_all(text.as_bytes());
    }
}

fn maybe_restore_recovery_snapshot(
    doc: &mut Document,
    state: &mut EditorState,
    cache: &mut LayoutCache,
    cfg: &LayoutConfig,
    app_state: &mut AppState,
) {
    let Ok(text) = fs::read_to_string(&app_state.recovery_path) else {
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    *doc = plain_text_to_document(&text);
    *state = EditorState::default();
    state.cursor.normalize(doc);
    cache.sync(doc, 1, cfg);
    app_state.status_message = Some("Recovered unsaved snapshot".to_string());
}

fn load_start_menu_title_lines(config_path: &Path, app_config: &AppConfig) -> Vec<String> {
    let selected_name = load_start_menu_title_name(config_path, app_config);
    let from_config = if is_valid_title_name(&selected_name) {
        title_text_path_for(&selected_name)
    } else {
        title_text_path_for("ANSI-Shadow")
    };
    let fallback = title_text_path_for("ANSI-Shadow");
    let selected_path = if from_config.exists() {
        from_config
    } else {
        fallback
    };
    match fs::read_to_string(selected_path) {
        Ok(raw) => raw.lines().map(str::to_string).collect(),
        Err(_) => Vec::new(),
    }
}

fn load_start_menu_title_name(config_path: &Path, app_config: &AppConfig) -> String {
    if !app_config.ui.start_menu_title.trim().is_empty() {
        return app_config.ui.start_menu_title.trim().to_string();
    }
    let Ok(raw) = fs::read_to_string(config_path) else {
        return "ANSI-Shadow".to_string();
    };
    let Ok(parsed) = toml::from_str::<Value>(&raw) else {
        return "ANSI-Shadow".to_string();
    };
    let Some(table) = parsed.as_table() else {
        return "ANSI-Shadow".to_string();
    };
    if let Some(value) = table.get("start_menu_title").and_then(Value::as_str) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(value) = table
        .get("ui")
        .and_then(Value::as_table)
        .and_then(|ui| ui.get("start_menu_title"))
        .and_then(Value::as_str)
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "ANSI-Shadow".to_string()
}

fn title_text_path_for(name: &str) -> PathBuf {
    Path::new("titletext").join(name)
}

fn is_valid_title_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && !name.contains('\\') && name != "." && name != ".."
}

fn require_writing_folder(root: &Path) -> Result<(), String> {
    match fs::metadata(root) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(format!(
            "Writing folder is not a directory: {}",
            root.display()
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(format!(
            "Writing folder does not exist: {}. Create it or set [paths].writing_folder in config.",
            root.display()
        )),
        Err(e) => Err(format!(
            "Cannot access Writing folder {}: {e}",
            root.display()
        )),
    }
}

/// Basename plus extension rules for a new document under the Writing folder: `.md` when omitted; only `.md` / `.txt` / `.toml` allowed otherwise.
fn finalize_new_document_basename(raw_input: &str) -> Result<String, String> {
    const MAX_CHARS: usize = 256;

    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err("Filename cannot be empty.".to_string());
    }
    if trimmed == "." || trimmed == ".." {
        return Err("Invalid filename.".to_string());
    }
    if trimmed.chars().count() > MAX_CHARS {
        return Err(format!(
            "Filename is too long (max {MAX_CHARS} characters)."
        ));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err("Use a basename only, not a path.".to_string());
    }
    if trimmed.bytes().any(|b| b == 0) {
        return Err("Invalid filename.".to_string());
    }

    let path = Path::new(trimmed);
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return Err("Invalid filename.".to_string());
    };
    if file_name != trimmed {
        return Err("Invalid filename.".to_string());
    }

    match path.extension().and_then(|e| e.to_str()) {
        None => Ok(format!("{trimmed}.md")),
        Some(ext) => {
            if ext.is_empty() {
                return Err("Invalid extension.".to_string());
            }
            let el = ext.to_ascii_lowercase();
            if matches!(el.as_str(), "md" | "txt" | "toml") {
                Ok(trimmed.to_string())
            } else {
                Err(format!(
                    "Extension .{ext} not allowed — use .md, .txt, or .toml."
                ))
            }
        }
    }
}

fn compose_new_document_path(writing_root: &Path, raw_input: &str) -> Result<PathBuf, String> {
    let base = finalize_new_document_basename(raw_input)?;
    let full = writing_root.join(base);
    if full.exists() {
        return Err(format!(
            "{} already exists. Choose another basename.",
            full.display()
        ));
    }
    Ok(full)
}

fn list_writing_folder_entries(root: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = Vec::new();
    let Ok(read_dir) = fs::read_dir(root) else {
        return entries;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_ascii_lowercase());
            if matches!(ext.as_deref(), Some("md" | "txt" | "toml")) {
                entries.push(path);
            }
        }
    }
    entries.sort();
    entries.dedup();
    entries
}

fn open_document_path(
    path: &Path,
    doc: &mut Document,
    state: &mut EditorState,
    cache: &mut LayoutCache,
    cfg: &LayoutConfig,
    app_state: &mut AppState,
) {
    match fs::read_to_string(path) {
        Ok(text) => {
            *doc = plain_text_to_document(&text);
            *state = EditorState::default();
            state.cursor.normalize(doc);
            cache.sync(doc, 1, cfg);
            app_state.current_document_path = Some(path.to_path_buf());
            push_recent_document(app_state, path.to_path_buf());
            app_state.status_message = Some(format!("Opened {}", path.display()));
        }
        Err(err) => {
            app_state.status_message = Some(format!(
                "Could not open {}: {}. Verify the path/permissions, then retry from Writing folder (Ctrl+M).",
                path.display(),
                err
            ));
        }
    }
}

fn push_recent_document(app_state: &mut AppState, path: PathBuf) {
    app_state.recent_documents.retain(|p| p != &path);
    app_state.recent_documents.insert(0, path);
    const MAX_RECENT_DOCS: usize = 12;
    if app_state.recent_documents.len() > MAX_RECENT_DOCS {
        app_state.recent_documents.truncate(MAX_RECENT_DOCS);
    }
}

fn plain_text_to_document(text: &str) -> Document {
    let mut blocks = Vec::new();
    for line in text.split('\n') {
        blocks.push(DocBlock::Paragraph(vec![Inline::text_str(line)]));
    }
    if blocks.is_empty() {
        Document::new()
    } else {
        Document::with_blocks(blocks)
    }
}

const H_PAD: usize = tyrannus::layout::H_PAD;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_style_wins_over_selection() {
        let theme = Theme::default();
        let both = resolve_cell_style(true, true, &theme, &CursorConfig::default());
        assert_eq!(both.fg, Some(theme_color_in(&theme, ThemeRole::Foreground)));
        assert_eq!(both.bg, None);
        assert!(both.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn start_menu_selected_item_is_not_background_highlighted() {
        let theme = Theme::default();
        let selected = resolve_start_menu_item_style(true, &theme);
        assert_eq!(selected.bg, None);
        assert_eq!(selected.fg, Some(theme_color_in(&theme, ThemeRole::Accent)));
        assert!(selected.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn command_palette_filter_matches_case_insensitively() {
        let filtered = filtered_palette_items("recent");
        assert!(filtered.iter().any(|(name, _)| *name == "Recent Documents"));
    }

    #[test]
    fn save_fails_when_parent_directory_missing() {
        let base = std::env::temp_dir().join(format!(
            "tyrannus_save_missing_parent_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let missing_parent = base.join("no_such_subdirectory");
        let mut app_state = AppState {
            current_document_path: Some(missing_parent.join("file.md")),
            ..Default::default()
        };

        let mut ui = UiState {
            overlay: OverlayMode::None,
            ..Default::default()
        };
        save_current_document(&Document::new(), &mut ui, &mut app_state);

        assert!(
            ui.save_feedback.is_some(),
            "save error should open save feedback modal"
        );
        let msg = ui.save_feedback.expect("expected error modal");
        assert!(
            msg.contains("directory does not exist"),
            "unexpected message: {msg}"
        );
        assert!(!missing_parent.join("file.md").exists());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn require_writing_folder_accepts_existing_dir() {
        let base = std::env::temp_dir().join(format!("tyrannus_wf_ok_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("mkdir");
        assert!(require_writing_folder(base.as_path()).is_ok());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn require_writing_folder_errors_when_missing() {
        let base = std::env::temp_dir().join(format!(
            "tyrannus_wf_missing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        let p = base.join("does_not_exist_here");
        let err = require_writing_folder(p.as_path()).expect_err("expected err");
        assert!(
            err.contains("does not exist"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn finalize_new_document_basename_appends_md_when_no_extension() {
        assert_eq!(
            finalize_new_document_basename("story").unwrap(),
            "story.md"
        );
        assert_eq!(
            finalize_new_document_basename("  note  ").unwrap(),
            "note.md"
        );
    }

    #[test]
    fn finalize_new_document_basename_rejects_paths_and_bad_extensions() {
        assert!(finalize_new_document_basename("a/b").is_err());
        assert!(finalize_new_document_basename("..").is_err());
        let pdf = finalize_new_document_basename("doc.pdf").expect_err("pdf rejected");
        assert!(
            pdf.contains("not allowed"),
            "unexpected message: {pdf}"
        );
    }

    #[test]
    fn finalize_new_document_accepts_whitelisted_extensions() {
        assert_eq!(
            finalize_new_document_basename("cfg.TOML").unwrap(),
            "cfg.TOML"
        );
        assert_eq!(finalize_new_document_basename("a.txt").unwrap(), "a.txt");
    }

    #[test]
    fn compose_new_document_path_errors_when_target_exists() {
        let base = std::env::temp_dir().join(format!("tyrannus_compose_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).expect("mkdir");
        fs::write(base.join("exists.md"), "").expect("touch");
        let err = compose_new_document_path(base.as_path(), "exists").expect_err("");
        assert!(err.contains("already exists"), "unexpected: {err}");
        assert_eq!(
            compose_new_document_path(base.as_path(), "fresh").unwrap(),
            base.join("fresh.md")
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn ui_state_defaults_to_start_menu() {
        let ui = UiState::default();
        assert_eq!(ui.overlay, OverlayMode::Menu);
        assert_eq!(ui.menu_selected, 0);
    }

    #[test]
    fn reads_start_menu_title_from_root_config_key() {
        let tmp = std::env::temp_dir().join("tyrannus-start-menu-title-root.toml");
        fs::write(&tmp, "start_menu_title = \"Rebel\"").expect("write temp config");
        assert_eq!(
            load_start_menu_title_name(
                tmp.as_path(),
                &AppConfig {
                    ui: crate::config::UiConfig {
                        start_menu_title: "".to_string(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
            "Rebel"
        );
        let _ = fs::remove_file(tmp);
    }

    #[test]
    fn reads_start_menu_title_from_ui_table_key() {
        let tmp = std::env::temp_dir().join("tyrannus-start-menu-title-ui.toml");
        fs::write(
            &tmp,
            r#"
[ui]
start_menu_title = "3D-ASCII"
"#,
        )
        .expect("write temp config");
        assert_eq!(
            load_start_menu_title_name(
                tmp.as_path(),
                &AppConfig {
                    ui: crate::config::UiConfig {
                        start_menu_title: "".to_string(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ),
            "3D-ASCII"
        );
        let _ = fs::remove_file(tmp);
    }

    #[test]
    fn snapshot_roundtrip_restores_document() {
        let mut doc = Document::new();
        let mut st = EditorState::default();
        assert!(reduce_edit(
            &mut doc,
            &mut st,
            EditOp::InsertChar('h')
        ));
        assert!(reduce_edit(
            &mut doc,
            &mut st,
            EditOp::InsertChar('i')
        ));

        let mut app = AppState {
            recovery_path: std::env::temp_dir().join("tyrannus-recovery-roundtrip.txt"),
            ..AppState::default()
        };
        app.edit_counter = SNAPSHOT_EVERY_EDITS - 1;
        record_edit_and_snapshot(&doc, &mut app);

        let mut recovered_doc = Document::new();
        let mut recovered_st = EditorState::default();
        let mut cache = LayoutCache::default();
        let cfg = LayoutConfig::default();
        maybe_restore_recovery_snapshot(
            &mut recovered_doc,
            &mut recovered_st,
            &mut cache,
            &cfg,
            &mut app,
        );
        let text: String = tyrannus::flatten_document_chars(&recovered_doc)
            .into_iter()
            .map(|(_, ch)| ch)
            .collect();
        assert_eq!(text, "hi");
        let _ = fs::remove_file(app.recovery_path);
    }

    #[test]
    fn parse_key_binding_supports_ctrl_combo() {
        let binding = parse_key_binding("ctrl+q").expect("binding parses");
        assert_eq!(binding.code, KeyCode::Char('q'));
        assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn runtime_keymap_rejects_duplicate_bindings() {
        let mut app = AppConfig::default();
        app.keymap
            .bindings
            .insert("quit".to_string(), "ctrl+q".to_string());
        app.keymap
            .bindings
            .insert("open_menu".to_string(), "ctrl+q".to_string());
        let err = parse_runtime_keymap(&app).expect_err("must reject duplicates");
        assert!(err.to_string().contains("duplicate key binding"));
    }

    #[test]
    fn plain_text_to_document_and_back_roundtrips() {
        let a = "hello\n\nworld\n";
        let doc = plain_text_to_document(a);
        let back = document_to_plain_text(&doc);
        assert_eq!(back, a);
        let empty = document_to_plain_text(&plain_text_to_document(""));
        assert_eq!(empty, "");
    }

    #[test]
    fn plain_text_trailing_newline_makes_empty_final_block() {
        let t = "one\ntwo\n";
        let doc = plain_text_to_document(t);
        // split('\n') yields three segments: "one", "two", "".
        assert_eq!(doc.blocks.len(), 3);
    }

    #[test]
    fn parse_key_binding_parses_shift_alt_and_specials() {
        let b1 = parse_key_binding("shift+up").expect("shift+up");
        assert_eq!(b1.code, KeyCode::Up);
        assert!(b1.modifiers.contains(KeyModifiers::SHIFT));
        let b2 = parse_key_binding("alt+enter").expect("alt+enter");
        assert_eq!(b2.code, KeyCode::Enter);
        assert!(b2.modifiers.contains(KeyModifiers::ALT));
        let b3 = parse_key_binding("ctrl+left").expect("left");
        assert_eq!(b3.code, KeyCode::Left);
        assert!(b3.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn parse_key_binding_rejects_empty_code() {
        let err = parse_key_binding("ctrl+").expect_err("missing key");
        assert!(err.to_string().contains("missing key code")
            || err.to_string().contains("missing"));
    }

    #[test]
    fn parse_key_binding_rejects_invalid_token() {
        let err = parse_key_binding("nope+zz").expect_err("invalid");
        assert!(err.to_string().contains("invalid key token")
            || err.to_string().contains("invalid"));
    }

    #[test]
    fn runtime_keymap_parses_valid_bindings() {
        let mut app = AppConfig::default();
        app.keymap
            .bindings
            .insert("quit".to_string(), "alt+esc".to_string());
        app.keymap
            .bindings
            .insert("open_palette".to_string(), "ctrl+shift+space".to_string());
        let map = parse_runtime_keymap(&app).expect("valid keymap");
        let quit = map
            .bindings
            .iter()
            .find(|(k, _)| k.code == KeyCode::Esc && k.modifiers.contains(KeyModifiers::ALT));
        assert!(quit.is_some());
    }

    #[test]
    fn runtime_keymap_rejects_unknown_action() {
        let mut app = AppConfig::default();
        app.keymap
            .bindings
            .insert("not_a_valid_action_name".to_string(), "f1".to_string());
        let err = parse_runtime_keymap(&app).expect_err("unknown");
        assert!(err.to_string().contains("unknown keymap action"));
    }
}
