//! Syntax highlighting integration via `bat --color=always --paging=never --style=plain`
//! and ANSI SGR escape sequence parser for the built-in Ratatui viewer.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use crate::external_apps::resolve_executable;

/// Highlights `decoded_content` using `bat.exe` if available on `PATH`, falling back
/// to plain text lines when `bat.exe` is not installed or fails.
pub fn highlight_with_bat(path: &Path, decoded_content: &str) -> Vec<String> {
    if let Some(lines) = try_run_bat(path, decoded_content) {
        return lines;
    }
    decoded_content
        .lines()
        .map(|l| l.replace('\t', "    "))
        .collect()
}

fn try_run_bat(path: &Path, decoded_content: &str) -> Option<Vec<String>> {
    let bat_exe = resolve_executable("bat.exe")?;
    let file_name = path.to_string_lossy();
    let mut child = Command::new(bat_exe)
        .args([
            "--color=always",
            "--paging=never",
            "--style=plain",
            "--tabs=4",
            "--file-name",
            file_name.as_ref(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(decoded_content.as_bytes());
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.lines().map(|l| l.replace('\t', "    ")).collect())
}

fn resolve_leaf_executable() -> Option< std::path::PathBuf > {
    if let Some(exe) = resolve_executable("leaf.exe") {
        return Some(exe);
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let candidate = Path::new(&local_app_data)
            .join("Programs")
            .join("leaf")
            .join("leaf.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Renders Markdown `decoded_content` using `leaf --inline ansi:<width>` if `leaf.exe` is available.
pub fn render_markdown_with_leaf(decoded_content: &str, width: usize) -> Option<Vec<String>> {
    let leaf_exe = resolve_leaf_executable()?;
    let clamped_width = width.max(20);
    let inline_spec = format!("ansi:{clamped_width}");
    let mut child = Command::new(leaf_exe)
        .args(["--inline", &inline_spec])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(decoded_content.as_bytes());
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    Some(text.lines().map(|l| l.replace('\t', "    ")).collect())
}

/// Parses a line containing ANSI SGR sequences (`\x1b[...m`) into styled Ratatui `Span`s,
/// skipping the first `scroll_x` visible characters while preserving the active `Style`.
pub fn parse_ansi_line_with_scroll(line: &str, scroll_x: usize) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut current_style = Style::default();
    let mut current_buf = String::new();
    let mut visible_char_idx = 0usize;

    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b' && i + 1 < chars.len() && chars[i + 1] == '[' {
            // Flush current buffer if non-empty before changing style
            if !current_buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut current_buf), current_style));
            }
            i += 2;
            let start = i;
            while i < chars.len() && !chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            if i < chars.len() {
                let final_byte = chars[i];
                let params: String = chars[start..i].iter().collect();
                if final_byte == 'm' {
                    apply_sgr_params(&mut current_style, &params);
                }
                i += 1;
            }
            continue;
        }

        if visible_char_idx >= scroll_x {
            current_buf.push(chars[i]);
        }
        visible_char_idx += 1;
        i += 1;
    }

    if !current_buf.is_empty() {
        spans.push(Span::styled(current_buf, current_style));
    }

    spans
}

fn apply_sgr_params(style: &mut Style, params: &str) {
    if params.is_empty() {
        *style = Style::default();
        return;
    }

    let codes: Vec<u16> = params
        .split(';')
        .map(|s| s.parse::<u16>().unwrap_or(0))
        .collect();

    let mut idx = 0;
    while idx < codes.len() {
        match codes[idx] {
            0 => *style = Style::default(),
            1 => *style = style.add_modifier(Modifier::BOLD),
            2 => *style = style.add_modifier(Modifier::DIM),
            3 => *style = style.add_modifier(Modifier::ITALIC),
            4 => *style = style.add_modifier(Modifier::UNDERLINED),
            7 => *style = style.add_modifier(Modifier::REVERSED),
            9 => *style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = style.remove_modifier(Modifier::ITALIC),
            24 => *style = style.remove_modifier(Modifier::UNDERLINED),
            27 => *style = style.remove_modifier(Modifier::REVERSED),
            29 => *style = style.remove_modifier(Modifier::CROSSED_OUT),
            c @ 30..=37 => style.fg = Some(standard_ansi_color(c - 30, false)),
            38 => {
                if idx + 2 < codes.len() && codes[idx + 1] == 5 {
                    style.fg = Some(Color::Indexed(codes[idx + 2] as u8));
                    idx += 2;
                } else if idx + 4 < codes.len() && codes[idx + 1] == 2 {
                    style.fg = Some(Color::Rgb(
                        codes[idx + 2] as u8,
                        codes[idx + 3] as u8,
                        codes[idx + 4] as u8,
                    ));
                    idx += 4;
                }
            }
            39 => style.fg = None,
            c @ 40..=47 => style.bg = Some(standard_ansi_color(c - 40, false)),
            48 => {
                if idx + 2 < codes.len() && codes[idx + 1] == 5 {
                    style.bg = Some(Color::Indexed(codes[idx + 2] as u8));
                    idx += 2;
                } else if idx + 4 < codes.len() && codes[idx + 1] == 2 {
                    style.bg = Some(Color::Rgb(
                        codes[idx + 2] as u8,
                        codes[idx + 3] as u8,
                        codes[idx + 4] as u8,
                    ));
                    idx += 4;
                }
            }
            49 => style.bg = None,
            c @ 90..=97 => style.fg = Some(standard_ansi_color(c - 90, true)),
            c @ 100..=107 => style.bg = Some(standard_ansi_color(c - 100, true)),
            _ => {}
        }
        idx += 1;
    }
}

fn standard_ansi_color(offset: u16, bright: bool) -> Color {
    match (offset, bright) {
        (0, false) => Color::Black,
        (1, false) => Color::Red,
        (2, false) => Color::Green,
        (3, false) => Color::Yellow,
        (4, false) => Color::Blue,
        (5, false) => Color::Magenta,
        (6, false) => Color::Cyan,
        (7, false) => Color::Gray,
        (0, true) => Color::DarkGray,
        (1, true) => Color::LightRed,
        (2, true) => Color::LightGreen,
        (3, true) => Color::LightYellow,
        (4, true) => Color::LightBlue,
        (5, true) => Color::LightMagenta,
        (6, true) => Color::LightCyan,
        (_, true) => Color::White,
        _ => Color::Reset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ansi_truecolor_and_scroll_preserves_style() {
        let line = "\x1b[1;38;2;255;128;0mfn\x1b[0m \x1b[32mmain\x1b[0m()";
        let spans = parse_ansi_line_with_scroll(line, 0);
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].content, "fn");
        assert_eq!(spans[0].style.fg, Some(Color::Rgb(255, 128, 0)));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[2].content, "main");
        assert_eq!(spans[2].style.fg, Some(Color::Green));

        // Horizontal scroll by 3 chars skips "fn " and starts at "main" with Green style intact
        let scrolled = parse_ansi_line_with_scroll(line, 3);
        assert_eq!(scrolled[0].content, "main");
        assert_eq!(scrolled[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn parse_leaf_inline_code_and_strikethrough_sgr() {
        let line = "\x1b[38;2;220;150;118;48;2;38;32;31mtuif\x1b[0m \x1b[9mold\x1b[0m";
        let spans = parse_ansi_line_with_scroll(line, 0);
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "tuif");
        assert_eq!(spans[0].style.fg, Some(Color::Rgb(220, 150, 118)));
        assert_eq!(spans[0].style.bg, Some(Color::Rgb(38, 32, 31)));
        assert_eq!(spans[2].content, "old");
        assert!(spans[2].style.add_modifier.contains(Modifier::CROSSED_OUT));
    }
}
