//! User-tunable defaults, persisted as `settings.json`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::storage;

const FILE: &str = "settings.json";

/// A setting with a fixed set of values, shown as a segmented control.
pub trait Choice: Copy + PartialEq + 'static {
    fn all() -> &'static [Self];
    fn label(self) -> &'static str;
}

/// Declares a serialisable enum and its [`Choice`] labels in one place.
macro_rules! choices {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl Choice for $name {
            fn all() -> &'static [Self] {
                &[$(Self::$variant),+]
            }

            fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),+
                }
            }
        }
    };
}

choices!(
    /// `Default` leaves the flag off so the CLI's own default applies.
    Model {
        Default => "Default",
        Fable => "Fable",
        Opus => "Opus",
        Sonnet => "Sonnet",
        Haiku => "Haiku",
    }
);

choices!(
    Effort {
        Default => "Default",
        Low => "Low",
        Medium => "Medium",
        High => "High",
        Xhigh => "XHigh",
        Max => "Max",
    }
);

choices!(
    Timeout {
        OneMinute => "1 min",
        FiveMinutes => "5 min",
        TenMinutes => "10 min",
        ThirtyMinutes => "30 min",
        OneHour => "1 hour",
    }
);

choices!(
    // Variant names double as readable values in settings.json.
    #[allow(clippy::enum_variant_names)]
    Hotkey {
        OptionSpace => "⌥Space",
        ControlSpace => "⌃Space",
        OptionShiftSpace => "⌥⇧Space",
        CommandShiftSpace => "⌘⇧Space",
    }
);

impl Choice for bool {
    fn all() -> &'static [Self] {
        &[true, false]
    }

    fn label(self) -> &'static str {
        if self { "On" } else { "Off" }
    }
}

impl Model {
    /// Value for `--model`.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Fable => Some("fable"),
            Self::Opus => Some("opus"),
            Self::Sonnet => Some("sonnet"),
            Self::Haiku => Some("haiku"),
        }
    }
}

impl Effort {
    /// Value for `--effort`.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::Xhigh => Some("xhigh"),
            Self::Max => Some("max"),
        }
    }
}

impl Timeout {
    pub fn duration(self) -> Duration {
        let minutes = match self {
            Self::OneMinute => 1,
            Self::FiveMinutes => 5,
            Self::TenMinutes => 10,
            Self::ThirtyMinutes => 30,
            Self::OneHour => 60,
        };
        Duration::from_secs(minutes * 60)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: Model,
    pub effort: Effort,
    /// How long a conversation survives without a finished reply.
    pub session_timeout: Timeout,
    pub web_search: bool,
    pub web_fetch: bool,
    pub hotkey: Hotkey,
    /// Hide the window when another app is activated, like Spotlight.
    pub hide_on_blur: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model: Model::Sonnet,
            effort: Effort::Low,
            session_timeout: Timeout::FiveMinutes,
            web_search: true,
            web_fetch: true,
            hotkey: Hotkey::OptionSpace,
            hide_on_blur: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        storage::load(FILE)
    }

    pub fn save(&self) -> std::io::Result<()> {
        storage::save(FILE, self)
    }

    /// Built-in tools to enable, for `--tools` and `--allowedTools`.
    pub fn tools(&self) -> Vec<&'static str> {
        [(self.web_search, "WebSearch"), (self.web_fetch, "WebFetch")]
            .into_iter()
            .filter_map(|(enabled, tool)| enabled.then_some(tool))
            .collect()
    }

    /// Whether switching from `self` to `other` needs a new `claude` process.
    pub fn changes_cli_flags(&self, other: &Settings) -> bool {
        self.model != other.model || self.effort != other.effort || self.tools() != other.tools()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"model":"opus"}"#).unwrap();
        assert_eq!(settings.model, Model::Opus);
        assert_eq!(settings.effort, Effort::Low);
    }

    #[test]
    fn tools_follow_toggles() {
        let mut settings = Settings::default();
        assert_eq!(settings.tools(), ["WebSearch", "WebFetch"]);
        settings.web_fetch = false;
        assert_eq!(settings.tools(), ["WebSearch"]);
    }

    #[test]
    fn hotkey_change_keeps_process() {
        let before = Settings::default();
        let after = Settings {
            hotkey: Hotkey::ControlSpace,
            ..before.clone()
        };
        assert!(!before.changes_cli_flags(&after));
    }
}
