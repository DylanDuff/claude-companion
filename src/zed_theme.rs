//! Borrows the user's Zed theme for code highlighting.
//!
//! Zed themes style tree-sitter captures (`keyword`, `function.method`, …)
//! while syntect grammars produce TextMate scopes (`storage.type`,
//! `entity.name.function`, …), so the theme is translated: each TextMate
//! selector in [`SCOPES`] takes the colour of the first Zed key that the
//! theme defines.

use std::{fs, path::PathBuf, str::FromStr};

use serde_json::Value;
use syntect::highlighting::{
    Color, FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings,
};

/// TextMate selectors and the Zed syntax keys to colour them with, most
/// specific first. syntect applies the most specific matching selector, so
/// broad entries (`keyword`) and narrow ones (`keyword.operator`) coexist.
const SCOPES: &[(&str, &[&str])] = &[
    ("comment, punctuation.definition.comment", &["comment"]),
    (
        "comment.block.documentation, comment.line.documentation",
        &["comment.doc", "comment"],
    ),
    ("string, punctuation.definition.string", &["string"]),
    ("string.regexp", &["string.regex", "string"]),
    ("constant.character.escape", &["string.escape", "string"]),
    ("constant.character", &["character", "string"]),
    ("constant.numeric", &["number"]),
    ("constant.language", &["boolean", "constant"]),
    (
        "constant, support.constant, entity.name.constant, variable.other.constant",
        &["constant"],
    ),
    ("keyword, storage", &["keyword"]),
    ("keyword.operator", &["operator"]),
    (
        "keyword.control.import, keyword.control.export",
        &["keyword.import", "keyword"],
    ),
    (
        "entity.name.function, support.function, variable.function",
        &["function"],
    ),
    (
        "entity.name.function.macro, support.macro",
        &["function.macro", "function"],
    ),
    (
        "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, \
         entity.name.trait, entity.name.impl, entity.other.inherited-class, support.type, \
         support.class",
        &["type"],
    ),
    ("variable", &["variable"]),
    (
        "variable.parameter",
        &["variable.parameter", "parameter", "variable"],
    ),
    (
        "variable.language",
        &["variable.special", "variable.builtin", "variable"],
    ),
    (
        "variable.other.member, variable.other.property, support.type.property-name, \
         meta.property-name",
        &["property"],
    ),
    ("entity.name.tag", &["tag"]),
    (
        "entity.other.attribute-name, meta.attribute, meta.annotation",
        &["attribute"],
    ),
    (
        "entity.name.namespace, entity.name.module, entity.name.package",
        &["namespace", "module"],
    ),
    ("entity.name.label", &["label"]),
    ("punctuation", &["punctuation"]),
    (
        "punctuation.section, punctuation.bracket, meta.brace",
        &["punctuation.bracket", "punctuation"],
    ),
    ("markup.heading", &["title"]),
    ("markup.bold", &["emphasis.strong"]),
    ("markup.italic", &["emphasis"]),
    ("markup.underline.link, markup.link", &["link_uri"]),
    ("markup.raw", &["text.literal", "string"]),
];

pub struct ZedTheme {
    pub syntax: Theme,
    /// The editor background as `0xRRGGBB`, for code blocks.
    pub background: Option<u32>,
}

/// Loads the theme Zed is set to use, if it can be found on disk. Zed's
/// built-in themes live inside the Zed binary, so only user and extension
/// themes are available.
pub fn load() -> Option<ZedTheme> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let settings = fs::read_to_string(home.join(".config/zed/settings.json")).ok()?;
    let name = selected_theme(&settings)?;
    let theme = find_theme(&home, &name)?;
    Some(convert(name, &theme))
}

/// The theme name from Zed's `"theme"` setting: a plain name, or the dark
/// variant of `{ "mode", "light", "dark" }`, since the companion is dark.
fn selected_theme(settings: &str) -> Option<String> {
    let settings: Value = serde_json::from_str(&strip_jsonc(settings)).ok()?;
    match &settings["theme"] {
        Value::String(name) => Some(name.clone()),
        Value::Object(choice) => choice
            .get("dark")
            .or_else(|| choice.get("light"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// Searches user themes and installed extensions for a theme named `name`.
fn find_theme(home: &std::path::Path, name: &str) -> Option<Value> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut add_dir = |dir: PathBuf| {
        if let Ok(entries) = fs::read_dir(dir) {
            files.extend(
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.extension().is_some_and(|ext| ext == "json")),
            );
        }
    };
    add_dir(home.join(".config/zed/themes"));
    let extensions = home.join("Library/Application Support/Zed/extensions/installed");
    if let Ok(entries) = fs::read_dir(extensions) {
        for entry in entries.flatten() {
            add_dir(entry.path().join("themes"));
        }
    }

    files.into_iter().find_map(|path| {
        let family: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        family["themes"]
            .as_array()?
            .iter()
            .find(|theme| theme["name"].as_str() == Some(name))
            .cloned()
    })
}

fn convert(name: String, theme: &Value) -> ZedTheme {
    let style = &theme["style"];
    let syntax = &style["syntax"];
    let foreground = parse_color(&style["editor.foreground"]);
    let background = parse_color(&style["editor.background"]);

    let scopes = SCOPES
        .iter()
        .filter_map(|(selector, keys)| {
            let style = keys.iter().find_map(|key| resolve(syntax, key))?;
            Some(ThemeItem {
                scope: ScopeSelectors::from_str(selector).ok()?,
                style: style_modifier(style),
            })
        })
        .collect();

    ZedTheme {
        syntax: Theme {
            name: Some(name),
            author: None,
            settings: ThemeSettings {
                foreground,
                background,
                ..ThemeSettings::default()
            },
            scopes,
        },
        background: background.map(rgb),
    }
}

/// Looks up a syntax key the way Zed does: `keyword.import` falls back to
/// `keyword` when the theme doesn't define it.
fn resolve<'a>(syntax: &'a Value, key: &str) -> Option<&'a Value> {
    let mut key = key;
    loop {
        if let Some(style) = syntax.get(key).filter(|style| style["color"].is_string()) {
            return Some(style);
        }
        key = &key[..key.rfind('.')?];
    }
}

fn style_modifier(style: &Value) -> StyleModifier {
    let mut font_style = FontStyle::empty();
    if style["font_style"].as_str() == Some("italic") {
        font_style |= FontStyle::ITALIC;
    }
    if style["font_weight"]
        .as_f64()
        .is_some_and(|weight| weight >= 600.)
    {
        font_style |= FontStyle::BOLD;
    }
    StyleModifier {
        foreground: parse_color(&style["color"]),
        background: None,
        font_style: Some(font_style),
    }
}

/// Parses `#rrggbb` or `#rrggbbaa`.
fn parse_color(value: &Value) -> Option<Color> {
    let hex = value.as_str()?.strip_prefix('#')?;
    let channel = |index: usize| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok();
    Some(Color {
        r: channel(0)?,
        g: channel(2)?,
        b: channel(4)?,
        a: if hex.len() >= 8 { channel(6)? } else { 0xff },
    })
}

fn rgb(color: Color) -> u32 {
    u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)
}

/// Turns Zed's JSON-with-comments settings into plain JSON: drops `//` and
/// `/* */` comments and trailing commas, leaving string contents alone.
fn strip_jsonc(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while chars.next_if(|&next| next != '\n').is_some() {}
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            '}' | ']' => {
                let trimmed = out.trim_end().len();
                if out[..trimmed].ends_with(',') {
                    out.truncate(trimmed - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_comments_and_trailing_commas() {
        let source = r#"// header
{
  "a": "keep // this", /* block */
  "b": [1, 2,],
  "c": { "d": "x\"y", },
}"#;
        let value: Value = serde_json::from_str(&strip_jsonc(source)).unwrap();
        assert_eq!(value["a"], "keep // this");
        assert_eq!(value["b"], serde_json::json!([1, 2]));
        assert_eq!(value["c"]["d"], "x\"y");
    }

    #[test]
    fn picks_the_dark_theme() {
        let settings = r#"{ "theme": { "mode": "system", "light": "L", "dark": "D" } }"#;
        assert_eq!(selected_theme(settings).as_deref(), Some("D"));
        assert_eq!(
            selected_theme(r#"{ "theme": "Plain" }"#).as_deref(),
            Some("Plain")
        );
    }

    #[test]
    fn resolves_keys_with_zed_fallback() {
        let syntax = serde_json::json!({
            "keyword": { "color": "#112233" },
            "comment": { "color": null },
        });
        assert!(resolve(&syntax, "keyword.import").is_some());
        assert!(resolve(&syntax, "comment").is_none());
        assert!(resolve(&syntax, "missing.key").is_none());
    }

    #[test]
    fn converts_colours_and_styles() {
        let theme = serde_json::json!({
            "style": {
                "editor.background": "#24273a",
                "editor.foreground": "#cad3f5",
                "syntax": {
                    "keyword": { "color": "#c6a0f6" },
                    "comment": { "color": "#939ab7", "font_style": "italic" },
                },
            },
        });
        let converted = convert("Test".into(), &theme);
        assert_eq!(converted.background, Some(0x24273a));
        let comment = converted
            .syntax
            .scopes
            .iter()
            .find(|item| item.style.font_style == Some(FontStyle::ITALIC))
            .expect("comment style");
        assert_eq!(comment.style.foreground.map(rgb), Some(0x939ab7));
    }
}
