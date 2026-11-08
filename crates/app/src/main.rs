#![cfg_attr(all(windows, not(feature = "console")), windows_subsystem = "windows")]
//! Pastebox entry point: argument dispatch and the one seam to the UI.
//!
//! `-smoke` and `-selftest` exist so CI can prove the Win32 layer works on a
//! Windows runner; with no arguments the app starts normally.

mod selftest;

use std::process::ExitCode;

const USAGE: &str = "\
pastebox — clipboard history for Windows

USAGE:
    pastebox              start Pastebox
    pastebox -smoke       run the Win32 self checks and exit
    pastebox -selftest    run the core and Win32 self checks and exit
    pastebox -version     print the version and exit";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Smoke,
    SelfTest,
    Version,
    Help,
}

/// `-selftest` wins over `-smoke`, which wins over `-version`, whatever order
/// the flags appear in.
fn promote(current: Mode, next: Mode) -> Mode {
    match (current, next) {
        (Mode::SelfTest, _) | (_, Mode::SelfTest) => Mode::SelfTest,
        (Mode::Smoke, _) | (_, Mode::Smoke) => Mode::Smoke,
        _ => next,
    }
}

fn main() -> ExitCode {
    let mut mode = Mode::Run;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-smoke" => mode = promote(mode, Mode::Smoke),
            "-selftest" => mode = promote(mode, Mode::SelfTest),
            "-version" | "--version" | "-V" => mode = promote(mode, Mode::Version),
            "-h" | "--help" => mode = Mode::Help,
            other => {
                eprintln!("pastebox: unknown argument `{other}` (try --help)");
                return ExitCode::from(2);
            }
        }
    }

    match mode {
        Mode::Version => {
            println!("pastebox {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Mode::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Mode::Smoke => ExitCode::from(u8::from(!platform_smoke())),
        Mode::SelfTest => ExitCode::from(u8::from(!selftest::run())),
        Mode::Run => launch(),
    }
}

#[cfg(windows)]
fn platform_smoke() -> bool {
    pastebox_platform::smoke()
}

#[cfg(not(windows))]
fn platform_smoke() -> bool {
    eprintln!("pastebox: -smoke is Windows only");
    false
}

/// Milliseconds since the Unix epoch; the UI is clock-injected by contract.
#[cfg(windows)]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The one seam: start the Win32 service on its own thread, then hand its
/// history to the UI and let every user intent travel back as a command.
#[cfg(windows)]
fn launch() -> ExitCode {
    use pastebox_platform::{Config, LiveSource};
    use pastebox_ui::Action;

    let source = match LiveSource::spawn(Config::default()) {
        Ok(source) => source,
        Err(err) => {
            pastebox_platform::alert(&format!("Pastebox could not start: {err}"));
            return ExitCode::FAILURE;
        }
    };

    let commands = source.sender();
    let sink = move |action: Action| -> Result<(), String> {
        let command = match action {
            Action::Paste { id, .. } => pastebox_platform::Command::Paste(id),
            Action::Copy { id } => pastebox_platform::Command::Copy(id),
            Action::Pin { id, pinned } => pastebox_platform::Command::Pin { id, pinned },
            Action::Remove { id } => pastebox_platform::Command::Remove(id),
            Action::ClearUnpinned => pastebox_platform::Command::ClearUnpinned,
            Action::Close => pastebox_platform::Command::Quit,
        };
        commands
            .send(command)
            .map_err(|_| "Pastebox is shutting down".to_string())
    };

    match pastebox_ui::run(source, now_ms(), sink) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            pastebox_platform::alert(&format!("Pastebox could not open its window: {err}"));
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn launch() -> ExitCode {
    // The demo surface still runs on a non-Windows host, so the UI can be
    // looked at without a Windows box.
    match pastebox_ui::run_demo() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("pastebox: could not open the demo window: {err}");
            ExitCode::FAILURE
        }
    }
}
