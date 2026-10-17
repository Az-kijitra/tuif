//! `ui_theme` of `config.toml` (SPEC-0300-0017 R-917 / R-917-3 / R-917-4; SPEC-0200-0001 §3.11.3).
//! Pure types and one pure function; no `tauri` reference (the mapping to `tauri::Theme` lives in
//! `shared/window_theme.rs`, SHOULD-181). `config/config_toml.rs` hands the value back RAW; judging it is
//! `interpret_ui_theme`'s job. Only "which setting / is it invalid" is decided here: the OS axis (following
//! the OS) is left to the platform (tao / WebView2) and CSS `prefers-color-scheme` (§3.11.8, H-2).

/// The top-level `ui_theme` key of `config.toml` as read, before any judgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiThemeRaw {
    /// The key is absent (also: no file, or the whole file was unreadable).
    Missing,
    /// A TOML string, verbatim (case and surrounding whitespace kept).
    Str(String),
    /// Present but not a string (number, boolean, array, table, datetime).
    WrongType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSetting {
    /// Follow the OS (the default).
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeResolution {
    pub setting: ThemeSetting,
    /// R-917-4: the value was present but not one of the three words, so a notice is due.
    pub invalid: bool,
}

/// R-917-3: exact, case-sensitive match of `"light"` / `"dark"` / `"system"`; no trimming. Anything else
/// that is present follows the OS and is reported as invalid.
pub fn interpret_ui_theme(raw: &UiThemeRaw) -> ThemeResolution {
    let (setting, invalid) = match raw {
        UiThemeRaw::Missing => (ThemeSetting::System, false),
        UiThemeRaw::Str(s) => match s.as_str() {
            "light" => (ThemeSetting::Light, false),
            "dark" => (ThemeSetting::Dark, false),
            "system" => (ThemeSetting::System, false),
            _ => (ThemeSetting::System, true),
        },
        UiThemeRaw::WrongType => (ThemeSetting::System, true),
    };
    ThemeResolution { setting, invalid }
}
