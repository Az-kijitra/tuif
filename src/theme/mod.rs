//! `theme/` — UI colors, themes, preset themes and color parsing.

use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// Custom color overrides configurable in `[colors]` of `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CustomColors {
    pub bg: Option<String>,
    pub border: Option<String>,
    pub border_inactive: Option<String>,
    pub cursor_fg: Option<String>,
    pub cursor_bg: Option<String>,
    pub folder: Option<String>,
    pub file: Option<String>,
    pub symlink: Option<String>,
    pub executable: Option<String>,
    pub selected: Option<String>,
    pub status_bar_fg: Option<String>,
    pub status_bar_bg: Option<String>,
    pub menu_bar_active_fg: Option<String>,
    pub menu_bar_active_bg: Option<String>,
    pub tree_current: Option<String>,
    pub tree_cursor: Option<String>,
}

/// Resolved concrete Ratatui colors used throughout the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub border: Color,
    pub border_inactive: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
    pub folder: Color,
    pub file: Color,
    pub symlink: Color,
    pub executable: Color,
    pub selected: Color,
    pub status_bar_fg: Color,
    pub status_bar_bg: Color,
    pub menu_bar_active_fg: Color,
    pub menu_bar_active_bg: Color,
    pub tree_current: Color,
    pub tree_cursor: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self::preset_default()
    }
}

impl Theme {
    pub fn preset_default() -> Self {
        Self {
            bg: Color::Reset,
            border: Color::Cyan,
            border_inactive: Color::DarkGray,
            cursor_fg: Color::Black,
            cursor_bg: Color::Cyan,
            folder: Color::Yellow,
            file: Color::Reset,
            symlink: Color::LightMagenta,
            executable: Color::LightGreen,
            selected: Color::LightBlue,
            status_bar_fg: Color::White,
            status_bar_bg: Color::DarkGray,
            menu_bar_active_fg: Color::Black,
            menu_bar_active_bg: Color::Cyan,
            tree_current: Color::Cyan,
            tree_cursor: Color::Yellow,
        }
    }

    pub fn preset_nord() -> Self {
        Self {
            bg: Color::Rgb(46, 52, 64),                 // #2E3440 (Nord Dark Slate)
            border: Color::Rgb(136, 192, 208),          // #88C0D0 (Frost Blue)
            border_inactive: Color::Rgb(76, 86, 106),   // #4C566A
            cursor_fg: Color::Rgb(46, 52, 64),          // #2E3440
            cursor_bg: Color::Rgb(143, 188, 187),       // #8FBCBB (Teal Frost)
            folder: Color::Rgb(129, 161, 193),          // #81A1C1 (Glacier Blue)
            file: Color::Rgb(236, 239, 244),            // #ECEFF4 (Snow Storm White)
            symlink: Color::Rgb(180, 142, 173),         // #B48EAD
            executable: Color::Rgb(163, 190, 140),      // #A3BE8C
            selected: Color::Rgb(235, 203, 139),        // #EBCB8B (Yellow)
            status_bar_fg: Color::Rgb(229, 233, 240),   // #E5E9F0
            status_bar_bg: Color::Rgb(59, 66, 82),      // #3B4252
            menu_bar_active_fg: Color::Rgb(46, 52, 64),
            menu_bar_active_bg: Color::Rgb(136, 192, 208),
            tree_current: Color::Rgb(136, 192, 208),
            tree_cursor: Color::Rgb(235, 203, 139),
        }
    }

    pub fn preset_monokai() -> Self {
        Self {
            bg: Color::Rgb(39, 40, 34),                 // #272822 (Monokai Charcoal)
            border: Color::Rgb(249, 38, 114),           // #F92672 (Monokai Hot Pink!)
            border_inactive: Color::Rgb(117, 113, 94),  // #75715E
            cursor_fg: Color::Rgb(39, 40, 34),
            cursor_bg: Color::Rgb(166, 226, 46),        // #A6E22E (Lime Green)
            folder: Color::Rgb(253, 151, 31),           // #FD971F (Monokai Orange!)
            file: Color::Rgb(248, 248, 242),            // #F8F8F2
            symlink: Color::Rgb(174, 129, 255),         // #AE81FF (Purple)
            executable: Color::Rgb(166, 226, 46),       // #A6E22E
            selected: Color::Rgb(230, 219, 116),        // #E6DB74 (Yellow)
            status_bar_fg: Color::Rgb(248, 248, 242),
            status_bar_bg: Color::Rgb(62, 61, 50),      // #3E3D32
            menu_bar_active_fg: Color::Rgb(39, 40, 34),
            menu_bar_active_bg: Color::Rgb(249, 38, 114),
            tree_current: Color::Rgb(102, 217, 239),    // #66D9EF (Cyan)
            tree_cursor: Color::Rgb(230, 219, 116),
        }
    }

    pub fn preset_catppuccin() -> Self {
        // Catppuccin Mocha
        Self {
            bg: Color::Rgb(30, 30, 46),                 // #1E1E2E (Catppuccin Base)
            border: Color::Rgb(203, 166, 247),          // #CBA6F7 (Catppuccin Mauve/Lavender!)
            border_inactive: Color::Rgb(88, 91, 112),   // #585B70
            cursor_fg: Color::Rgb(17, 17, 27),          // #11111B
            cursor_bg: Color::Rgb(137, 180, 250),       // #89B4FA (Blue)
            folder: Color::Rgb(249, 226, 175),          // #F9E2AF (Yellow/Gold)
            file: Color::Rgb(205, 214, 244),            // #CDD6F4
            symlink: Color::Rgb(245, 194, 231),         // #F5C2E7 (Pink)
            executable: Color::Rgb(166, 227, 161),      // #A6E3A1 (Green)
            selected: Color::Rgb(250, 179, 135),        // #FAB387 (Peach)
            status_bar_fg: Color::Rgb(205, 214, 244),
            status_bar_bg: Color::Rgb(49, 50, 68),      // #313244
            menu_bar_active_fg: Color::Rgb(17, 17, 27),
            menu_bar_active_bg: Color::Rgb(203, 166, 247),
            tree_current: Color::Rgb(137, 220, 235),    // #89DCEB
            tree_cursor: Color::Rgb(249, 226, 175),
        }
    }

    pub fn preset_solarized_dark() -> Self {
        Self {
            bg: Color::Rgb(0, 43, 54),                  // #002B36 (Solarized base03)
            border: Color::Rgb(38, 139, 210),           // #268BD2 (Solarized Blue)
            border_inactive: Color::Rgb(7, 54, 66),     // #073642 (Solarized base02)
            cursor_fg: Color::Rgb(0, 43, 54),
            cursor_bg: Color::Rgb(42, 161, 152),        // #2AA198 (Solarized Cyan)
            folder: Color::Rgb(181, 137, 0),            // #B58900 (Solarized Yellow)
            file: Color::Rgb(131, 148, 150),            // #839496 (base0)
            symlink: Color::Rgb(211, 54, 130),          // #D33682 (Magenta)
            executable: Color::Rgb(133, 153, 0),        // #859900 (Green)
            selected: Color::Rgb(203, 75, 22),          // #CB4B16 (Orange)
            status_bar_fg: Color::Rgb(147, 161, 161),   // #93A1A1 (base1)
            status_bar_bg: Color::Rgb(7, 54, 66),       // #073642
            menu_bar_active_fg: Color::Rgb(0, 43, 54),
            menu_bar_active_bg: Color::Rgb(38, 139, 210),
            tree_current: Color::Rgb(42, 161, 152),
            tree_cursor: Color::Rgb(181, 137, 0),
        }
    }

    pub fn preset_light() -> Self {
        Self {
            bg: Color::Rgb(245, 245, 245),              // #F5F5F5 (Light Gray)
            border: Color::Rgb(0, 102, 204),            // #0066CC (Blue)
            border_inactive: Color::Rgb(180, 180, 180), // #B4B4B4
            cursor_fg: Color::White,
            cursor_bg: Color::Rgb(0, 120, 215),         // #0078D7
            folder: Color::Rgb(217, 119, 6),            // #D97706 (Amber/Orange)
            file: Color::Rgb(31, 41, 55),               // #1F2937 (Dark Charcoal)
            symlink: Color::Rgb(124, 58, 237),          // #7C3AED
            executable: Color::Rgb(22, 163, 74),        // #16A34A
            selected: Color::Rgb(234, 88, 12),          // #EA580C
            status_bar_fg: Color::Rgb(55, 65, 81),
            status_bar_bg: Color::Rgb(229, 231, 235),   // #E5E7EB
            menu_bar_active_fg: Color::White,
            menu_bar_active_bg: Color::Rgb(0, 120, 215),
            tree_current: Color::Rgb(0, 102, 204),
            tree_cursor: Color::Rgb(217, 119, 6),
        }
    }

    pub fn from_preset(name: &str) -> Self {
        match name.to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "nord" => Self::preset_nord(),
            "monokai" => Self::preset_monokai(),
            "catppuccin" | "catppuccinmocha" => Self::preset_catppuccin(),
            "solarizeddark" | "solarized" => Self::preset_solarized_dark(),
            "light" => Self::preset_light(),
            _ => Self::preset_default(),
        }
    }

    pub fn apply_overrides(&mut self, colors: &CustomColors) {
        if let Some(ref c) = colors.bg {
            if let Some(col) = parse_color(c) { self.bg = col; }
        }
        if let Some(ref c) = colors.border {
            if let Some(col) = parse_color(c) { self.border = col; }
        }
        if let Some(ref c) = colors.border_inactive {
            if let Some(col) = parse_color(c) { self.border_inactive = col; }
        }
        if let Some(ref c) = colors.cursor_fg {
            if let Some(col) = parse_color(c) { self.cursor_fg = col; }
        }
        if let Some(ref c) = colors.cursor_bg {
            if let Some(col) = parse_color(c) { self.cursor_bg = col; }
        }
        if let Some(ref c) = colors.folder {
            if let Some(col) = parse_color(c) { self.folder = col; }
        }
        if let Some(ref c) = colors.file {
            if let Some(col) = parse_color(c) { self.file = col; }
        }
        if let Some(ref c) = colors.symlink {
            if let Some(col) = parse_color(c) { self.symlink = col; }
        }
        if let Some(ref c) = colors.executable {
            if let Some(col) = parse_color(c) { self.executable = col; }
        }
        if let Some(ref c) = colors.selected {
            if let Some(col) = parse_color(c) { self.selected = col; }
        }
        if let Some(ref c) = colors.status_bar_fg {
            if let Some(col) = parse_color(c) { self.status_bar_fg = col; }
        }
        if let Some(ref c) = colors.status_bar_bg {
            if let Some(col) = parse_color(c) { self.status_bar_bg = col; }
        }
        if let Some(ref c) = colors.menu_bar_active_fg {
            if let Some(col) = parse_color(c) { self.menu_bar_active_fg = col; }
        }
        if let Some(ref c) = colors.menu_bar_active_bg {
            if let Some(col) = parse_color(c) { self.menu_bar_active_bg = col; }
        }
        if let Some(ref c) = colors.tree_current {
            if let Some(col) = parse_color(c) { self.tree_current = col; }
        }
        if let Some(ref c) = colors.tree_cursor {
            if let Some(col) = parse_color(c) { self.tree_cursor = col; }
        }
    }
}

/// Parses a color string. Supports standard names (e.g. "cyan", "red", "dark_gray"),
/// 256-color index numbers ("123"), and Hex codes ("#RRGGBB" / "#RGB").
pub fn parse_color(s: &str) -> Option<Color> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Hex color: #RRGGBB or #RGB
    if let Some(hex) = trimmed.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::Rgb(r, g, b));
        } else if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            return Some(Color::Rgb(r, g, b));
        }
        return None;
    }

    // Number (indexed ANSI color 0..255)
    if let Ok(idx) = trimmed.parse::<u8>() {
        return Some(Color::Indexed(idx));
    }

    // Standard named colors
    let normalized = trimmed.to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "reset" | "default" | "none" => Some(Color::Reset),
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" | "purple" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "dark_gray" | "dark_grey" | "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "light_red" | "lightred" => Some(Color::LightRed),
        "light_green" | "lightgreen" => Some(Color::LightGreen),
        "light_yellow" | "lightyellow" => Some(Color::LightYellow),
        "light_blue" | "lightblue" => Some(Color::LightBlue),
        "light_magenta" | "lightmagenta" | "light_purple" | "lightpurple" => Some(Color::LightMagenta),
        "light_cyan" | "lightcyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color_names() {
        assert_eq!(parse_color("cyan"), Some(Color::Cyan));
        assert_eq!(parse_color("CYAN"), Some(Color::Cyan));
        assert_eq!(parse_color("dark-gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("dark_gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("light_blue"), Some(Color::LightBlue));
        assert_eq!(parse_color("white"), Some(Color::White));
        assert_eq!(parse_color("reset"), Some(Color::Reset));
        assert_eq!(parse_color("unknown_xyz"), None);
    }

    #[test]
    fn test_parse_color_hex() {
        assert_eq!(parse_color("#ffffff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_color("#000000"), Some(Color::Rgb(0, 0, 0)));
        assert_eq!(parse_color("#1e1e2e"), Some(Color::Rgb(30, 30, 46)));
        assert_eq!(parse_color("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_color("#000"), Some(Color::Rgb(0, 0, 0)));
        assert_eq!(parse_color("#invalid"), None);
    }

    #[test]
    fn test_parse_color_indexed() {
        assert_eq!(parse_color("123"), Some(Color::Indexed(123)));
        assert_eq!(parse_color("255"), Some(Color::Indexed(255)));
        assert_eq!(parse_color("256"), None);
    }

    #[test]
    fn test_preset_and_overrides() {
        let mut theme = Theme::from_preset("nord");
        assert_eq!(theme.border, Color::Rgb(136, 192, 208));

        let overrides = CustomColors {
            border: Some("magenta".to_string()),
            cursor_bg: Some("#123456".to_string()),
            ..Default::default()
        };
        theme.apply_overrides(&overrides);
        assert_eq!(theme.border, Color::Magenta);
        assert_eq!(theme.cursor_bg, Color::Rgb(0x12, 0x34, 0x56));
        // Other fields remain Nord preset
        assert_eq!(theme.folder, Color::Rgb(129, 161, 193));
    }
}
