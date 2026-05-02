# tyrannus

A terminal word processor in Rust, built on [ratatui](https://ratatui.rs) and
[crossterm](https://docs.rs/crossterm). Block AST + inline tree, viewport-bounded
layout cache, selection-as-overlay rendering. Not a line editor.

**Status:** Phase 7 — hardening toward release. Editing, selection, scrolling,
overlays, resize handling, **save** (Ctrl+S when a file path is active), and
periodic **recovery snapshots** are implemented. The main menu includes
`New Document`, `Recent Documents`, `Writing folder`, and `Configuration`.
The writing folder scans `[paths].writing_folder` from config (default:
`~/Writing`) for `.md`, `.txt`, and `.toml` files.

## Build

```bash
cargo build --release
```

Requires a stable Rust toolchain (edition 2021).

## Install (config layout)

Creating `~/.config/tyrannus` and dropping in the bundled default config (only if `config.toml` is absent) belongs in installation, not gatekept at runtime:

```bash
./scripts/install-config.sh
```

Respects `XDG_CONFIG_HOME` when set. Afterward, Tyrannus finds `config.toml` on startup. You can still create the layout from Configuration → **C** if you skipped the script.

## Run

```bash
cargo run --release
```

The editor opens on an empty paragraph. Press `Ctrl+Q` to quit. If the terminal
rejects mouse capture (some remote sessions / restricted shells), the editor
continues without scroll-wheel support and the title bar is annotated
`(no mouse)`.

## Keymap

Mirrors the in-app help overlay (open with **F1** or **Ctrl+H**; some terminals
send **Ctrl+Backspace** for the same chord as Ctrl+H).

| Key                         | Action                                       |
|-----------------------------|----------------------------------------------|
| Arrows / Home / End         | Move cursor                                  |
| Shift + Arrows / Home / End | Extend selection                             |
| PgUp / PgDn / scroll wheel  | Scroll viewport                              |
| Printable keys              | Insert character                             |
| Backspace                   | Delete character (or selection)              |
| Enter                       | Newline / split paragraph                    |
| Ctrl+P                      | Open command palette                         |
| Ctrl+S                      | Save to the current document path            |
| Ctrl+K                      | Toggle focus mode (hide/show frame & status)  |
| Ctrl+M                      | Open main menu (new/recent/writing/config)    |
| F1 / Ctrl+H                 | Toggle help overlay                          |
| F2                          | Toggle extra detail on the status line       |
| Esc                         | Close any overlay                            |
| Ctrl+Q                      | Quit                                         |

Several bindings can be overridden in `config.toml` under `[keymap]`; see
`contrib/default-config.toml` for commented examples.

## Test

```bash
cargo test
```

## Bench

```bash
cargo bench --bench large_document
```

Runs criterion benchmarks over a synthetic ~10k-paragraph document covering
the hot paths the editor calls each frame:

- `layout_document` cold pass at width 80
- `LayoutCache::sync` after a single keystroke (full relayout under the
  current generation-bumped cache)
- `flatten_document_chars` and `cursor_to_gap_index` (called by every
  selection-extending move)
- The per-frame body-span build loop with the viewport scrolled deep into the document

Add `-- --quick` for a fast smoke run; HTML reports land in
`target/criterion/`.

## Architecture

Top-level layout:

- `src/lib.rs` — public re-exports for the editor library
- `src/document.rs` — block + inline AST
- `src/edit.rs` — `EditOp` reducer, cursor/selection state
- `src/cursor.rs` — cursor model and inline-path traversal
- `src/layout.rs` — block → wrapped visual lines, cursor mapping
- `src/viewport.rs` — viewport range, dirty regions, layout cache
- `src/config.rs` — TOML config, optional key remaps, paths
- `src/theme_presets.rs` — built-in theme presets
- `src/main.rs` — TUI entry point, event loop, overlays, paint pipeline
- `benches/large_document.rs` — criterion smoke benchmark

## Limitations

- **Document loading is plain-text and single-buffer.** **Writing folder**
  (documents under `[paths].writing_folder`, default `~/Writing`),
  `Recent Documents`, and `Configuration` open files by loading line-by-line
  paragraph blocks; markdown structure is not parsed yet.
- **Save requires an on-disk path.** `Ctrl+S` writes plain text to the file
  associated with the buffer (after opening from the menu). A brand-new buffer
  with no opened path cannot be saved until you open or create a file through
  the writing folder flow.
- **Single document, single buffer.** No splits, no tabs.
- **No search / replace, no clipboard, no spellcheck** in this release.
- **`LayoutCache::sync` does a full relayout on every generation bump.**
  The benchmark numbers above show what that costs at 10k paragraphs;
  range-based incremental layout is not implemented yet.

## License

Dual-licensed under MIT or Apache-2.0, at your option (see `Cargo.toml`).
