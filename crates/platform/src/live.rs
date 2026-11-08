//! The bridge between the Win32 message loop and the egui surface.
//!
//! Win32 owns its thread: the clipboard listener, the hotkey and the tray all
//! deliver messages to the window that [`Platform`] created, and those
//! messages must be pumped on that same thread. The egui surface likewise owns
//! the main thread. [`LiveSource::spawn`] therefore runs the platform on a
//! worker thread and talks to the surface over two `std::sync::mpsc` channels:
//!
//! * snapshots of the history go *to* the UI, woken with
//!   [`pastebox_ui::Waker`] (which calls `Context::request_repaint`),
//! * [`Command`]s go *from* the UI, and the UI never waits for them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use pastebox_core::{rank, ClipItem};
use pastebox_ui::{ClipMeta, ClipboardSource, Waker};

use crate::app::{Config, Event, Platform};
use crate::tray::TrayCommand;

/// How long the message loop sleeps between polls. Long enough to idle at
/// roughly zero CPU, short enough that a clipboard change feels instant (the
/// wake hook then repaints immediately).
const POLL_MS: u64 = 20;

/// A palette click writes the clipboard and then synthesises Ctrl+V; give the
/// surface a moment to give the foreground back before the keystroke.
const PASTE_DELAY_MS: u64 = 120;

/// Something the surface asks the platform to do. Sent from the UI thread;
/// executed on the message-loop thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Paste(u64),
    Copy(u64),
    Pin { id: u64, pinned: bool },
    Remove(u64),
    ClearUnpinned,
    Quit,
}

#[derive(Clone)]
enum Update {
    Clips { items: Vec<ClipItem>, bytes: usize },
    Failed(String),
}

/// A [`ClipboardSource`] backed by the real Win32 history.
///
/// Dropping it stops the message loop and joins the worker, so the process
/// exits cleanly when the palette closes.
pub struct LiveSource {
    clips: Vec<ClipItem>,
    bytes: usize,
    rx: Receiver<Update>,
    cmd: Sender<Command>,
    wake: Arc<Mutex<Option<Waker>>>,
    show: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl LiveSource {
    /// A handle for posting [`Command`]s. `send` never blocks the UI thread.
    #[must_use]
    pub fn sender(&self) -> Sender<Command> {
        self.cmd.clone()
    }

    /// Start the platform on its own thread and wait for the first snapshot.
    ///
    /// Returns the platform's own message when it cannot start (a second
    /// instance, no free hotkey), so the caller can show it before opening a
    /// window.
    ///
    /// # Errors
    /// The platform failed to start, or did not report in within five seconds.
    pub fn spawn(config: Config) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
        let (upd_tx, upd_rx) = mpsc::channel::<Update>();
        let wake: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let show = Arc::new(AtomicBool::new(false));

        let worker = {
            let wake = Arc::clone(&wake);
            let show = Arc::clone(&show);
            thread::Builder::new()
                .name("pastebox-platform".to_string())
                .spawn(move || run(config, cmd_rx, upd_tx, wake, show))
                .map_err(|err| format!("could not start the platform thread: {err}"))?
        };

        let first = upd_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "Pastebox did not start in time".to_string())?;
        match first {
            Update::Clips { items, bytes } => Ok(Self {
                clips: items,
                bytes,
                rx: upd_rx,
                cmd: cmd_tx,
                wake,
                show,
                worker: Some(worker),
            }),
            Update::Failed(err) => {
                let _ = worker.join();
                Err(err)
            }
        }
    }
}

impl ClipboardSource for LiveSource {
    fn clips(&self) -> &[ClipItem] {
        &self.clips
    }

    fn search(&self, query: &str) -> Vec<usize> {
        rank(&self.clips, query)
    }

    fn bytes_on_disk(&self) -> usize {
        self.bytes
    }

    fn meta(&self, _id: u64) -> ClipMeta {
        // The Win32 clipboard API does not hand us the owning process; rows
        // show the content detail alone rather than a guessed app name.
        ClipMeta::default()
    }

    fn attach(&mut self, wake: Waker) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(wake);
        }
    }

    fn sync(&mut self) -> bool {
        let mut changed = false;
        while let Ok(update) = self.rx.try_recv() {
            match update {
                Update::Clips { items, bytes } => {
                    changed |= items.len() != self.clips.len() || items != self.clips;
                    self.clips = items;
                    self.bytes = bytes;
                }
                // A platform that dies after start leaves the last snapshot on
                // screen; there is nothing useful to show in its place.
                Update::Failed(_) => {}
            }
        }
        changed
    }

    fn take_show(&mut self) -> bool {
        self.show.swap(false, Ordering::Relaxed)
    }
}

impl Drop for LiveSource {
    fn drop(&mut self) {
        let _ = self.cmd.send(Command::Quit);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    config: Config,
    rx: Receiver<Command>,
    tx: Sender<Update>,
    wake: Arc<Mutex<Option<Waker>>>,
    show: Arc<AtomicBool>,
) {
    let mut platform = match Platform::start(config) {
        Ok(platform) => platform,
        Err(err) => {
            let _ = tx.send(Update::Failed(err.to_string()));
            return;
        }
    };

    if tx.send(snapshot(&platform)).is_err() {
        return;
    }
    loop {
        let mut dirty = false;
        let mut quit = false;

        while let Ok(command) = rx.try_recv() {
            dirty = true;
            match command {
                Command::Quit => {
                    quit = true;
                    break;
                }
                Command::Paste(id) => {
                    thread::sleep(Duration::from_millis(PASTE_DELAY_MS));
                    platform.paste(id);
                }
                Command::Copy(id) => {
                    platform.copy(id);
                }
                Command::Pin { id, pinned } => {
                    let _ = platform.set_pinned(id, pinned);
                }
                Command::Remove(id) => {
                    let _ = platform.remove(id);
                }
                Command::ClearUnpinned => {
                    let _ = platform.clear_unpinned();
                }
            }
        }

        if quit {
            platform.request_quit();
        }

        for event in platform.poll_events() {
            match event {
                Event::Quit => {
                    let _ = tx.send(snapshot(&platform));
                    return;
                }
                Event::HotkeyPressed | Event::Tray(TrayCommand::Open) => {
                    show.store(true, Ordering::Relaxed);
                    dirty = true;
                }
                Event::ClipCaptured(_) => dirty = true,
                Event::Tray(TrayCommand::TogglePause)
                | Event::Tray(TrayCommand::ClearUnpinned)
                | Event::Tray(TrayCommand::Quit) => dirty = true,
                Event::ClipboardRestored => {}
                // Internal states, already resolved inside `poll_events`.
                Event::ClipboardChanged | Event::TrayMenu | Event::RestoreDue => {}
            }
        }

        if dirty {
            if tx.send(snapshot(&platform)).is_err() {
                return;
            }
            if let Ok(slot) = wake.lock() {
                if let Some(wake) = slot.as_ref() {
                    wake();
                }
            }
        }
        // The show flag alone needs a repaint even when nothing else moved.
        if show.load(Ordering::Relaxed) {
            if let Ok(slot) = wake.lock() {
                if let Some(wake) = slot.as_ref() {
                    wake();
                }
            }
        }

        thread::sleep(Duration::from_millis(POLL_MS));
    }
}

fn snapshot(platform: &Platform) -> Update {
    let items = platform.store().items().to_vec();
    let bytes = items
        .iter()
        .filter(|item| item.pinned)
        .map(|item| item.byte_len)
        .sum();
    Update::Clips { items, bytes }
}
