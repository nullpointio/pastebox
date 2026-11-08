//! Pastebox's Win32 layer.
//!
//! * clipboard capture (`CF_UNICODETEXT`, `CF_HDROP`, `CF_DIB`/PNG dimensions)
//! * a global hotkey with a fallback chain
//! * paste-back with clipboard restoration
//! * a single-instance guard and a tray icon
//!
//! Everything here is Windows-only; on other hosts the crate compiles to a stub
//! so the workspace still builds on a Linux dev box. [`LiveSource`] implements
//! `pastebox_ui::ClipboardSource`, so the arrow points platform → UI; the UI
//! never links this crate.

#![deny(clippy::unwrap_used, clippy::expect_used)]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod hotkey;
#[cfg(windows)]
mod live;
#[cfg(windows)]
mod paste;
#[cfg(windows)]
mod single;
#[cfg(windows)]
mod tray;
#[cfg(windows)]
mod wide;

#[cfg(windows)]
pub use app::{
    alert, default_pins_path, run_headless, smoke, Config, Event, Platform, PlatformError,
};
#[cfg(windows)]
pub use hotkey::HotkeyCombo;
#[cfg(windows)]
pub use live::{Command, LiveSource};
#[cfg(windows)]
pub use tray::TrayCommand;

/// Crate version, for `-version` and about boxes.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Non-Windows stub: the crate is Windows-only, so nothing is available.
#[cfg(not(windows))]
pub mod stub {
    /// Always false: there is no Win32 layer to smoke-test here.
    pub fn smoke() -> bool {
        println!("SKIP pastebox-platform: Windows only");
        false
    }
}

#[cfg(not(windows))]
pub use stub::smoke;
