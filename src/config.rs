use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ratatui::style::Color;
use toml::Value;

use crate::theme_presets;

#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub link: String,
    pub link_missing: String,
    pub cursor_line: String,
    pub code: String,
    pub list_selection_foreground: String,
    pub list_selection_background: String,
    pub heading1: String,
    pub heading2: String,
    pub heading3: String,
    pub heading4: String,
    pub heading5: String,
    pub heading6: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpacingConfig {
    pub line_gap_lines: usize,
    pub paragraph_gap_lines: usize,
    pub code_margin: usize,
}

impl Default for SpacingConfig {
    fn default() -> Self {
        Self {
            line_gap_lines: 0,
            paragraph_gap_lines: 0,
            code_margin: 1,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypographyConfig {
    pub extra_word_spacing: usize,
    pub extra_letter_spacing: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyleConfig {
    Block,
    Line,
    Underline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CursorConfig {
    pub style: CursorStyleConfig,
    pub blink: bool,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            style: CursorStyleConfig::Line,
            blink: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorConfig {
    pub tab_width: usize,
    pub hard_tabs: bool,
    pub auto_wrap: bool,
    pub show_invisibles: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            tab_width: 4,
            hard_tabs: false,
            auto_wrap: true,
            show_invisibles: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiConfig {
    pub status_details_default: bool,
    pub start_menu_title: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            status_details_default: false,
            start_menu_title: "ANSI-Shadow".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct KeymapConfig {
    pub bindings: BTreeMap<String, String>,
}

/// Resolved filesystem paths derived from `[paths]` in config (defaults when omitted).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathsConfig {
    /// Directory scanned for `.md` / `.txt` / `.toml` documents to open from the Writing folder UI.
    pub writing_folder: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            writing_folder: default_writing_folder_path(),
        }
    }
}

pub(crate) fn default_writing_folder_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Writing"))
        .unwrap_or_else(|| PathBuf::from("Writing"))
}

/// Applies optional `paths.writing_folder` string: empty ⇒ default `~/Writing`, supports `~/` tilde expansion.
pub(crate) fn resolve_writing_folder_from_config_value(raw_config: Option<&str>) -> PathBuf {
    let default = default_writing_folder_path();
    let Some(s) = raw_config else {
        return default;
    };
    let t = s.trim();
    if t.is_empty() {
        return default;
    }
    if let Some(rest) = t.strip_prefix("~/") {
        return std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest))
            .unwrap_or_else(|| PathBuf::from(".").join(rest));
    }
    if t == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    PathBuf::from(t)
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AppConfig {
    pub theme: Theme,
    pub spacing: SpacingConfig,
    pub typography: TypographyConfig,
    pub cursor: CursorConfig,
    pub editor: EditorConfig,
    pub ui: UiConfig,
    pub keymap: KeymapConfig,
    pub paths: PathsConfig,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "black".to_string(),
            foreground: "white".to_string(),
            accent: "green".to_string(),
            link: "cyan".to_string(),
            link_missing: "red".to_string(),
            cursor_line: "darkgray".to_string(),
            code: "yellow".to_string(),
            list_selection_foreground: "black".to_string(),
            list_selection_background: "green".to_string(),
            heading1: "".to_string(),
            heading2: "".to_string(),
            heading3: "".to_string(),
            heading4: "".to_string(),
            heading5: "".to_string(),
            heading6: "".to_string(),
        }
    }
}

impl Theme {
    pub fn merge_theme_table(
        &mut self,
        theme_table: &toml::map::Map<String, Value>,
    ) -> Result<(), ThemeError> {
        for (key, value) in theme_table {
            let Some(raw_value) = value.as_str() else {
                return Err(ThemeError::InvalidType {
                    field: format!("theme.{key}"),
                    expected: "string".to_string(),
                });
            };
            match key.as_str() {
                "background" => self.background = raw_value.to_string(),
                "foreground" => self.foreground = raw_value.to_string(),
                "accent" => self.accent = raw_value.to_string(),
                "link" => self.link = raw_value.to_string(),
                "link_missing" => self.link_missing = raw_value.to_string(),
                "cursor_line" => self.cursor_line = raw_value.to_string(),
                "code" => self.code = raw_value.to_string(),
                "list_selection_foreground" => {
                    self.list_selection_foreground = raw_value.to_string()
                }
                "list_selection_background" => {
                    self.list_selection_background = raw_value.to_string()
                }
                "heading1" => self.heading1 = raw_value.to_string(),
                "heading2" => self.heading2 = raw_value.to_string(),
                "heading3" => self.heading3 = raw_value.to_string(),
                "heading4" => self.heading4 = raw_value.to_string(),
                "heading5" => self.heading5 = raw_value.to_string(),
                "heading6" => self.heading6 = raw_value.to_string(),
                _ => return Err(ThemeError::UnknownThemeKey(key.to_string())),
            }
        }
        Ok(())
    }

    pub fn validate_theme_colors(&self) -> Result<(), ThemeError> {
        self.validate_required("background", &self.background)?;
        self.validate_required("foreground", &self.foreground)?;
        self.validate_required("accent", &self.accent)?;
        self.validate_required("link", &self.link)?;
        self.validate_required("link_missing", &self.link_missing)?;
        self.validate_required("cursor_line", &self.cursor_line)?;
        self.validate_required("code", &self.code)?;
        self.validate_required("list_selection_foreground", &self.list_selection_foreground)?;
        self.validate_required("list_selection_background", &self.list_selection_background)?;

        self.validate_heading("heading1", &self.heading1)?;
        self.validate_heading("heading2", &self.heading2)?;
        self.validate_heading("heading3", &self.heading3)?;
        self.validate_heading("heading4", &self.heading4)?;
        self.validate_heading("heading5", &self.heading5)?;
        self.validate_heading("heading6", &self.heading6)?;
        Ok(())
    }

    fn validate_required(&self, field: &str, value: &str) -> Result<(), ThemeError> {
        let norm = normalize_color_name(value);
        if norm.is_empty() {
            return Err(ThemeError::EmptyRequiredField(field.to_string()));
        }
        if parse_ansi_color_name(&norm).is_none() {
            return Err(ThemeError::InvalidColorName {
                field: field.to_string(),
                value: value.to_string(),
            });
        }
        Ok(())
    }

    fn validate_heading(&self, field: &str, value: &str) -> Result<(), ThemeError> {
        let norm = normalize_color_name(value);
        if norm.is_empty() {
            return Ok(());
        }
        if parse_ansi_color_name(&norm).is_none() {
            return Err(ThemeError::InvalidColorName {
                field: field.to_string(),
                value: value.to_string(),
            });
        }
        Ok(())
    }

    pub fn value_for(&self, role: ThemeRole) -> &str {
        match role {
            ThemeRole::Background => &self.background,
            ThemeRole::Foreground => &self.foreground,
            ThemeRole::Accent => &self.accent,
            ThemeRole::Link => &self.link,
            ThemeRole::LinkMissing => &self.link_missing,
            ThemeRole::CursorLine => &self.cursor_line,
            ThemeRole::Code => &self.code,
            ThemeRole::ListSelectionForeground => &self.list_selection_foreground,
            ThemeRole::ListSelectionBackground => &self.list_selection_background,
            ThemeRole::Heading1 => &self.heading1,
            ThemeRole::Heading2 => &self.heading2,
            ThemeRole::Heading3 => &self.heading3,
            ThemeRole::Heading4 => &self.heading4,
            ThemeRole::Heading5 => &self.heading5,
            ThemeRole::Heading6 => &self.heading6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum ThemeRole {
    Background,
    Foreground,
    Accent,
    Link,
    LinkMissing,
    CursorLine,
    Code,
    ListSelectionForeground,
    ListSelectionBackground,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
}

#[derive(Debug)]
pub enum ThemeError {
    Io(io::Error),
    InvalidToml(toml::de::Error),
    InvalidType { field: String, expected: String },
    UnknownThemeKey(String),
    UnknownPreset { value: String, known: Vec<String> },
    EmptyRequiredField(String),
    InvalidColorName { field: String, value: String },
    InvalidRange {
        field: String,
        min: usize,
        max: usize,
        value: usize,
    },
    InvalidEnum {
        field: String,
        value: String,
        expected: Vec<&'static str>,
    },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::Io(err) => write!(f, "{err}"),
            ThemeError::InvalidToml(err) => write!(f, "invalid TOML config: {err}"),
            ThemeError::InvalidType { field, expected } => {
                write!(f, "invalid type for `{field}` (expected {expected})")
            }
            ThemeError::UnknownThemeKey(key) => {
                write!(f, "unknown theme role `{key}` in [theme] table")
            }
            ThemeError::UnknownPreset { value, known } => {
                write!(
                    f,
                    "theme_preset: unknown value \"{value}\" (known: {})",
                    known.join(", ")
                )
            }
            ThemeError::EmptyRequiredField(field) => {
                write!(f, "theme role `{field}` cannot be empty")
            }
            ThemeError::InvalidColorName { field, value } => {
                write!(
                    f,
                    "theme role `{field}` has invalid ANSI color name `{value}`"
                )
            }
            ThemeError::InvalidRange {
                field,
                min,
                max,
                value,
            } => {
                write!(
                    f,
                    "invalid value for `{field}`: {value} (expected {min}..={max})"
                )
            }
            ThemeError::InvalidEnum {
                field,
                value,
                expected,
            } => {
                write!(
                    f,
                    "invalid value for `{field}`: `{value}` (expected one of: {})",
                    expected.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ThemeError {}

#[allow(dead_code)]
pub fn load_theme_from_path(path: &Path) -> Result<Theme, ThemeError> {
    Ok(load_app_config_from_path(path)?.theme)
}

#[allow(dead_code)]
pub fn load_theme_from_str(raw: &str) -> Result<Theme, ThemeError> {
    Ok(load_app_config_from_str(raw)?.theme)
}

pub fn load_app_config_from_path(path: &Path) -> Result<AppConfig, ThemeError> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(path).map_err(ThemeError::Io)?;
    load_app_config_from_str(&raw)
}

pub fn load_app_config_from_str(raw: &str) -> Result<AppConfig, ThemeError> {
    let parsed: Value = toml::from_str(raw).map_err(ThemeError::InvalidToml)?;
    let table = parsed.as_table().ok_or_else(|| ThemeError::InvalidType {
        field: "root".to_string(),
        expected: "table".to_string(),
    })?;

    let mut app = AppConfig::default();

    if let Some(value) = table.get("theme_preset") {
        let Some(raw_slug) = value.as_str() else {
            return Err(ThemeError::InvalidType {
                field: "theme_preset".to_string(),
                expected: "string".to_string(),
            });
        };
        if !raw_slug.trim().is_empty() {
            app.theme =
                theme_presets::resolve_preset(raw_slug).map_err(|_| ThemeError::UnknownPreset {
                    value: raw_slug.to_string(),
                    known: theme_presets::known_presets(),
                })?;
        }
    }

    if let Some(value) = table.get("theme") {
        let Some(theme_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "theme".to_string(),
                expected: "table".to_string(),
            });
        };
        app.theme.merge_theme_table(theme_table)?;
    }

    if let Some(value) = table.get("spacing") {
        let Some(spacing_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "spacing".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_spacing_table(&mut app.spacing, spacing_table)?;
    }

    if let Some(value) = table.get("typography") {
        let Some(typography_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "typography".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_typography_table(&mut app.typography, typography_table)?;
    }

    if let Some(value) = table.get("cursor") {
        let Some(cursor_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "cursor".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_cursor_table(&mut app.cursor, cursor_table)?;
    }

    if let Some(value) = table.get("editor") {
        let Some(editor_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "editor".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_editor_table(&mut app.editor, editor_table)?;
    }

    if let Some(value) = table.get("ui") {
        let Some(ui_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "ui".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_ui_table(&mut app.ui, ui_table)?;
    }

    if let Some(value) = table.get("keymap") {
        let Some(keymap_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "keymap".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_keymap_table(&mut app.keymap, keymap_table)?;
    }

    if let Some(value) = table.get("paths") {
        let Some(paths_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: "paths".to_string(),
                expected: "table".to_string(),
            });
        };
        merge_paths_table(&mut app.paths, paths_table)?;
    }

    app.theme.validate_theme_colors()?;
    validate_app_config(&app)?;
    Ok(app)
}

pub fn theme_color_in(theme: &Theme, role: ThemeRole) -> Color {
    let raw = match role {
        ThemeRole::Heading1 if normalize_color_name(theme.value_for(role)).is_empty() => {
            theme.value_for(ThemeRole::Accent)
        }
        ThemeRole::Heading2 if normalize_color_name(theme.value_for(role)).is_empty() => {
            theme.value_for(ThemeRole::Accent)
        }
        ThemeRole::Heading3 if normalize_color_name(theme.value_for(role)).is_empty() => {
            theme.value_for(ThemeRole::Accent)
        }
        ThemeRole::Heading4 if normalize_color_name(theme.value_for(role)).is_empty() => {
            theme.value_for(ThemeRole::Accent)
        }
        ThemeRole::Heading5 if normalize_color_name(theme.value_for(role)).is_empty() => {
            theme.value_for(ThemeRole::Accent)
        }
        ThemeRole::Heading6 if normalize_color_name(theme.value_for(role)).is_empty() => {
            theme.value_for(ThemeRole::Accent)
        }
        _ => theme.value_for(role),
    };

    if let Some(color) = parse_ansi_color_name(raw) {
        return color;
    }
    if let Some(color) = parse_ansi_color_name(theme.value_for(ThemeRole::Foreground)) {
        return color;
    }
    Color::White
}

pub fn normalize_color_name(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace("grey", "gray")
}

pub fn parse_ansi_color_name(value: &str) -> Option<Color> {
    let name = normalize_color_name(value);
    match name.as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "darkgray" | "dark_gray" => Some(Color::DarkGray),
        "gray" => Some(Color::Gray),
        _ => None,
    }
}

pub fn parse_preset_catalog(raw: &str) -> Result<BTreeMap<String, Theme>, ThemeError> {
    let parsed: Value = toml::from_str(raw).map_err(ThemeError::InvalidToml)?;
    let root = parsed.as_table().ok_or_else(|| ThemeError::InvalidType {
        field: "preset_catalog".to_string(),
        expected: "table".to_string(),
    })?;
    let presets = root
        .get("presets")
        .and_then(Value::as_table)
        .ok_or_else(|| ThemeError::InvalidType {
            field: "presets".to_string(),
            expected: "table".to_string(),
        })?;

    let mut out = BTreeMap::new();
    for (slug, value) in presets {
        let Some(theme_table) = value.as_table() else {
            return Err(ThemeError::InvalidType {
                field: format!("presets.{slug}"),
                expected: "table".to_string(),
            });
        };
        let mut theme = Theme::default();
        theme.merge_theme_table(theme_table)?;
        theme.validate_theme_colors()?;
        out.insert(theme_presets::normalize_slug(slug), theme);
    }
    Ok(out)
}

fn parse_usize_in_range(
    field: &str,
    value: &Value,
    min: usize,
    max: usize,
) -> Result<usize, ThemeError> {
    let Some(raw) = value.as_integer() else {
        return Err(ThemeError::InvalidType {
            field: field.to_string(),
            expected: "integer".to_string(),
        });
    };
    if raw < min as i64 || raw > max as i64 {
        return Err(ThemeError::InvalidRange {
            field: field.to_string(),
            min,
            max,
            value: raw.max(0) as usize,
        });
    }
    Ok(raw as usize)
}

fn parse_bool(field: &str, value: &Value) -> Result<bool, ThemeError> {
    value.as_bool().ok_or_else(|| ThemeError::InvalidType {
        field: field.to_string(),
        expected: "boolean".to_string(),
    })
}

fn parse_string(field: &str, value: &Value) -> Result<String, ThemeError> {
    value
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| ThemeError::InvalidType {
            field: field.to_string(),
            expected: "string".to_string(),
        })
}

fn merge_spacing_table(
    spacing: &mut SpacingConfig,
    table: &toml::map::Map<String, Value>,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        match key.as_str() {
            "line_gap_lines" => {
                spacing.line_gap_lines = parse_usize_in_range("spacing.line_gap_lines", value, 0, 8)?
            }
            "paragraph_gap_lines" => {
                spacing.paragraph_gap_lines =
                    parse_usize_in_range("spacing.paragraph_gap_lines", value, 0, 8)?
            }
            "code_margin" => {
                spacing.code_margin = parse_usize_in_range("spacing.code_margin", value, 0, 12)?
            }
            _ => return Err(ThemeError::UnknownThemeKey(format!("spacing.{key}"))),
        }
    }
    Ok(())
}

fn merge_typography_table(
    typography: &mut TypographyConfig,
    table: &toml::map::Map<String, Value>,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        match key.as_str() {
            "extra_word_spacing" => {
                typography.extra_word_spacing =
                    parse_usize_in_range("typography.extra_word_spacing", value, 0, 4)?
            }
            "extra_letter_spacing" => {
                typography.extra_letter_spacing =
                    parse_usize_in_range("typography.extra_letter_spacing", value, 0, 2)?
            }
            _ => return Err(ThemeError::UnknownThemeKey(format!("typography.{key}"))),
        }
    }
    Ok(())
}

fn merge_cursor_table(
    cursor: &mut CursorConfig,
    table: &toml::map::Map<String, Value>,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        match key.as_str() {
            "style" => {
                let raw = parse_string("cursor.style", value)?;
                cursor.style = match normalize_cursor_style(&raw).as_str() {
                    "block" => CursorStyleConfig::Block,
                    "line" => CursorStyleConfig::Line,
                    "underline" => CursorStyleConfig::Underline,
                    _ => {
                        return Err(ThemeError::InvalidEnum {
                            field: "cursor.style".to_string(),
                            value: raw,
                            expected: vec!["block", "line", "underline"],
                        });
                    }
                };
            }
            "blink" => cursor.blink = parse_bool("cursor.blink", value)?,
            _ => return Err(ThemeError::UnknownThemeKey(format!("cursor.{key}"))),
        }
    }
    Ok(())
}

fn merge_editor_table(
    editor: &mut EditorConfig,
    table: &toml::map::Map<String, Value>,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        match key.as_str() {
            "tab_width" => editor.tab_width = parse_usize_in_range("editor.tab_width", value, 1, 12)?,
            "hard_tabs" => editor.hard_tabs = parse_bool("editor.hard_tabs", value)?,
            "auto_wrap" => editor.auto_wrap = parse_bool("editor.auto_wrap", value)?,
            "show_invisibles" => editor.show_invisibles = parse_bool("editor.show_invisibles", value)?,
            _ => return Err(ThemeError::UnknownThemeKey(format!("editor.{key}"))),
        }
    }
    Ok(())
}

fn merge_paths_table(
    paths: &mut PathsConfig,
    table: &toml::map::Map<String, Value>,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        match key.as_str() {
            "writing_folder" => {
                let raw = parse_string("paths.writing_folder", value)?;
                paths.writing_folder = resolve_writing_folder_from_config_value(Some(raw.as_str()));
            }
            _ => return Err(ThemeError::UnknownThemeKey(format!("paths.{key}"))),
        }
    }
    Ok(())
}

fn merge_ui_table(ui: &mut UiConfig, table: &toml::map::Map<String, Value>) -> Result<(), ThemeError> {
    for (key, value) in table {
        match key.as_str() {
            "status_details_default" => {
                ui.status_details_default = parse_bool("ui.status_details_default", value)?
            }
            "start_menu_title" => {
                ui.start_menu_title = parse_string("ui.start_menu_title", value)?.trim().to_string()
            }
            _ => return Err(ThemeError::UnknownThemeKey(format!("ui.{key}"))),
        }
    }
    Ok(())
}

fn merge_keymap_table(
    keymap: &mut KeymapConfig,
    table: &toml::map::Map<String, Value>,
) -> Result<(), ThemeError> {
    for (key, value) in table {
        let binding = parse_string(&format!("keymap.{key}"), value)?;
        let binding = binding.trim().to_ascii_lowercase();
        if binding.is_empty() {
            return Err(ThemeError::EmptyRequiredField(format!("keymap.{key}")));
        }
        keymap.bindings.insert(key.to_ascii_lowercase(), binding);
    }
    Ok(())
}

fn normalize_cursor_style(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn validate_app_config(app: &AppConfig) -> Result<(), ThemeError> {
    if app.ui.start_menu_title.is_empty() {
        return Err(ThemeError::EmptyRequiredField(
            "ui.start_menu_title".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_can_be_empty_and_falls_back_to_accent() {
        let theme = Theme::default();
        assert_eq!(
            theme_color_in(&theme, ThemeRole::Heading1),
            theme_color_in(&theme, ThemeRole::Accent)
        );
    }

    #[test]
    fn preset_then_override_precedence() {
        let raw = r#"
theme_preset = "dark"

[theme]
foreground = "yellow"
"#;
        let theme = load_theme_from_str(raw).expect("theme loads");
        assert_eq!(normalize_color_name(&theme.foreground), "yellow");
        assert_eq!(normalize_color_name(&theme.background), "black");
    }

    #[test]
    fn unknown_theme_key_is_rejected() {
        let raw = r#"
[theme]
wat = "red"
"#;
        let err = load_theme_from_str(raw).expect_err("must fail");
        assert!(err.to_string().contains("unknown theme role"));
    }

    #[test]
    fn invalid_required_color_is_rejected() {
        let raw = r#"
[theme]
foreground = "beige"
"#;
        let err = load_theme_from_str(raw).expect_err("must fail");
        assert!(err.to_string().contains("invalid ANSI color name"));
    }

    #[test]
    fn app_config_parses_extended_sections() {
        let raw = r#"
theme_preset = "dark"

[spacing]
line_gap_lines = 1
paragraph_gap_lines = 2
code_margin = 2

[typography]
extra_word_spacing = 1
extra_letter_spacing = 1

[cursor]
style = "block"
blink = false

[editor]
tab_width = 2
hard_tabs = false
auto_wrap = true
show_invisibles = true

[ui]
status_details_default = true
start_menu_title = "ANSI-Shadow"

[keymap]
quit = "ctrl+x"
"#;
        let app = load_app_config_from_str(raw).expect("app config parses");
        assert_eq!(app.spacing.line_gap_lines, 1);
        assert_eq!(app.typography.extra_word_spacing, 1);
        assert_eq!(app.cursor.style, CursorStyleConfig::Block);
        assert_eq!(app.editor.tab_width, 2);
        assert!(app.ui.status_details_default);
        assert_eq!(app.keymap.bindings.get("quit"), Some(&"ctrl+x".to_string()));
    }

    #[test]
    fn app_config_rejects_invalid_cursor_style() {
        let raw = r#"
[cursor]
style = "bar"
"#;
        let err = load_app_config_from_str(raw).expect_err("must fail");
        assert!(err.to_string().contains("cursor.style"));
    }

    #[test]
    fn app_config_rejects_out_of_range_spacing() {
        let raw = r#"
[spacing]
line_gap_lines = 100
"#;
        let err = load_app_config_from_str(raw).expect_err("must fail");
        assert!(err.to_string().contains("spacing.line_gap_lines"));
    }

    #[test]
    fn app_config_rejects_empty_key_binding() {
        let raw = r#"
[keymap]
quit = ""
"#;
        let err = load_app_config_from_str(raw).expect_err("must fail");
        assert!(err.to_string().contains("keymap.quit"));
    }

    #[test]
    fn resolve_writing_folder_absolute_is_unchanged() {
        assert_eq!(
            resolve_writing_folder_from_config_value(Some("/abs/writing")),
            PathBuf::from("/abs/writing")
        );
    }

    #[test]
    fn app_config_paths_absolute_writing_folder() {
        let raw = r#"
[ui]
start_menu_title = "ANSI-Shadow"

[paths]
writing_folder = "/custom/writing"
"#;
        let app = load_app_config_from_str(raw).expect("parses");
        assert_eq!(app.paths.writing_folder, PathBuf::from("/custom/writing"));
    }

    #[test]
    fn app_config_paths_relative_writing_folder() {
        let raw = r#"
[ui]
start_menu_title = "ANSI-Shadow"

[paths]
writing_folder = "notes/inbox"
"#;
        let app = load_app_config_from_str(raw).expect("parses");
        assert_eq!(app.paths.writing_folder, PathBuf::from("notes/inbox"));
    }

    #[test]
    fn app_config_paths_empty_string_uses_default() {
        let raw = r#"
[ui]
start_menu_title = "ANSI-Shadow"

[paths]
writing_folder = "   "
"#;
        let app = load_app_config_from_str(raw).expect("parses");
        assert_eq!(app.paths.writing_folder, default_writing_folder_path());
    }

    #[test]
    fn app_config_paths_unknown_key_is_rejected() {
        let raw = r#"
[ui]
start_menu_title = "ANSI-Shadow"

[paths]
bogus = "/x"
"#;
        let err = load_app_config_from_str(raw).expect_err("must fail");
        assert!(err.to_string().contains("paths.bogus"));
    }
}
