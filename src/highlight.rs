//! Syntax highlighting for code blocks, via syntect's bundled Sublime Text
//! grammars and themes.

use std::{ops::Range, sync::OnceLock};

use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

const THEME: &str = "base16-ocean.dark";

/// A byte range of the code and its foreground colour as `0xRRGGBB`.
pub type Highlight = (Range<usize>, u32);

struct Assets {
    syntaxes: SyntaxSet,
    theme: Theme,
}

static ASSETS: OnceLock<Assets> = OnceLock::new();

fn assets() -> &'static Assets {
    ASSETS.get_or_init(|| {
        let mut themes = ThemeSet::load_defaults();
        Assets {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            theme: themes.themes.remove(THEME).expect("bundled theme exists"),
        }
    })
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
            let color = u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b);
            let range = offset..offset + text.len();
            offset = range.end;
            match highlights.last_mut() {
                Some((previous, previous_color))
                    if *previous_color == color && previous.end == range.start =>
                {
                    previous.end = range.end;
                }
                _ => highlights.push((range, color)),
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
