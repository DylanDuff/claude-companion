use gpui::{Hsla, Rgba, rgb, rgba};

// Geometry. Corner radii nest: a container's radius is its child's radius
// plus the gap between them, so the curves stay concentric. The chain runs
// outward from the composer's round buttons.

/// Diameter of the composer's icon buttons, including send.
pub const BUTTON_SIZE: f32 = 36.;
/// Gap from the composer card's edge to the buttons inside it.
pub const COMPOSER_INSET: f32 = 6.;
pub const COMPOSER_RADIUS: f32 = BUTTON_SIZE / 2. + COMPOSER_INSET;
/// Gap from the panel's edge to the composer card.
pub const COMPOSER_MARGIN: f32 = 8.;
pub const PANEL_RADIUS: f32 = COMPOSER_RADIUS + COMPOSER_MARGIN;
/// In the compact bar the panel itself is the composer, so its buttons sit
/// this far in to stay concentric with the panel's corners.
pub const COMPACT_INSET: f32 = PANEL_RADIUS - BUTTON_SIZE / 2.;

// The window is blurred (`WindowBackgroundAppearance::Blurred`), so the
// background and the surfaces on it are partly transparent to let it show.

pub fn background() -> Rgba {
    rgba(0x1c1c2080)
}

/// The panel's hairline edge and other dividers.
pub fn window_border() -> Rgba {
    rgba(0xffffff26)
}

/// The composer card and other raised areas.
pub fn surface() -> Rgba {
    rgba(0xffffff0f)
}

pub fn hover() -> Rgba {
    rgba(0xffffff1a)
}

pub fn text() -> Rgba {
    rgb(0xf2f2f4)
}

pub fn muted_text() -> Hsla {
    Hsla::from(rgb(0xf2f2f4)).opacity(0.55)
}

pub fn accent() -> Rgba {
    rgb(0xd97757)
}

pub fn user_bubble() -> Rgba {
    rgba(0xffffff1f)
}

pub fn error() -> Rgba {
    rgb(0xff8a80)
}

pub fn selection() -> Rgba {
    rgba(0xd9775750)
}

pub fn inline_code() -> Rgba {
    rgb(0xf2f2f4)
}

pub fn link() -> Rgba {
    rgb(0x9cc4ff)
}

/// The send button when there's something to send.
pub fn send_ready() -> Rgba {
    rgb(0xf2f2f4)
}

pub fn send_ready_icon() -> Rgba {
    rgb(0x1c1c20)
}

pub fn send_idle() -> Rgba {
    rgba(0xffffff26)
}

/// Track behind a segmented control.
pub fn segment_track() -> Rgba {
    rgba(0x0000002e)
}

/// The selected segment, lifted off the track.
pub fn segment_selected() -> Rgba {
    rgba(0xffffff29)
}

pub fn switch_off() -> Rgba {
    rgba(0xffffff29)
}

pub fn switch_knob() -> Rgba {
    rgb(0xffffff)
}
