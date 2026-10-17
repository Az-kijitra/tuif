//! `external_apps/` — CUI external tool registration (`pwsh`, `nvim`, `lazygit`, `bat`),
//! argument substitution (`{path}`, `{dir}`), executable resolution, and launching (SPEC-0300-0001 §3).

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::keymap::KeyChord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalAppEntry {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub args_for_file: Option<Vec<String>>,
    pub key: Option<Vec<KeyChord>>,
    pub key_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    File,
    Dir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    pub path: PathBuf,
    pub dir: PathBuf,
    pub kind: TargetKind,
}

impl LaunchTarget {
    pub fn from_entry(entry_path: &Path, is_dir: bool, current_dir: &Path) -> Self {
        if is_dir {
            Self {
                path: entry_path.to_path_buf(),
                dir: entry_path.to_path_buf(),
                kind: TargetKind::Dir,
            }
        } else {
            let dir = entry_path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| current_dir.to_path_buf());
            Self {
                path: entry_path.to_path_buf(),
                dir,
                kind: TargetKind::File,
            }
        }
    }

    pub fn from_current_dir(current_dir: &Path) -> Self {
        Self {
            path: current_dir.to_path_buf(),
            dir: current_dir.to_path_buf(),
            kind: TargetKind::Dir,
        }
    }
}

/// Default 4 CUI tools (`pwsh`, `nvim`, `lazygit`, `bat`) per SPEC-0100-0001 R-05 / SPEC-0300-0001 R-303.
pub fn default_external_apps() -> Vec<ExternalAppEntry> {
    vec![
        ExternalAppEntry {
            name: "Open PowerShell".to_string(),
            program: "pwsh.exe".to_string(),
            args: vec!["-NoLogo".to_string(), "-WorkingDirectory".to_string(), "{dir}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+C").ok(),
            key_label: "Ctrl+Alt+C".to_string(),
        },
        ExternalAppEntry {
            name: "Open in Neovim".to_string(),
            program: "nvim.exe".to_string(),
            args: vec!["{path}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+V").ok(),
            key_label: "Ctrl+Alt+V".to_string(),
        },
        ExternalAppEntry {
            name: "Open Lazygit".to_string(),
            program: "lazygit.exe".to_string(),
            args: vec!["-p".to_string(), "{dir}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+G").ok(),
            key_label: "Ctrl+Alt+G".to_string(),
        },
        ExternalAppEntry {
            name: "View with bat".to_string(),
            program: "bat.exe".to_string(),
            args: vec!["--paging=always".to_string(), "{path}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+B").ok(),
            key_label: "Ctrl+Alt+B".to_string(),
        },
    ]
}

/// Substitutes `{path}`, `{dir}`, `{{`, `}}` in a single argument template (SPEC-0300-0021 R-1520).
pub fn expand_arg(template: &str, target: &LaunchTarget) -> Result<String, String> {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                out.push('{');
                i += 2;
                continue;
            }
            if let Some(close_rel) = chars[i + 1..].iter().position(|&c| c == '}') {
                let close = i + 1 + close_rel;
                let token: String = chars[i + 1..close].iter().collect();
                match token.as_str() {
                    "path" => out.push_str(&target.path.to_string_lossy()),
                    "dir" => out.push_str(&target.dir.to_string_lossy()),
                    other => return Err(format!("unknown placeholder {{{other}}}")),
                }
                i = close + 1;
            } else {
                return Err("unclosed '{' in argument".to_string());
            }
        } else if chars[i] == '}' {
            if i + 1 < chars.len() && chars[i + 1] == '}' {
                out.push('}');
                i += 2;
            } else {
                return Err("unmatched '}' in argument".to_string());
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Ok(out)
}

pub fn build_args(app: &ExternalAppEntry, target: &LaunchTarget) -> Result<Vec<String>, String> {
    let templates = match (target.kind, &app.args_for_file) {
        (TargetKind::File, Some(file_args)) => file_args,
        _ => &app.args,
    };
    templates.iter().map(|t| expand_arg(t, target)).collect()
}

/// Resolves `program` against absolute path or `PATH` environment variable.
pub fn resolve_executable(program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        return None;
    }
    if program.contains('\\') || program.contains('/') {
        return None;
    }
    let candidate_name = if program.to_ascii_lowercase().ends_with(".exe") {
        program.to_string()
    } else {
        format!("{program}.exe")
    };
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        if !dir.is_absolute() {
            continue;
        }
        let full = dir.join(&candidate_name);
        if full.exists() && !full.is_dir() {
            return Some(full);
        }
    }
    None
}

/// Runs the resolved CUI process synchronously with inherited stdio (caller suspends TUI before calling).
pub fn spawn_cui_blocking(exe: &Path, args: &[String], working_dir: &Path) -> Result<(), String> {
    Command::new(exe)
        .args(args)
        .current_dir(working_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch '{}': {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_four_cui_tools_match_spec() {
        let apps = default_external_apps();
        assert_eq!(apps.len(), 4);
        assert_eq!(apps[0].program, "pwsh.exe");
        assert_eq!(apps[1].program, "nvim.exe");
        assert_eq!(apps[2].program, "lazygit.exe");
        assert_eq!(apps[3].program, "bat.exe");
    }

    #[test]
    fn expand_arg_replaces_path_and_dir_and_escaped_braces() {
        let target = LaunchTarget {
            path: PathBuf::from(r"C:\work\src\main.rs"),
            dir: PathBuf::from(r"C:\work\src"),
            kind: TargetKind::File,
        };
        assert_eq!(expand_arg("{path}", &target).unwrap(), r"C:\work\src\main.rs");
        assert_eq!(expand_arg("{dir}", &target).unwrap(), r"C:\work\src");
        assert_eq!(expand_arg("{{literal}}", &target).unwrap(), "{literal}");
        assert!(expand_arg("{unknown}", &target).is_err());
    }
}
