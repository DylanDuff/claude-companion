//! Syntax highlighting for code blocks, via syntect's bundled Sublime Text
//! grammars. Colours come from the user's Zed theme when it can be found
//! (`zed_theme.rs`), else a bundled theme.

use std::{ops::Range, sync::OnceLock};

use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

use crate::zed_theme;

/// Used when no Zed theme is found.
const FALLBACK_THEME: &str = "base16-ocean.dark";

/// How a run of code is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    /// Foreground as `0xRRGGBB`.
    pub color: u32,
    pub italic: bool,
    pub bold: bool,
}

/// A byte range of the code and how to draw it.
pub type Highlight = (Range<usize>, Paint);

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
    /// The Zed theme's editor background, if a Zed theme is in use.
    background: Option<u32>,
}

static ASSETS: OnceLock<Assets> = OnceLock::new();

fn assets() -> &'static Assets {
    ASSETS.get_or_init(|| {
        let syntaxes = SyntaxSet::load_defaults_newlines();
        match zed_theme::load() {
            Some(zed) => Assets {
                syntaxes,
                theme: zed.syntax,
                background: zed.background,
            },
            None => {
                let mut themes = ThemeSet::load_defaults();
                Assets {
                    syntaxes,
                    theme: themes
                        .themes
                        .remove(FALLBACK_THEME)
                        .expect("bundled theme exists"),
                    background: None,
                }
            }
        }
    })
}

/// Background for code blocks: the Zed theme's editor background, so its
/// colours sit on the surface they were designed for.
pub fn code_background() -> Option<u32> {
    assets().background
}

/// Loads the grammars on a background thread so the first code block
/// doesn't pay for it.
pub fn preload() {
    std::thread::spawn(|| {
        assets();
    });
}

/// Fence labels the bundled grammars don't recognise, mapped to the closest
/// grammar they include.
fn grammar_token(language: &str) -> &str {
    match language {
        "ts" | "typescript" | "tsx" | "jsx" | "mjs" | "cjs" => "js",
        "shell" | "zsh" | "console" | "sh-session" => "sh",
        "golang" => "go",
        "yml" => "yaml",
        "c++" => "cpp",
        other => other,
    }
}

/// Colours `code` written in `language`. Returns nothing for unknown or
/// missing languages, which render as plain monospace text.
pub fn highlight(language: Option<&str>, code: &str) -> Vec<Highlight> {
    let Some(language) = language else {
        return Vec::new();
    };
    let assets = assets();
    let token = grammar_token(&language.to_ascii_lowercase()).to_string();
    let Some(syntax) = assets.syntaxes.find_syntax_by_token(&token) else {
        return Vec::new();
    };

    let mut highlighter = HighlightLines::new(syntax, &assets.theme);
    let mut highlights: Vec<Highlight> = Vec::new();
    let mut offset = 0;
    for line in LinesWithEndings::from(code) {
        let Ok(regions) = highlighter.highlight_line(line, &assets.syntaxes) else {
            return Vec::new();
        };
        for (style, text) in regions {
            let color = style.foreground;
            let paint = Paint {
                color: u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b),
                italic: style.font_style.contains(FontStyle::ITALIC),
                bold: style.font_style.contains(FontStyle::BOLD),
            };
            let range = offset..offset + text.len();
            offset = range.end;
            match highlights.last_mut() {
                Some((previous, previous_paint))
                    if *previous_paint == paint && previous.end == range.start =>
                {
                    previous.end = range.end;
                }
                _ => highlights.push((range, paint)),
            }
        }
    }
    highlights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_known_languages() {
        let code = "fn main() {}";
        let highlights = highlight(Some("rust"), code);
        assert!(highlights.len() > 1);
        assert_eq!(highlights.last().unwrap().0.end, code.len());
    }

    #[test]
    fn maps_aliases() {
        assert!(!highlight(Some("ts"), "const x = 1;").is_empty());
    }

    #[test]
    fn leaves_unknown_languages_plain() {
        assert!(highlight(Some("not-a-language"), "x").is_empty());
        assert!(highlight(None, "x").is_empty());
    }
}
