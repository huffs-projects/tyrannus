use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::config::{parse_preset_catalog, Theme, ThemeError};

const BUNDLED_THEME_PRESETS: &str = include_str!("../assets/theme_presets.toml");

static CATALOG: OnceLock<BTreeMap<String, Theme>> = OnceLock::new();

pub fn dark() -> Theme {
    Theme::default()
}

pub fn light() -> Theme {
    Theme {
        background: "white".to_string(),
        foreground: "black".to_string(),
        accent: "magenta".to_string(),
        link: "blue".to_string(),
        link_missing: "red".to_string(),
        cursor_line: "gray".to_string(),
        code: "magenta".to_string(),
        list_selection_foreground: "white".to_string(),
        list_selection_background: "magenta".to_string(),
        heading1: "".to_string(),
        heading2: "".to_string(),
        heading3: "".to_string(),
        heading4: "".to_string(),
        heading5: "".to_string(),
        heading6: "".to_string(),
    }
}

pub fn normalize_slug(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

pub fn resolve_preset(raw_slug: &str) -> Result<Theme, ThemeError> {
    let slug = normalize_slug(raw_slug);
    if slug == "dark" {
        return Ok(dark());
    }
    if slug == "light" {
        return Ok(light());
    }

    let catalog = catalog()?;
    catalog
        .get(&slug)
        .cloned()
        .ok_or(ThemeError::UnknownPreset {
            value: raw_slug.to_string(),
            known: known_presets(),
        })
}

pub fn known_presets() -> Vec<String> {
    let mut known = vec!["dark".to_string(), "light".to_string()];
    if let Ok(catalog) = catalog() {
        known.extend(catalog.keys().cloned());
    }
    known
}

pub fn validate_bundled_presets() -> Result<(), ThemeError> {
    let _ = catalog()?;
    Ok(())
}

fn catalog() -> Result<&'static BTreeMap<String, Theme>, ThemeError> {
    if let Some(existing) = CATALOG.get() {
        return Ok(existing);
    }
    let parsed = parse_preset_catalog(BUNDLED_THEME_PRESETS)?;
    let _ = CATALOG.set(parsed);
    Ok(CATALOG.get().expect("catalog initialized"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_parses() {
        validate_bundled_presets().expect("catalog should be valid");
    }

    #[test]
    fn slug_normalization_is_stable() {
        assert_eq!(normalize_slug(" Tokyo_Night "), "tokyo-night");
    }

    #[test]
    fn unknown_preset_is_rejected() {
        let err = resolve_preset("does-not-exist").expect_err("must fail");
        assert!(err.to_string().contains("unknown value"));
    }
}
