//! The code highlighting theme: Catppuccin Macchiato, matching the user's
//! Zed theme.
//!
//! The colours were taken from the Catppuccin Zed extension's
//! `catppuccin-mauve.json`. Zed themes style tree-sitter captures, while
//! syntect grammars emit TextMate scopes, so each entry in [`SCOPES`] pairs a
//! TextMate selector with the colour Zed gives the matching capture
//! (`keyword.operator` ↔ `operator`, `support.macro` ↔ `function.macro`, …).

use std::str::FromStr;

use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};

// Catppuccin Macchiato palette.
const ROSEWATER: u32 = 0xf4dbd6;
const PINK: u32 = 0xf5bde6;
const MAUVE: u32 = 0xc6a0f6;
const RED: u32 = 0xed8796;
const MAROON: u32 = 0xee99a0;
const PEACH: u32 = 0xf5a97f;
const YELLOW: u32 = 0xeed49f;
const GREEN: u32 = 0xa6da95;
const TEAL: u32 = 0x8bd5ca;
const SKY: u32 = 0x91d7e3;
const SAPPHIRE: u32 = 0x7dc4e4;
const BLUE: u32 = 0x8aadf4;
const LAVENDER: u32 = 0xb7bdf8;
const TEXT: u32 = 0xcad3f5;
const OVERLAY_2: u32 = 0x939ab7;
const BASE: u32 = 0x24273a;

/// Code block background: Zed's editor background in this theme.
pub const BACKGROUND: u32 = BASE;

const PLAIN: FontStyle = FontStyle::empty();
const ITALIC: FontStyle = FontStyle::ITALIC;
const BOLD: FontStyle = FontStyle::BOLD;

/// TextMate selectors and how to draw them. syntect applies the most
/// specific matching selector, so broad entries (`keyword`) and narrow ones
/// (`keyword.operator`) coexist.
const SCOPES: &[(&str, u32, FontStyle)] = &[
    ("comment, punctuation.definition.comment", OVERLAY_2, ITALIC),
    ("string, punctuation.definition.string", GREEN, PLAIN),
    ("string.regexp, constant.character.escape", PINK, PLAIN),
    ("constant.character", TEAL, PLAIN),
    (
        "constant, constant.numeric, constant.language, support.constant, \
         entity.name.constant, variable.other.constant",
        PEACH,
        PLAIN,
    ),
    ("keyword, storage", MAUVE, PLAIN),
    ("keyword.operator", SKY, PLAIN),
    (
        "entity.name.function, support.function, variable.function",
        BLUE,
        PLAIN,
    ),
    (
        "entity.name.function.macro, support.macro",
        ROSEWATER,
        PLAIN,
    ),
    (
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
         entity.name.trait, entity.name.impl, entity.other.inherited-class, support.type, \
         support.class, entity.other.attribute-name, meta.attribute, meta.annotation",
        YELLOW,
        PLAIN,
    ),
    (
        "entity.name.namespace, entity.name.module, entity.name.package",
        YELLOW,
        ITALIC,
    ),
    ("variable", TEXT, PLAIN),
    ("variable.parameter", MAROON, PLAIN),
    ("variable.language", RED, ITALIC),
    (
        "variable.other.member, variable.other.property, support.type.property-name, \
         meta.property-name",
        LAVENDER,
        PLAIN,
    ),
    (
        "entity.name.tag, markup.underline.link, markup.link",
        BLUE,
        PLAIN,
    ),
    ("entity.name.label", SAPPHIRE, PLAIN),
    (
        "punctuation, punctuation.section, punctuation.bracket, meta.brace",
        OVERLAY_2,
        PLAIN,
    ),
    ("markup.heading", RED, BOLD),
    ("markup.bold", MAROON, BOLD),
    ("markup.italic", MAROON, ITALIC),
    ("markup.raw", GREEN, PLAIN),
];

pub fn catppuccin_macchiato() -> Theme {
    let scopes = SCOPES
        .iter()
        .map(|&(selector, color, font_style)| ThemeItem {
            scope: ScopeSelectors::from_str(selector).expect("valid scope selector"),
            style: StyleModifier {
                foreground: Some(color_from(color)),
                background: None,
                font_style: Some(font_style),
            },
        })
        .collect();
    Theme {
        name: Some("Catppuccin Macchiato".into()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(color_from(TEXT)),
            background: Some(color_from(BASE)),
            ..ThemeSettings::default()
        },
        scopes,
    }
}

fn color_from(rgb: u32) -> Color {
    Color {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
        a: 0xff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_selector_parses() {
        assert_eq!(catppuccin_macchiato().scopes.len(), SCOPES.len());
    }
}
