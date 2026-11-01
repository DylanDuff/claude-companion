//! A band of light that sweeps across status text while Claude works.
//!
//! gpui can't mask text with a moving gradient, so each character gets its
//! own text run, brightened by its distance from the band's centre. The
//! caller keeps frames coming (`Window::request_animation_frame`) while a
//! shimmer is on screen.

use std::{sync::OnceLock, time::Instant};

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Hsla, SharedString, StyledText, TextRun};

/// Seconds for the band to cross the text once.
const PERIOD: f32 = 1.8;
/// Half-width of the band, in characters.
const BAND: f32 = 4.;
/// Brightness of text away from the band, as a share of its full opacity.
const BASE: f32 = 0.45;

/// Progress through the current sweep, from 0 to 1. Shared by every
/// shimmer, so the text and its icon stay in step.
fn phase() -> f32 {
    static START: OnceLock<Instant> = OnceLock::new();
    let elapsed = START.get_or_init(Instant::now).elapsed().as_secs_f32();
    (elapsed % PERIOD) / PERIOD
}

/// How brightly the band lights position `index` of `len`, from 0 to 1.
/// The band starts and ends just off the text, so each sweep enters and
/// leaves smoothly.
fn light(index: f32, len: f32, phase: f32) -> f32 {
    let centre = -BAND + (len + BAND * 2.) * phase;
    let k = (1. - (index - centre).abs() / BAND).max(0.);
    k * k * (3. - 2. * k)
}

/// `text` in `color`, with the band sweeping across it.
pub fn text(text: impl Into<SharedString>, color: Hsla) -> StyledText {
    let text = text.into();
    let phase = phase();
    let len = text.chars().count() as f32;
    let runs = text
        .chars()
        .enumerate()
        .map(|(index, char)| TextRun {
            len: char.len_utf8(),
            font: Font {
                family: ".SystemUIFont".into(),
                features: FontFeatures::default(),
                fallbacks: None,
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
            },
            color: color.opacity(BASE + (1. - BASE) * light(index as f32, len, phase)),
            background_color: None,
            underline: None,
            strikethrough: None,
        })
        .collect();
    StyledText::new(text).with_runs(runs)
}

/// Opacity for an icon beside shimmering text: brightest as the band
/// passes its leading edge.
pub fn icon_opacity() -> f32 {
    BASE + (1. - BASE) * light(0., BAND * 2., phase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_peaks_at_its_centre_and_fades() {
        let len = 10.;
        // Phase where the centre sits on index 3.
        let phase = (3. + BAND) / (len + BAND * 2.);
        assert!((light(3., len, phase) - 1.).abs() < 1e-5);
        assert!(light(5., len, phase) < 1.);
        assert_eq!(light(3. + BAND, len, phase), 0.);
    }

    #[test]
    fn sweep_starts_and_ends_off_the_text() {
        assert_eq!(light(0., 10., 0.), 0.);
        assert_eq!(light(9., 10., 1.), 0.);
    }
}
