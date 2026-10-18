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
    pub is_gui: bool,
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

/// Default external tools: 4 CUI tools (`pwsh`, `nvim`, `lazygit`, `bat`) + 2 GUI tools (`VSCode`, `Explorer`).
pub fn default_external_apps() -> Vec<ExternalAppEntry> {
    vec![
        ExternalAppEntry {
            name: "Open PowerShell".to_string(),
            program: "pwsh.exe".to_string(),
            args: vec!["-NoLogo".to_string(), "-WorkingDirectory".to_string(), "{dir}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+C").ok(),
            key_label: "Ctrl+Alt+C".to_string(),
            is_gui: false,
        },
        ExternalAppEntry {
            name: "Open in Neovim".to_string(),
            program: "nvim.exe".to_string(),
            args: vec!["{path}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+V").ok(),
            key_label: "Ctrl+Alt+V".to_string(),
            is_gui: false,
        },
        ExternalAppEntry {
            name: "Open Lazygit".to_string(),
            program: "lazygit.exe".to_string(),
            args: vec!["-p".to_string(), "{dir}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+G").ok(),
            key_label: "Ctrl+Alt+G".to_string(),
            is_gui: false,
        },
        ExternalAppEntry {
            name: "View with bat".to_string(),
            program: "bat.exe".to_string(),
            args: vec!["--paging=always".to_string(), "{path}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+B").ok(),
            key_label: "Ctrl+Alt+B".to_string(),
            is_gui: false,
        },
        ExternalAppEntry {
            name: "Open VSCode".to_string(),
            program: "Code.exe".to_string(),
            args: vec!["{path}".to_string()],
            args_for_file: None,
            key: KeyChord::parse_list("Ctrl+Alt+O").ok(),
            key_label: "Ctrl+Alt+O".to_string(),
            is_gui: true,
        },
        ExternalAppEntry {
            name: "Open Explorer".to_string(),
            program: "explorer.exe".to_string(),
            args: vec!["{dir}".to_string()],
            args_for_file: Some(vec!["/select,{path}".to_string()]),
            key: KeyChord::parse_list("Ctrl+Alt+E").ok(),
            key_label: "Ctrl+Alt+E".to_string(),
            is_gui: true,
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

/// Resolves `program` against absolute path, `PATH` environment variable, or known installation paths.
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
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            if !dir.is_absolute() {
                continue;
            }
            let full = dir.join(&candidate_name);
            if full.exists() && !full.is_dir() {
                return Some(full);
            }
        }
    }

    if candidate_name.eq_ignore_ascii_case("Code.exe") {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            let candidate = Path::new(&local_app_data)
                .join("Programs")
                .join("Microsoft VS Code")
                .join("Code.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        for p in [
            r"C:\Program Files\Microsoft VS Code\Code.exe",
            r"C:\Program Files (x86)\Microsoft VS Code\Code.exe",
        ] {
            let candidate = PathBuf::from(p);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    } else if candidate_name.eq_ignore_ascii_case("explorer.exe") {
        let candidate = PathBuf::from(r"C:\Windows\explorer.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Resolves `less.exe` from `PATH` or standard Git for Windows installation directories.
pub fn resolve_less_executable() -> Option<PathBuf> {
    if let Some(exe) = resolve_executable("less.exe") {
        return Some(exe);
    }
    let mut candidates = vec![
        PathBuf::from(r"C:\Program Files\Git\usr\bin\less.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\usr\bin\less.exe"),
    ];
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        candidates.push(
            Path::new(&local_app_data)
                .join("Programs")
                .join("Git")
                .join("usr")
                .join("bin")
                .join("less.exe"),
        );
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Spawns a GUI process (`Code.exe`, `explorer.exe`) asynchronously without suspending the TUI.
pub fn spawn_gui_nonblocking(exe: &Path, args: &[String], working_dir: &Path) -> Result<(), String> {
    Command::new(exe)
        .args(args)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to launch '{}': {e}", exe.display()))
}

/// Runs the resolved CUI process synchronously with inherited stdio (caller suspends TUI before calling).
pub fn spawn_cui_blocking(exe: &Path, args: &[String], working_dir: &Path) -> Result<(), String> {
    let is_bat = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("bat"))
        .unwrap_or(false);

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(working_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let mut has_less_pager = false;
    if is_bat {
        if env::var_os("BAT_PAGER").is_some() {
            has_less_pager = true;
        } else if let Some(less_exe) = resolve_less_executable() {
            // Pass `-R` without `-F` (`--quit-if-one-screen`) so `less` stays open
            // even when the file fits on a single screen, until the user presses `q`.
            let normalized = less_exe.to_string_lossy().replace('\\', "/");
            cmd.env("BAT_PAGER", format!("\"{normalized}\" -R"));
            has_less_pager = true;
        }
    }

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to launch '{}': {e}", exe.display()))?;

    // If `bat` ran without `less.exe` available, pause so the output doesn't flash and disappear.
    if is_bat && (!has_less_pager || !status.success()) {
        println!("\n[Press Enter to return to tuif...]");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_external_tools_include_cui_and_gui_apps() {
        let apps = default_external_apps();
        assert_eq!(apps.len(), 6);
        assert_eq!(apps[0].program, "pwsh.exe");
        assert!(!apps[0].is_gui);
        assert_eq!(apps[1].program, "nvim.exe");
        assert!(!apps[1].is_gui);
        assert_eq!(apps[2].program, "lazygit.exe");
        assert!(!apps[2].is_gui);
        assert_eq!(apps[3].program, "bat.exe");
        assert!(!apps[3].is_gui);
        assert_eq!(apps[4].program, "Code.exe");
        assert!(apps[4].is_gui);
        assert_eq!(apps[5].program, "explorer.exe");
        assert!(apps[5].is_gui);
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
