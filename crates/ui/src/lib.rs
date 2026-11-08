//! Pastebox UI — the palette surface, drawn with egui.
//!
//! The crate is a pure renderer: it never touches the clipboard, the disk or
//! the network. Everything it shows comes from a [`ClipboardSource`], and every
//! user intent leaves as an [`Action`] for the app crate to perform.
//!
//! ```no_run
//! pastebox_ui::run_demo().expect("demo window");
//! ```

#![forbid(unsafe_code)]

pub mod app;
pub mod backdrop;
pub mod demo;
pub mod format;
pub mod icons;
pub mod layout;
pub mod tokens;

use pastebox_core::ClipItem;

pub use app::{Action, Backdrop, Filter, PasteboxApp, Theme};
pub use demo::DemoSource;

/// Per-clip facts the core model does not carry but the rows need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipMeta {
    /// Owning process, e.g. `OUTLOOK.EXE`. Empty when unknown.
    pub source_app: String,
    /// Pixel dimensions for image clips, when the platform decoded them.
    pub dims: Option<(u32, u32)>,
    /// Over the R-09 limit: shown but not pasteable.
    pub too_large: bool,
}

/// What the UI renders from. The app crate supplies the real one; this crate
/// ships [`DemoSource`].
pub trait ClipboardSource {
    /// Newest first, exactly the store's order.
    fn clips(&self) -> &[ClipItem];

    /// Indices into [`ClipboardSource::clips`], best first.
    fn search(&self, query: &str) -> Vec<usize>;

    /// How many clips the history holds, for `Search {n} clips…`. Defaults to
    /// the number actually retained.
    fn total(&self) -> usize {
        self.clips().len()
    }

    /// Bytes on disk, for the footer trust line. Pins only, so usually small.
    fn bytes_on_disk(&self) -> usize {
        0
    }

    /// Source app and dimensions for one clip.
    fn meta(&self, _id: u64) -> ClipMeta {
        ClipMeta::default()
    }

    /// Hand the source a way to ask the surface to repaint from another thread
    /// (the Win32 message loop, a timer, a worker). Called once, after the
    /// window exists.
    fn attach(&mut self, _wake: Waker) {}

    /// Pull in anything the platform produced since the last frame. Returns
    /// `true` when the visible list changed and the app must re-rank. Static
    /// sources (the demo) leave this as a no-op.
    fn sync(&mut self) -> bool {
        false
    }

    /// Whether the platform asked for the palette to be shown (global hotkey,
    /// tray "Open"). One-shot.
    fn take_show(&mut self) -> bool {
        false
    }
}

/// Ask the surface to repaint. Cheap, thread-safe, never blocks.
pub type Waker = Box<dyn Fn() + Send + Sync>;

/// Open the real palette over `source`: 960 × 600, no decorations, transparent
/// so the desktop shows through the rounded corners.
///
/// `sink` receives every [`Action`] the user produces. It runs on the UI
/// thread, so it must not block — send a command to the platform's message
/// loop and return.
///
/// # Errors
/// Propagates the windowing backend's failure to create a viewport.
pub fn run<S>(
    source: S,
    now_ms: u64,
    sink: impl FnMut(Action) -> Result<(), String> + 'static,
) -> eframe::Result<()>
where
    S: ClipboardSource + 'static,
{
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([layout::PANEL_W, layout::PANEL_H])
            .with_min_inner_size([760.0, 440.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_title("Pastebox"),
        ..Default::default()
    };
    eframe::run_native(
        "Pastebox",
        options,
        Box::new(move |cc| {
            let app = PasteboxApp::new(cc, Box::new(source), now_ms)
                .with_backdrop(Backdrop::None)
                .with_sink(Box::new(sink));
            Ok(Box::new(app))
        }),
    )
}

/// Open the 1280 × 800 demo surface: the hero composition, over a dimmed
/// desktop, loaded with [`DemoSource`].
///
/// # Errors
/// Propagates the windowing backend's failure to create a viewport.
pub fn run_demo() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([760.0, 440.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_title("Pastebox"),
        ..Default::default()
    };
    eframe::run_native(
        "Pastebox",
        options,
        Box::new(|cc| Ok(Box::new(PasteboxApp::demo(cc)))),
    )
}
