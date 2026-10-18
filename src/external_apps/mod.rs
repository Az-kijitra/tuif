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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzyKind {
    FindFile,
    LiveGrep,
    JumpDir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingFuzzySearch {
    pub kind: FuzzyKind,
    pub working_dir: PathBuf,
    pub show_hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzySelection {
    pub kind: FuzzyKind,
    pub target_path: PathBuf,
    pub line_number: Option<usize>,
}

/// Strips leading `./` or `.\` prefixes and surrounding whitespace from a relative path output.
pub fn clean_relative_path(raw: &str) -> &str {
    let mut s = raw.trim();
    while let Some(rest) = s.strip_prefix(".\\").or_else(|| s.strip_prefix("./")) {
        s = rest;
    }
    s
}

/// Parses an `rg --line-number --no-heading` output line (`path:line:content`) selected via `fzf`.
pub fn parse_live_grep_selection(line: &str) -> Option<(String, Option<usize>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let search_start = if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        2
    } else {
        0
    };

    let rel_colon = trimmed[search_start..].find(':')?;
    let colon_idx = search_start + rel_colon;
    let raw_path = &trimmed[..colon_idx];
    let cleaned_path = clean_relative_path(raw_path);
    if cleaned_path.is_empty() {
        return None;
    }

    let after_path = &trimmed[colon_idx + 1..];
    let line_num = after_path
        .split(':')
        .next()
        .and_then(|s| s.trim().parse::<usize>().ok());

    Some((cleaned_path.to_string(), line_num))
}

/// Walks subdirectories under `root` and writes relative paths (one per line) to `sink`.
pub fn stream_subdirectories(root: &Path, show_hidden: bool, mut sink: impl std::io::Write) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = rd
            .flatten()
            .filter_map(|de| {
                let ft = de.file_type().ok()?;
                if !ft.is_dir() {
                    return None;
                }
                let name = de.file_name();
                let name_str = name.to_string_lossy();
                if name_str.eq_ignore_ascii_case(".git") {
                    return None;
                }
                if !show_hidden && name_str.starts_with('.') {
                    return None;
                }
                Some(de.path())
            })
            .collect();

        children.sort_by(|a, b| {
            b.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
                .cmp(
                    &a.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase(),
                )
        });

        for child in &children {
            if let Ok(rel) = child.strip_prefix(root) {
                let rel_str = rel.to_string_lossy();
                if !rel_str.is_empty() && writeln!(sink, "{rel_str}").is_err() {
                    return;
                }
            }
        }
        stack.extend(children);
    }
}

/// Walks files under `root` as a fallback when `rg` is unavailable.
fn stream_files_fallback(root: &Path, show_hidden: bool, mut sink: impl std::io::Write) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for de in rd.flatten() {
            let Ok(ft) = de.file_type() else {
                continue;
            };
            let name = de.file_name();
            let name_str = name.to_string_lossy();
            if name_str.eq_ignore_ascii_case(".git") {
                continue;
            }
            if !show_hidden && name_str.starts_with('.') {
                continue;
            }
            let p = de.path();
            if ft.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(root) {
                if writeln!(sink, "{}", rel.display()).is_err() {
                    return;
                }
            }
        }
    }
}

/// Runs `fzf` (paired with `rg` and `bat` when available) synchronously and returns the selected target.
pub fn run_fuzzy_finder(req: &PendingFuzzySearch) -> Result<Option<FuzzySelection>, String> {
    use std::io::Read;

    let Some(fzf_exe) = resolve_executable("fzf.exe") else {
        return Err("fzf.exe is not found in PATH".to_string());
    };
    let rg_exe = resolve_executable("rg.exe");
    let bat_exe = resolve_executable("bat.exe");

    match req.kind {
        FuzzyKind::FindFile => {
            let mut fzf_args = vec![
                "--layout=reverse".to_string(),
                "--border".to_string(),
                "--prompt=Find File> ".to_string(),
                "--header=Enter: Jump to file in tuif | Esc: Cancel".to_string(),
            ];
            if bat_exe.is_some() {
                fzf_args.push("--preview=bat --color=always --style=numbers --line-range=:300 -- {}".to_string());
                fzf_args.push("--preview-window=right:55%:border-left".to_string());
            }

            let mut rg_child = if let Some(ref rg) = rg_exe {
                let mut rg_args = vec!["--files", "--color=never"];
                if req.show_hidden {
                    rg_args.extend_from_slice(&["--hidden", "-g", "!.git"]);
                }
                Command::new(rg)
                    .args(&rg_args)
                    .current_dir(&req.working_dir)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .ok()
            } else {
                None
            };

            let mut fzf_cmd = Command::new(&fzf_exe);
            fzf_cmd
                .args(&fzf_args)
                .current_dir(&req.working_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());

            let mut fallback_writer = false;
            if let Some(ref mut rg_proc) = rg_child {
                if let Some(rg_out) = rg_proc.stdout.take() {
                    fzf_cmd.stdin(Stdio::from(rg_out));
                } else {
                    fzf_cmd.stdin(Stdio::piped());
                    fallback_writer = true;
                }
            } else {
                fzf_cmd.stdin(Stdio::piped());
                fallback_writer = true;
            }

            let mut fzf_child = fzf_cmd
                .spawn()
                .map_err(|e| format!("Failed to launch fzf: {e}"))?;

            let writer_handle = if fallback_writer {
                if let Some(stdin) = fzf_child.stdin.take() {
                    let root = req.working_dir.clone();
                    let show_hidden = req.show_hidden;
                    Some(std::thread::spawn(move || {
                        stream_files_fallback(&root, show_hidden, stdin);
                    }))
                } else {
                    None
                }
            } else {
                None
            };

            let mut output_str = String::new();
            if let Some(mut stdout) = fzf_child.stdout.take() {
                let _ = stdout.read_to_string(&mut output_str);
            }
            let status = fzf_child
                .wait()
                .map_err(|e| format!("Failed waiting on fzf: {e}"))?;

            if let Some(mut rg_proc) = rg_child {
                let _ = rg_proc.kill();
                let _ = rg_proc.wait();
            }
            if let Some(h) = writer_handle {
                let _ = h.join();
            }

            if !status.success() {
                return Ok(None);
            }
            let cleaned = clean_relative_path(&output_str);
            if cleaned.is_empty() {
                return Ok(None);
            }
            Ok(Some(FuzzySelection {
                kind: FuzzyKind::FindFile,
                target_path: req.working_dir.join(cleaned),
                line_number: None,
            }))
        }
        FuzzyKind::LiveGrep => {
            let Some(rg) = rg_exe else {
                return Err("ripgrep (rg.exe) is not found in PATH".to_string());
            };
            let mut rg_args = vec![
                "--line-number",
                "--no-heading",
                "--color=always",
                "--smart-case",
            ];
            if req.show_hidden {
                rg_args.extend_from_slice(&["--hidden", "-g", "!.git"]);
            }
            rg_args.extend_from_slice(&["", "."]);

            let mut rg_child = Command::new(&rg)
                .args(&rg_args)
                .current_dir(&req.working_dir)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to launch rg: {e}"))?;

            let rg_stdout = rg_child
                .stdout
                .take()
                .ok_or_else(|| "Failed to capture rg stdout".to_string())?;

            let mut fzf_args = vec![
                "--ansi".to_string(),
                "--layout=reverse".to_string(),
                "--border".to_string(),
                "--prompt=Live Grep> ".to_string(),
                "--header=Type keywords to search contents | Enter: Jump to file | Esc: Cancel".to_string(),
                "--delimiter=:".to_string(),
            ];
            if bat_exe.is_some() {
                fzf_args.push("--preview=bat --color=always --style=numbers --highlight-line {2} -- {1}".to_string());
                fzf_args.push("--preview-window=right:55%:border-left:+{2}-/2".to_string());
            }

            let mut fzf_child = Command::new(&fzf_exe)
                .args(&fzf_args)
                .current_dir(&req.working_dir)
                .stdin(Stdio::from(rg_stdout))
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("Failed to launch fzf: {e}"))?;

            let mut output_str = String::new();
            if let Some(mut stdout) = fzf_child.stdout.take() {
                let _ = stdout.read_to_string(&mut output_str);
            }
            let status = fzf_child
                .wait()
                .map_err(|e| format!("Failed waiting on fzf: {e}"))?;

            let _ = rg_child.kill();
            let _ = rg_child.wait();

            if !status.success() {
                return Ok(None);
            }
            let Some((rel_path, line_number)) = parse_live_grep_selection(&output_str) else {
                return Ok(None);
            };
            Ok(Some(FuzzySelection {
                kind: FuzzyKind::LiveGrep,
                target_path: req.working_dir.join(rel_path),
                line_number,
            }))
        }
        FuzzyKind::JumpDir => {
            let fzf_args = vec![
                "--layout=reverse".to_string(),
                "--border".to_string(),
                "--prompt=Jump Folder> ".to_string(),
                "--header=Enter: Open selected folder | Esc: Cancel".to_string(),
                "--preview=cmd /c dir /b {}".to_string(),
                "--preview-window=right:50%:border-left".to_string(),
            ];

            let mut fzf_child = Command::new(&fzf_exe)
                .args(&fzf_args)
                .current_dir(&req.working_dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .map_err(|e| format!("Failed to launch fzf: {e}"))?;

            let writer_handle = fzf_child.stdin.take().map(|stdin| {
                let root = req.working_dir.clone();
                let show_hidden = req.show_hidden;
                std::thread::spawn(move || {
                    stream_subdirectories(&root, show_hidden, stdin);
                })
            });

            let mut output_str = String::new();
            if let Some(mut stdout) = fzf_child.stdout.take() {
                let _ = stdout.read_to_string(&mut output_str);
            }
            let status = fzf_child
                .wait()
                .map_err(|e| format!("Failed waiting on fzf: {e}"))?;

            if let Some(h) = writer_handle {
                let _ = h.join();
            }

            if !status.success() {
                return Ok(None);
            }
            let cleaned = clean_relative_path(&output_str);
            if cleaned.is_empty() {
                return Ok(None);
            }
            Ok(Some(FuzzySelection {
                kind: FuzzyKind::JumpDir,
                target_path: req.working_dir.join(cleaned),
                line_number: None,
            }))
        }
    }
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

    #[test]
    fn clean_relative_path_strips_dot_slashes_and_newlines() {
        assert_eq!(clean_relative_path("  .\\src\\app.rs\r\n"), r"src\app.rs");
        assert_eq!(clean_relative_path("./docs/user_manual.md\n"), "docs/user_manual.md");
        assert_eq!(clean_relative_path("Cargo.toml"), "Cargo.toml");
    }

    #[test]
    fn parse_live_grep_selection_extracts_path_and_line_number() {
        let (path, line) =
            parse_live_grep_selection(r".\src\app.rs:1601:    pub fn poll_active_transfer(&mut self) {")
                .unwrap();
        assert_eq!(path, r"src\app.rs");
        assert_eq!(line, Some(1601));

        let (abs_path, abs_line) =
            parse_live_grep_selection(r"C:\work\src\main.rs:42:fn main() {").unwrap();
        assert_eq!(abs_path, r"C:\work\src\main.rs");
        assert_eq!(abs_line, Some(42));
    }

    #[test]
    fn stream_subdirectories_respects_hidden_flag_and_skips_git() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("src").join("remote")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".git").join("objects")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".hidden_dir")).unwrap();

        let mut buf_no_hidden = Vec::new();
        stream_subdirectories(tmp.path(), false, &mut buf_no_hidden);
        let out_no_hidden = String::from_utf8(buf_no_hidden).unwrap();
        let lines: Vec<&str> = out_no_hidden.lines().collect();
        assert!(lines.contains(&"src"));
        assert!(lines.contains(&r"src\remote") || lines.contains(&"src/remote"));
        assert!(!lines.iter().any(|l| l.contains(".git")));
        assert!(!lines.iter().any(|l| l.contains(".hidden_dir")));

        let mut buf_with_hidden = Vec::new();
        stream_subdirectories(tmp.path(), true, &mut buf_with_hidden);
        let out_with_hidden = String::from_utf8(buf_with_hidden).unwrap();
        let lines_hidden: Vec<&str> = out_with_hidden.lines().collect();
        assert!(lines_hidden.contains(&".hidden_dir"));
        assert!(!lines_hidden.iter().any(|l| l.contains(".git")));
    }
}
