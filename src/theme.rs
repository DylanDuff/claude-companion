use gpui::{Hsla, Rgba, rgb, rgba};

// The window is blurred (`WindowBackgroundAppearance::Blurred`), so the
// background and the surfaces on it are partly transparent to let it show.

pub fn background() -> Rgba {
    rgba(0x1e1e2299)
}

pub fn surface() -> Rgba {
    rgba(0xffffff14)
}

pub fn border() -> Rgba {
    rgba(0xffffff1f)
}

pub fn text() -> Rgba {
    rgb(0xe8e8ec)
}

pub fn muted_text() -> Hsla {
    Hsla::from(rgb(0xe8e8ec)).opacity(0.5)
}

pub fn accent() -> Rgba {
    rgb(0xd97757)
}

pub fn on_accent() -> Rgba {
    rgb(0xffffff)
}

pub fn user_bubble() -> Rgba {
    rgba(0xffffff24)
}

pub fn error() -> Rgba {
    rgb(0xf07070)
}

pub fn selection() -> Rgba {
    rgba(0xd9775750)
}

pub fn code_background() -> Rgba {
    rgba(0x0000004d)
}

pub fn inline_code() -> Rgba {
    rgb(0xebcb8b)
}

pub fn link() -> Rgba {
    rgb(0x8fb8f0)
}
