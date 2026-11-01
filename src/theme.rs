use gpui::{Hsla, Rgba, rgb, rgba};

pub fn background() -> Rgba {
    rgb(0x1e1e22)
}

pub fn surface() -> Rgba {
    rgb(0x2a2a30)
}

pub fn border() -> Rgba {
    rgb(0x3a3a42)
}

pub fn text() -> Rgba {
    rgb(0xe8e8ec)
}

pub fn muted_text() -> Hsla {
    Hsla::from(rgb(0xe8e8ec)).opacity(0.45)
}

pub fn accent() -> Rgba {
    rgb(0xd97757)
}

pub fn on_accent() -> Rgba {
    rgb(0xffffff)
}

pub fn user_bubble() -> Rgba {
    rgb(0x3b3b45)
}

pub fn error() -> Rgba {
    rgb(0xf07070)
}

pub fn selection() -> Rgba {
    rgba(0xd9775750)
}
