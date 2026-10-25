//! File and directory icon mapping with Nerd Font, Unicode emoji, and ASCII support.

use ratatui::style::Color;

use crate::config::config_toml::IconTheme;

/// Returns the icon glyph and recommended foreground color for a file or directory.
pub fn file_icon(name: &str, is_dir: bool, theme: IconTheme) -> (&'static str, Color) {
    match theme {
        IconTheme::None => ("", Color::Reset),
        IconTheme::Ascii => {
            if is_dir {
                ("[/]", Color::LightBlue)
            } else if is_archive_ext(name) {
                ("[Z]", Color::LightMagenta)
            } else if is_executable_ext(name) {
                ("*", Color::LightGreen)
            } else {
                ("-", Color::Reset)
            }
        }
        IconTheme::Unicode => {
            if is_dir {
                ("📁", Color::Reset)
            } else {
                unicode_file_icon(name)
            }
        }
        IconTheme::Nerd => {
            if is_dir {
                nerd_folder_icon(name)
            } else {
                nerd_file_icon(name)
            }
        }
    }
}

/// Returns the icon glyph and color for a tree pane node.
pub fn tree_icon(
    expanded: bool,
    is_remote: bool,
    is_drive: bool,
    theme: IconTheme,
) -> (&'static str, Color) {
    match theme {
        IconTheme::None => ("", Color::Reset),
        IconTheme::Ascii => {
            if is_drive {
                ("[D]", Color::LightCyan)
            } else if is_remote {
                ("[R]", Color::LightMagenta)
            } else if expanded {
                ("[-]", Color::LightBlue)
            } else {
                ("[+]", Color::LightBlue)
            }
        }
        IconTheme::Unicode => {
            if is_drive {
                ("🖴", Color::Reset)
            } else if is_remote {
                ("☁️", Color::Reset)
            } else if expanded {
                ("📂", Color::Reset)
            } else {
                ("📁", Color::Reset)
            }
        }
        IconTheme::Nerd => {
            if is_drive {
                ("\u{f0a0}", Color::LightCyan) //  (Disk)
            } else if is_remote {
                ("\u{f0c2}", Color::LightMagenta) //  (Cloud)
            } else if expanded {
                ("\u{f115}", Color::LightBlue) //  (Folder Open)
            } else {
                ("\u{f114}", Color::LightBlue) //  (Folder Closed)
            }
        }
    }
}

fn is_archive_ext(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".zip")
        || lower.ends_with(".tar")
        || lower.ends_with(".gz")
        || lower.ends_with(".tgz")
        || lower.ends_with(".7z")
        || lower.ends_with(".rar")
        || lower.ends_with(".bz2")
        || lower.ends_with(".xz")
}

fn is_executable_ext(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".exe")
        || lower.ends_with(".bat")
        || lower.ends_with(".cmd")
        || lower.ends_with(".ps1")
        || lower.ends_with(".msi")
}

fn nerd_folder_icon(name: &str) -> (&'static str, Color) {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        ".git" => ("\u{e5fb}", Color::LightRed),           // 
        ".github" => ("\u{e709}", Color::White),           // 
        ".vscode" => ("\u{e70c}", Color::LightBlue),       // 
        "node_modules" => ("\u{e718}", Color::LightGreen), // 
        "src" => ("\u{e5fc}", Color::LightCyan),           // 
        "target" | "dist" | "build" | "bin" | "out" => ("\u{e5fd}", Color::Yellow), // 
        "docs" | "doc" => ("\u{e5fe}", Color::LightBlue),  // 
        _ => {
            if name.starts_with('.') {
                ("\u{e5ff}", Color::DarkGray) // 
            } else {
                ("\u{f115}", Color::LightBlue) // 
            }
        }
    }
}

fn nerd_file_icon(name: &str) -> (&'static str, Color) {
    let lower = name.to_ascii_lowercase();

    // 1. Exact filename matches
    match lower.as_str() {
        "cargo.toml" | "cargo.lock" => return ("\u{e6b2}", Color::Rgb(222, 85, 38)), // 
        "package.json" | "package-lock.json" => return ("\u{e718}", Color::LightGreen), // 
        "tsconfig.json" => return ("\u{e628}", Color::LightBlue),                    // 
        "readme.md" | "readme.txt" | "readme" => return ("\u{f48a}", Color::LightCyan), // 
        "license" | "licence" | "copying" => return ("\u{f02d}", Color::Yellow),     // 
        ".gitignore" | ".gitmodules" | ".gitattributes" => return ("\u{e702}", Color::LightRed), // 
        "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" => {
            return ("\u{e7b0}", Color::LightBlue); // 
        }
        "makefile" => return ("\u{e673}", Color::Yellow), // 
        _ => {}
    }

    if lower.starts_with(".env") {
        return ("\u{e615}", Color::Yellow); // 
    }

    // 2. Extension matches
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    if let Some(ext) = ext.as_deref() {
        match ext {
            // Rust
            "rs" => ("\u{e7a8}", Color::Rgb(222, 85, 38)), // 
            // Python
            "py" | "pyw" | "ipynb" => ("\u{e606}", Color::Yellow), // 
            // Web / JS / TS
            "js" | "mjs" | "cjs" => ("\u{e60c}", Color::Yellow), // 
            "ts" | "mts" | "cts" => ("\u{e628}", Color::LightCyan), // 
            "jsx" | "tsx" => ("\u{e625}", Color::Cyan),          // 
            "html" | "htm" => ("\u{e60e}", Color::LightRed),     // 
            "css" | "scss" | "sass" | "less" => ("\u{e749}", Color::LightBlue), // 
            "vue" => ("\u{e6a0}", Color::LightGreen),            // 
            "svelte" => ("\u{e697}", Color::LightRed),           // 
            // Systems & Compiled
            "c" | "h" => ("\u{e61e}", Color::LightBlue), // 
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => ("\u{e61d}", Color::LightBlue), // 
            "go" => ("\u{e627}", Color::Cyan),           // 
            "cs" => ("\u{e648}", Color::Magenta),        // 󰌛
            "java" | "kt" | "kts" => ("\u{e738}", Color::LightRed), // 
            "swift" => ("\u{e755}", Color::LightRed),    // 
            "zig" => ("\u{e6a9}", Color::Yellow),        // 
            "lua" => ("\u{e620}", Color::LightBlue),     // 
            "rb" => ("\u{e739}", Color::LightRed),       // 
            "php" => ("\u{e608}", Color::LightMagenta),  // 
            // Shell & Scripting
            "sh" | "bash" | "zsh" => ("\u{e795}", Color::LightGreen), // 
            "ps1" | "psm1" | "psd1" => ("\u{e70f}", Color::LightBlue), // 
            "bat" | "cmd" => ("\u{e70f}", Color::Green),              // 
            // Config & Data
            "toml" => ("\u{e615}", Color::LightYellow), // 
            "json" | "jsonc" | "json5" => ("\u{e60b}", Color::Yellow), // 
            "yaml" | "yml" => ("\u{e6a8}", Color::LightMagenta), // 
            "xml" => ("\u{e619}", Color::LightYellow),  // 
            "ini" | "cfg" | "conf" => ("\u{e615}", Color::DarkGray), // 
            "sql" | "db" | "sqlite" | "sqlite3" => ("\u{e706}", Color::LightCyan), // 
            // Documents & Text
            "md" | "markdown" => ("\u{f48a}", Color::LightCyan), // 
            "txt" | "log" => ("\u{f15c}", Color::Gray),          // 
            "pdf" => ("\u{f1c1}", Color::LightRed),              // 
            "csv" | "tsv" => ("\u{f1c3}", Color::LightGreen),    // 
            "doc" | "docx" => ("\u{f1c2}", Color::LightBlue),    // 
            "xls" | "xlsx" => ("\u{f1c3}", Color::LightGreen),   // 
            "ppt" | "pptx" => ("\u{f1c4}", Color::LightRed),     // 
            // Archives
            "zip" | "tar" | "gz" | "tgz" | "7z" | "rar" | "bz2" | "xz" => {
                ("\u{f410}", Color::Magenta) // 
            }
            // Media
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "svg" => {
                ("\u{f1c5}", Color::LightMagenta) // 
            }
            "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" => ("\u{f1c7}", Color::LightCyan), // 
            "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" => ("\u{f1c8}", Color::LightYellow), // 
            // Executables & Binaries
            "exe" | "msi" => ("\u{e70f}", Color::LightGreen), // 
            "dll" | "so" | "dylib" | "bin" => ("\u{f013}", Color::DarkGray), // 
            // Fallback
            _ => ("\u{f15b}", Color::Gray), // 
        }
    } else if name.starts_with('.') {
        ("\u{e615}", Color::DarkGray) // 
    } else {
        ("\u{f15b}", Color::Gray) // 
    }
}

fn unicode_file_icon(name: &str) -> (&'static str, Color) {
    if is_archive_ext(name) {
        return ("📦", Color::Reset);
    }
    if is_executable_ext(name) {
        return ("⚡", Color::Reset);
    }

    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    if let Some(ext) = ext.as_deref() {
        match ext {
            "rs" => ("🦀", Color::Reset),
            "py" | "pyw" => ("🐍", Color::Reset),
            "js" | "ts" | "jsx" | "tsx" | "c" | "cpp" | "go" | "cs" | "java" | "kt" | "rb"
            | "php" | "html" | "css" | "sh" => ("📜", Color::Reset),
            "toml" | "json" | "yaml" | "yml" | "xml" | "ini" | "cfg" | "conf" => {
                ("⚙️", Color::Reset)
            }
            "md" | "markdown" | "txt" | "log" | "doc" | "docx" => ("📝", Color::Reset),
            "pdf" => ("📕", Color::Reset),
            "csv" | "tsv" | "xls" | "xlsx" => ("📊", Color::Reset),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" | "svg" => ("🖼️", Color::Reset),
            "mp3" | "wav" | "flac" | "aac" | "ogg" => ("🎵", Color::Reset),
            "mp4" | "mkv" | "avi" | "mov" | "wmv" => ("🎬", Color::Reset),
            _ => ("📄", Color::Reset),
        }
    } else {
        ("📄", Color::Reset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nerd_font_icons_and_colors() {
        let (icon, color) = file_icon("main.rs", false, IconTheme::Nerd);
        assert_eq!(icon, "\u{e7a8}");
        assert_eq!(color, Color::Rgb(222, 85, 38));

        let (icon, _) = file_icon("script.py", false, IconTheme::Nerd);
        assert_eq!(icon, "\u{e606}");

        let (icon, _) = file_icon("Cargo.toml", false, IconTheme::Nerd);
        assert_eq!(icon, "\u{e6b2}");

        let (icon, _) = file_icon("src", true, IconTheme::Nerd);
        assert_eq!(icon, "\u{e5fc}");

        let (icon, _) = file_icon("other_folder", true, IconTheme::Nerd);
        assert_eq!(icon, "\u{f115}");
    }

    #[test]
    fn test_unicode_and_ascii_icons() {
        let (icon, _) = file_icon("main.rs", false, IconTheme::Unicode);
        assert_eq!(icon, "🦀");

        let (icon, _) = file_icon("data.zip", false, IconTheme::Unicode);
        assert_eq!(icon, "📦");

        let (icon, _) = file_icon("test.exe", false, IconTheme::Ascii);
        assert_eq!(icon, "*");

        let (icon, _) = file_icon("folder", true, IconTheme::Ascii);
        assert_eq!(icon, "[/]");

        let (icon, _) = file_icon("main.rs", false, IconTheme::None);
        assert_eq!(icon, "");
    }

    #[test]
    fn test_tree_icons() {
        let (icon, _) = tree_icon(false, false, false, IconTheme::Nerd);
        assert_eq!(icon, "\u{f114}");

        let (icon, _) = tree_icon(true, false, false, IconTheme::Nerd);
        assert_eq!(icon, "\u{f115}");

        let (icon, _) = tree_icon(false, false, true, IconTheme::Nerd);
        assert_eq!(icon, "\u{f0a0}");

        let (icon, _) = tree_icon(false, true, false, IconTheme::Nerd);
        assert_eq!(icon, "\u{f0c2}");
    }
}
