//! Embedded SVG icons and the round icon button used across the UI.

use std::borrow::Cow;

use gpui::{
    AnyElement, App, AssetSource, ClickEvent, ElementId, Hsla, SharedString, Window, div,
    prelude::*, px, svg,
};

use crate::theme;

macro_rules! icons {
    ($($variant:ident => $file:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $($variant),+
        }

        impl Icon {
            fn path(self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("icons/", $file)),+
                }
            }
        }

        /// Serves the icons to GPUI's `svg()` from bytes compiled into the
        /// binary.
        pub struct Assets;

        impl AssetSource for Assets {
            fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
                let bytes: Option<&'static [u8]> = match path {
                    $(concat!("icons/", $file) => {
                        Some(include_bytes!(concat!("../assets/icons/", $file)))
                    })+
                    _ => None,
                };
                Ok(bytes.map(Cow::Borrowed))
            }

            fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
                Ok(Vec::new())
            }
        }
    };
}

icons!(
    ArrowUp => "arrow-up.svg",
    Check => "check.svg",
    Copy => "copy.svg",
    Globe => "globe.svg",
    Plus => "plus.svg",
    Retry => "retry.svg",
    Settings => "settings.svg",
    Speaker => "speaker.svg",
    Stop => "stop.svg",
);

impl Icon {
    pub fn render(self, size: f32, color: impl Into<Hsla>) -> AnyElement {
        svg()
            .path(self.path())
            .size(px(size))
            .flex_none()
            .text_color(color.into())
            .into_any_element()
    }
}

type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A round, borderless button holding one icon.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    /// Diameter of the button.
    size: f32,
    icon_size: f32,
    color: Hsla,
    background: Option<Hsla>,
    on_click: Option<OnClick>,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self {
            id: id.into(),
            icon,
            size: 36.,
            icon_size: 20.,
            color: theme::text().into(),
            background: None,
            on_click: None,
        }
    }

    pub fn small(mut self) -> Self {
        self.size = 28.;
        self.icon_size = 16.;
        self.color = theme::muted_text();
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = color.into();
        self
    }

    pub fn background(mut self, background: impl Into<Hsla>) -> Self {
        self.background = Some(background.into());
        self
    }

    pub fn on_click(
        mut self,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(on_click));
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let clickable = self.on_click.is_some();
        div()
            .id(self.id)
            .size(px(self.size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .when_some(self.background, |this, background| this.bg(background))
            .when(clickable, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(theme::hover()))
            })
            .when_some(self.on_click, |this, on_click| this.on_click(on_click))
            .child(self.icon.render(self.icon_size, self.color))
    }
}
