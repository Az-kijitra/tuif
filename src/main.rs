//! `tuif` binary entry point (SPEC-0300-0001).
//! Supports `--choosedir <FILE>` / `--cwd-file <FILE>` for PowerShell `cd on exit` integration.

use std::env;
use std::fs;
use std::io::{self, stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use tuif::app::AppState;
use tuif::external_apps::{run_fuzzy_finder, spawn_cui_blocking};
use tuif::tui;

struct CliArgs {
    start_dir: PathBuf,
    choosedir_file: Option<PathBuf>,
    show_help: bool,
    show_version: bool,
}

fn parse_args() -> CliArgs {
    let mut start_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from(r"C:\"));
    let mut choosedir_file = None;
    let mut show_help = false;
    let mut show_version = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => show_help = true,
            "-V" | "--version" => show_version = true,
            "--choosedir" | "--cwd-file" => {
                if let Some(p) = args.next() {
                    choosedir_file = Some(PathBuf::from(p));
                }
            }
            other if !other.starts_with('-') => {
                let candidate = PathBuf::from(other);
                if candidate.is_dir() {
                    start_dir = candidate;
                }
            }
            _ => {}
        }
    }

    CliArgs {
        start_dir,
        choosedir_file,
        show_help,
        show_version,
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen);
}

fn main() -> io::Result<()> {
    let cli = parse_args();
    if cli.show_version {
        println!("tuif {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if cli.show_help {
        println!(
            "tuif {} — Vim-like TUI File Manager for PowerShell",
            env!("CARGO_PKG_VERSION")
        );
        println!();
        println!("Usage: tuif [OPTIONS] [START_DIR]");
        println!();
        println!("Options:");
        println!("  --choosedir <FILE>   Write the last visited directory to <FILE> on exit");
        println!("  -h, --help           Print help");
        println!("  -V, --version        Print version");
        return Ok(());
    }

    // Install panic hook so terminal raw mode / alternate screen is always restored
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppState::new(cli.start_dir);

    while !app.should_quit {
        app.poll_active_transfer();

        if let Ok(size) = terminal.size() {
            app.update_viewport_size(size.width, size.height);
        }
        terminal.draw(|frame| tui::draw(frame, &app))?;

        // Check if an external CUI tool (`pwsh`, `nvim`, `lazygit`, `bat`) was requested
        if let Some(launch) = app.pending_launch.take() {
            restore_terminal();
            let res = spawn_cui_blocking(&launch.exe, &launch.args, &launch.working_dir);
            enable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                EnterAlternateScreen,
                EnableMouseCapture
            )?;
            terminal.clear()?;
            app.reload_dir();
            match res {
                Ok(()) => {
                    app.status_message = format!("Returned from '{}'", launch.app_name);
                }
                Err(err_msg) => {
                    app.status_message = err_msg;
                }
            }
            continue;
        }

        // Check if a fuzzy search (`fzf` + `rg` + `bat`) was requested
        if let Some(fuzzy_req) = app.pending_fuzzy.take() {
            restore_terminal();
            let res = run_fuzzy_finder(&fuzzy_req);
            enable_raw_mode()?;
            execute!(
                terminal.backend_mut(),
                EnterAlternateScreen,
                EnableMouseCapture
            )?;
            terminal.clear()?;
            match res {
                Ok(Some(sel)) => {
                    app.apply_fuzzy_selection(sel);
                }
                Ok(None) => {
                    app.reload_dir();
                    app.status_message = "Fuzzy search cancelled".to_string();
                }
                Err(err_msg) => {
                    app.reload_dir();
                    app.status_message = err_msg;
                }
            }
            continue;
        }

        let poll_timeout = if app.active_transfer.is_some() {
            Duration::from_millis(50)
        } else {
            Duration::from_millis(100)
        };

        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key_event) => {
                    // On Windows, crossterm emits both Press and Release events; handle Press only.
                    if key_event.kind == KeyEventKind::Press {
                        app.handle_key_event(key_event);
                    }
                }
                Event::Mouse(mouse_event) => {
                    app.handle_mouse_event(mouse_event);
                }
                Event::Resize(w, h) => {
                    app.update_viewport_size(w, h);
                }
                _ => {}
            }
        }
    }

    restore_terminal();

    if app.should_cd_on_exit {
        if let Some(out_file) = cli.choosedir_file {
            let _ = fs::write(out_file, app.current_dir.to_string_lossy().as_bytes());
        }
    }

    Ok(())
}
