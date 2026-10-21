//! `keymap/` — Vim-like key normalization, multi-key sequence handling (`gg`), and action resolution (SPEC-0300-0001).

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::config_toml::RawKeymapEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    MoveTop,
    MoveBottom,
    PageUp,
    PageDown,
    SelectToggle,
    SelectAll,
    ClearSelection,
    Open,
    GoParent,
    Exit,
    FocusPath,
    ToggleHidden,
    ToggleTree,
    Copy,
    Cut,
    Paste,
    Delete,
    DeletePermanent,
    Rename,
    NewFile,
    NewFolder,
    Search,
    SortMenu,
    History,
    HistoryJumpList,
    JumpAdd,
    ToggleSize,
    ToggleTime,
    ZipCreate,
    ZipExtract,
    ActionMenu,
    ConnectionManager,
    FuzzyFindFile,
    FuzzyLiveGrep,
    FuzzyJumpDir,
    OpenDefault,
    ExitAndCd,
}

impl ActionId {
    pub const ALL: &'static [ActionId] = &[
        ActionId::MoveUp,
        ActionId::MoveDown,
        ActionId::MoveLeft,
        ActionId::MoveRight,
        ActionId::MoveTop,
        ActionId::MoveBottom,
        ActionId::PageUp,
        ActionId::PageDown,
        ActionId::SelectToggle,
        ActionId::SelectAll,
        ActionId::ClearSelection,
        ActionId::Open,
        ActionId::GoParent,
        ActionId::Exit,
        ActionId::FocusPath,
        ActionId::ToggleHidden,
        ActionId::ToggleTree,
        ActionId::Copy,
        ActionId::Cut,
        ActionId::Paste,
        ActionId::Delete,
        ActionId::Rename,
        ActionId::NewFile,
        ActionId::NewFolder,
        ActionId::Search,
        ActionId::SortMenu,
        ActionId::History,
        ActionId::HistoryJumpList,
        ActionId::JumpAdd,
        ActionId::ToggleSize,
        ActionId::ToggleTime,
        ActionId::ZipCreate,
        ActionId::ZipExtract,
        ActionId::ActionMenu,
        ActionId::ConnectionManager,
        ActionId::FuzzyFindFile,
        ActionId::FuzzyLiveGrep,
        ActionId::FuzzyJumpDir,
        ActionId::OpenDefault,
        ActionId::ExitAndCd,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ActionId::MoveUp => "move_up",
            ActionId::MoveDown => "move_down",
            ActionId::MoveLeft => "move_left",
            ActionId::MoveRight => "move_right",
            ActionId::MoveTop => "move_top",
            ActionId::MoveBottom => "move_bottom",
            ActionId::PageUp => "page_up",
            ActionId::PageDown => "page_down",
            ActionId::SelectToggle => "select_toggle",
            ActionId::SelectAll => "select_all",
            ActionId::ClearSelection => "clear_selection",
            ActionId::Open => "open",
            ActionId::GoParent => "go_parent",
            ActionId::Exit => "exit",
            ActionId::ExitAndCd => "exit_and_cd",
            ActionId::FocusPath => "focus_path",
            ActionId::ToggleHidden => "toggle_hidden",
            ActionId::ToggleTree => "toggle_tree",
            ActionId::Copy => "copy",
            ActionId::Cut => "cut",
            ActionId::Paste => "paste",
            ActionId::Delete => "delete",
            ActionId::DeletePermanent => "delete_permanent",
            ActionId::Rename => "rename",
            ActionId::NewFile => "new_file",
            ActionId::NewFolder => "new_folder",
            ActionId::Search => "search",
            ActionId::SortMenu => "sort_menu",
            ActionId::History => "history",
            ActionId::HistoryJumpList => "history_jump_list",
            ActionId::JumpAdd => "jump_add",
            ActionId::ToggleSize => "toggle_size",
            ActionId::ToggleTime => "toggle_time",
            ActionId::ZipCreate => "zip_create",
            ActionId::ZipExtract => "zip_extract",
            ActionId::ActionMenu => "action_menu",
            ActionId::ConnectionManager => "connection_manager",
            ActionId::FuzzyFindFile => "fuzzy_find_file",
            ActionId::FuzzyLiveGrep => "fuzzy_live_grep",
            ActionId::FuzzyJumpDir => "fuzzy_jump_dir",
            ActionId::OpenDefault => "open_default",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
        if let Some(a) = Self::ALL.iter().copied().find(|a| a.as_str() == normalized) {
            return Some(a);
        }
        match normalized.as_str() {
            "quit" => Some(ActionId::Exit),
            "quit_and_cd" | "quit_cd" | "exit_cd" => Some(ActionId::ExitAndCd),
            "up" => Some(ActionId::MoveUp),
            "down" => Some(ActionId::MoveDown),
            "left" => Some(ActionId::MoveLeft),
            "right" => Some(ActionId::MoveRight),
            "top" => Some(ActionId::MoveTop),
            "bottom" => Some(ActionId::MoveBottom),
            "find" => Some(ActionId::Search),
            "jump" => Some(ActionId::HistoryJumpList),
            "zip" => Some(ActionId::ZipCreate),
            "unzip" => Some(ActionId::ZipExtract),
            _ => None,
        }
    }

    pub fn default_binding_spec(self) -> &'static str {
        match self {
            ActionId::MoveUp => "K,ArrowUp",
            ActionId::MoveDown => "J,ArrowDown",
            ActionId::MoveLeft => "H,ArrowLeft",
            ActionId::MoveRight => "L,ArrowRight",
            ActionId::MoveTop => "Home",
            ActionId::MoveBottom => "Shift+G,End",
            ActionId::PageUp => "Ctrl+U,PageUp",
            ActionId::PageDown => "Ctrl+D,PageDown",
            ActionId::SelectToggle => "Space,V",
            ActionId::SelectAll => "Shift+V,Ctrl+A",
            ActionId::ClearSelection => "Escape",
            ActionId::Open => "Enter",
            ActionId::GoParent => "Backspace,Minus",
            ActionId::Exit => "Q",
            ActionId::ExitAndCd => "Ctrl+Q",
            ActionId::FocusPath => "Ctrl+L",
            ActionId::ToggleHidden => "Dot,Ctrl+H",
            ActionId::ToggleTree => "T,Ctrl+Shift+B",
            ActionId::Copy => "Y,Ctrl+C",
            ActionId::Cut => "X,Ctrl+X",
            ActionId::Paste => "P,Ctrl+V",
            ActionId::Delete => "Delete,D",
            ActionId::DeletePermanent => "Shift+Delete,Shift+D",
            ActionId::Rename => "R,F2",
            ActionId::NewFile => "A,Ctrl+N",
            ActionId::NewFolder => "Shift+A,Ctrl+Shift+N",
            ActionId::Search => "/",
            ActionId::SortMenu => "S",
            ActionId::History => "Ctrl+Y",
            ActionId::HistoryJumpList => "Ctrl+J",
            ActionId::JumpAdd => "Ctrl+Shift+J,M",
            ActionId::ToggleSize => "Ctrl+Shift+S",
            ActionId::ToggleTime => "Ctrl+Shift+T",
            ActionId::ZipCreate => "Z,Ctrl+Alt+Z",
            ActionId::ZipExtract => "Shift+Z,Ctrl+Alt+X",
            ActionId::ActionMenu => "Semicolon,F9",
            ActionId::ConnectionManager => "Ctrl+S",
            ActionId::FuzzyFindFile => "Ctrl+P,Ctrl+F",
            ActionId::FuzzyLiveGrep => "Ctrl+G,Shift+F",
            ActionId::FuzzyJumpDir => "Alt+C,F",
            ActionId::OpenDefault => "O,Shift+Enter",
        }
    }
}

/// Normalized key chord representation (e.g. `Ctrl+Alt+V`, `Shift+G`, `J`, `ArrowDown`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl KeyChord {
    pub fn parse_single(spec: &str) -> Result<Self, String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err("empty key spec".to_string());
        }

        let mut remaining = trimmed;
        let mut ctrl = false;
        let mut alt = false;
        let mut shift = false;

        loop {
            let lower = remaining.to_ascii_lowercase();
            if lower.starts_with("ctrl+") || lower.starts_with("ctrl-") {
                ctrl = true;
                remaining = remaining[5..].trim_start();
            } else if lower.starts_with("control+") || lower.starts_with("control-") {
                ctrl = true;
                remaining = remaining[8..].trim_start();
            } else if lower.starts_with("alt+") || lower.starts_with("alt-") {
                alt = true;
                remaining = remaining[4..].trim_start();
            } else if lower.starts_with("shift+") || lower.starts_with("shift-") {
                shift = true;
                remaining = remaining[6..].trim_start();
            } else if (lower.starts_with("c+") || lower.starts_with("c-")) && remaining.len() > 2 {
                ctrl = true;
                remaining = remaining[2..].trim_start();
            } else if (lower.starts_with("a+") || lower.starts_with("a-")) && remaining.len() > 2 {
                alt = true;
                remaining = remaining[2..].trim_start();
            } else if (lower.starts_with("s+") || lower.starts_with("s-")) && remaining.len() > 2 {
                shift = true;
                remaining = remaining[2..].trim_start();
            } else {
                break;
            }
        }

        if remaining.is_empty() {
            return Err("missing key name after modifier".to_string());
        }

        let key = normalize_key_name(remaining)?;
        Ok(Self { ctrl, alt, shift, key })
    }

    pub fn parse_list(spec: &str) -> Result<Vec<Self>, String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for token in trimmed.split(',') {
            out.push(Self::parse_single(token)?);
        }
        Ok(out)
    }

    pub fn from_crossterm(ev: KeyEvent) -> Option<Self> {
        let mut ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
        let mut alt = ev.modifiers.contains(KeyModifiers::ALT);
        let mut shift = ev.modifiers.contains(KeyModifiers::SHIFT);

        // Windows Ctrl+Alt (AltGr) safety
        let _ = (&mut ctrl, &mut alt);

        let key = match ev.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char('/') => "/".to_string(),
            KeyCode::Char('.') => "Dot".to_string(),
            KeyCode::Char('-') => "Minus".to_string(),
            KeyCode::Char(';') => "Semicolon".to_string(),
            KeyCode::Char(',') => "Comma".to_string(),
            KeyCode::Char('+') => "Plus".to_string(),
            KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                if c.is_ascii_uppercase() {
                    shift = true;
                }
                c.to_ascii_uppercase().to_string()
            }
            KeyCode::Char(c) if c.is_ascii_digit() => c.to_string(),
            KeyCode::Up => "ArrowUp".to_string(),
            KeyCode::Down => "ArrowDown".to_string(),
            KeyCode::Left => "ArrowLeft".to_string(),
            KeyCode::Right => "ArrowRight".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Escape".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => {
                shift = true;
                "Tab".to_string()
            }
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::F(n @ 1..=12) => format!("F{n}"),
            _ => return None,
        };

        Some(Self { ctrl, alt, shift, key })
    }

    pub fn display_label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        let key_disp = match self.key.as_str() {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "ArrowLeft" => "←",
            "ArrowRight" => "→",
            "Escape" => "Esc",
            "Dot" => ".",
            "Minus" => "-",
            "Semicolon" => ";",
            "Comma" => ",",
            "Plus" => "+",
            other => other,
        };
        parts.push(key_disp.to_string());
        parts.join("+")
    }
}

fn normalize_key_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty key name".to_string());
    }
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next().unwrap();
        if ch.is_ascii_alphanumeric() {
            return Ok(ch.to_ascii_uppercase().to_string());
        }
        match ch {
            '/' => return Ok("/".to_string()),
            '.' => return Ok("Dot".to_string()),
            '-' => return Ok("Minus".to_string()),
            ';' => return Ok("Semicolon".to_string()),
            ',' => return Ok("Comma".to_string()),
            '+' => return Ok("Plus".to_string()),
            _ => {}
        }
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "arrowup" | "up" => Ok("ArrowUp".to_string()),
        "arrowdown" | "down" => Ok("ArrowDown".to_string()),
        "arrowleft" | "left" => Ok("ArrowLeft".to_string()),
        "arrowright" | "right" => Ok("ArrowRight".to_string()),
        "enter" | "return" | "ret" => Ok("Enter".to_string()),
        "escape" | "esc" => Ok("Escape".to_string()),
        "delete" | "del" => Ok("Delete".to_string()),
        "backspace" | "bs" => Ok("Backspace".to_string()),
        "tab" => Ok("Tab".to_string()),
        "space" | "spc" => Ok("Space".to_string()),
        "home" => Ok("Home".to_string()),
        "end" => Ok("End".to_string()),
        "pageup" | "pgup" => Ok("PageUp".to_string()),
        "pagedown" | "pgdn" => Ok("PageDown".to_string()),
        "comma" => Ok("Comma".to_string()),
        "plus" => Ok("Plus".to_string()),
        "dot" | "period" => Ok("Dot".to_string()),
        "minus" | "hyphen" => Ok("Minus".to_string()),
        "semicolon" | "semi" => Ok("Semicolon".to_string()),
        "f1" => Ok("F1".to_string()),
        "f2" => Ok("F2".to_string()),
        "f3" => Ok("F3".to_string()),
        "f4" => Ok("F4".to_string()),
        "f5" => Ok("F5".to_string()),
        "f6" => Ok("F6".to_string()),
        "f7" => Ok("F7".to_string()),
        "f8" => Ok("F8".to_string()),
        "f9" => Ok("F9".to_string()),
        "f10" => Ok("F10".to_string()),
        "f11" => Ok("F11".to_string()),
        "f12" => Ok("F12".to_string()),
        _ => Err(format!("unsupported key name: {raw}")),
    }
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<ActionId, Vec<KeyChord>>,
    chord_to_action: HashMap<KeyChord, ActionId>,
    /// Tracks whether a single `g` was pressed (`gg` -> `MoveTop`).
    pub pending_g: bool,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_custom(&[]).0
    }
}

impl Keymap {
    /// Builds the active keymap from `config.toml`'s `[input_keymap_custom]` entries,
    /// returning `(Keymap, rejected_action_ids)`.
    pub fn from_custom(entries: &[RawKeymapEntry]) -> (Self, Vec<String>) {
        let mut bindings: HashMap<ActionId, Vec<KeyChord>> = HashMap::new();
        for &action in ActionId::ALL {
            if let Ok(chords) = KeyChord::parse_list(action.default_binding_spec()) {
                bindings.insert(action, chords);
            }
        }

        let mut rejected = Vec::new();
        for entry in entries {
            let Some(action) = ActionId::from_name(&entry.action_id) else {
                rejected.push(entry.action_id.clone());
                continue;
            };
            let Some(ref val) = entry.value else {
                rejected.push(entry.action_id.clone());
                continue;
            };
            match KeyChord::parse_list(val) {
                Ok(chords) => {
                    // Delete cannot have Shift in custom binding (Shift+Delete is permanent delete)
                    if action == ActionId::Delete && chords.iter().any(|c| c.shift) {
                        rejected.push(entry.action_id.clone());
                        continue;
                    }
                    bindings.insert(action, chords);
                }
                Err(_) => rejected.push(entry.action_id.clone()),
            }
        }

        // Derive Shift+Delete / Shift+D from active Delete bindings
        if let Some(del_chords) = bindings.get(&ActionId::Delete).cloned() {
            let perm_chords: Vec<KeyChord> = del_chords
                .into_iter()
                .map(|mut c| {
                    c.shift = true;
                    c
                })
                .collect();
            bindings.insert(ActionId::DeletePermanent, perm_chords);
        }

        let mut chord_to_action = HashMap::new();
        for &action in ActionId::ALL.iter().chain(&[ActionId::DeletePermanent]) {
            if let Some(chords) = bindings.get(&action) {
                for chord in chords {
                    chord_to_action.entry(chord.clone()).or_insert(action);
                }
            }
        }

        (
            Self {
                bindings,
                chord_to_action,
                pending_g: false,
            },
            rejected,
        )
    }

    pub fn resolve(&mut self, chord: &KeyChord) -> Option<ActionId> {
        // Handle `gg` sequence for MoveTop
        if !chord.ctrl && !chord.alt && !chord.shift && chord.key == "G" {
            if self.pending_g {
                self.pending_g = false;
                return Some(ActionId::MoveTop);
            } else {
                self.pending_g = true;
                return None;
            }
        }
        self.pending_g = false;
        self.chord_to_action.get(chord).copied()
    }

    pub fn primary_label(&self, action: ActionId) -> String {
        self.bindings
            .get(&action)
            .and_then(|v| v.first())
            .map(KeyChord::display_label)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_keys_resolve_by_default() {
        let mut km = Keymap::default();
        let j = KeyChord::parse_single("J").unwrap();
        let k = KeyChord::parse_single("K").unwrap();
        let y = KeyChord::parse_single("Y").unwrap();
        let p = KeyChord::parse_single("P").unwrap();
        let big_g = KeyChord::parse_single("Shift+G").unwrap();

        assert_eq!(km.resolve(&j), Some(ActionId::MoveDown));
        assert_eq!(km.resolve(&k), Some(ActionId::MoveUp));
        assert_eq!(km.resolve(&y), Some(ActionId::Copy));
        assert_eq!(km.resolve(&p), Some(ActionId::Paste));
        assert_eq!(km.resolve(&big_g), Some(ActionId::MoveBottom));
    }

    #[test]
    fn gg_sequence_resolves_to_move_top() {
        let mut km = Keymap::default();
        let g = KeyChord {
            ctrl: false,
            alt: false,
            shift: false,
            key: "G".to_string(),
        };
        assert_eq!(km.resolve(&g), None);
        assert!(km.pending_g);
        assert_eq!(km.resolve(&g), Some(ActionId::MoveTop));
        assert!(!km.pending_g);
    }

    #[test]
    fn custom_keymap_overrides_and_derives_shift_delete() {
        let custom = vec![RawKeymapEntry {
            action_id: "copy".to_string(),
            value: Some("Ctrl+Shift+C".to_string()),
        }];
        let (mut km, rejected) = Keymap::from_custom(&custom);
        assert!(rejected.is_empty());
        assert_eq!(
            km.resolve(&KeyChord::parse_single("Ctrl+Shift+C").unwrap()),
            Some(ActionId::Copy)
        );
        assert_eq!(
            km.resolve(&KeyChord::parse_single("Shift+Delete").unwrap()),
            Some(ActionId::DeletePermanent)
        );
    }

    #[test]
    fn parses_key_chord_variations_and_hyphens() {
        let c1 = KeyChord::parse_single("ctrl+p").unwrap();
        let c2 = KeyChord::parse_single("Ctrl-P").unwrap();
        let c3 = KeyChord::parse_single("c-p").unwrap();
        let c4 = KeyChord::parse_single("Control+P").unwrap();

        assert_eq!(c1, c2);
        assert_eq!(c2, c3);
        assert_eq!(c3, c4);
        assert!(c1.ctrl);
        assert_eq!(c1.key, "P");

        // Key is minus
        let minus_chord = KeyChord::parse_single("Ctrl+-").unwrap();
        assert!(minus_chord.ctrl);
        assert_eq!(minus_chord.key, "Minus");

        let minus_hyphen = KeyChord::parse_single("ctrl--").unwrap();
        assert_eq!(minus_chord, minus_hyphen);
    }

    #[test]
    fn action_id_aliases_and_case_insensitivity() {
        assert_eq!(ActionId::from_name("open_default"), Some(ActionId::OpenDefault));
        assert_eq!(ActionId::from_name("open-default"), Some(ActionId::OpenDefault));
        assert_eq!(ActionId::from_name("OPEN_DEFAULT"), Some(ActionId::OpenDefault));
        assert_eq!(ActionId::from_name("quit"), Some(ActionId::Exit));
        assert_eq!(ActionId::from_name("exit-and-cd"), Some(ActionId::ExitAndCd));
    }
}
