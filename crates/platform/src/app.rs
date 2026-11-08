//! The Win32 service: one hidden message-only window that owns the clipboard
//! listener, the global hotkey and the tray icon, plus the history it feeds.
//!
//! Everything runs on the thread that created the platform (the message loop
//! thread). The window procedure never touches [`Platform`] directly — it only
//! queues [`Event`]s in thread-local storage — so there is no aliasing `&mut`
//! across the Win32 re-entry.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::mem;
use std::path::PathBuf;
use std::ptr;
use std::time::{SystemTime, UNIX_EPOCH};

use pastebox_core::{load_pinned, save_pinned, ClipKind, Store, DEFAULT_CAPACITY};
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, LoadCursorW,
    LoadIconW, PeekMessageW, PostMessageW, PostQuitMessage, RegisterClassW, SetTimer,
    TranslateMessage, UnregisterClassW, HWND_MESSAGE, IDC_ARROW, IDI_APPLICATION, MSG, PM_REMOVE,
    WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_QUIT, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSW,
};

use crate::clipboard::{self, ImageCache, Previous};
use crate::hotkey::{self, Registered};
use crate::single::{GuardError, SingleInstance};
use crate::tray::{Tray, TrayCommand};
use crate::wide::to_wide;

const WINDOW_CLASS: &str = "Pastebox.MessageWindow";
const WINDOW_TITLE: &str = "Pastebox";
const MUTEX_NAME: &str = "Local\\Pastebox.SingleInstance";
/// One-shot timer that puts the previous clipboard content back after a paste.
const TIMER_RESTORE: usize = 1;
/// How long the target application gets to read the clipboard before we restore
/// what was there before. Long enough for a real paste, short enough to feel
/// instant.
const RESTORE_DELAY_MS: u32 = 400;

/// Things the message loop can report to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A new clip was stored (or an existing row was refreshed); carries its id.
    ClipCaptured(u64),
    /// The user pressed the global hotkey.
    HotkeyPressed,
    /// A tray menu item was chosen.
    Tray(TrayCommand),
    /// The previous clipboard content has been put back.
    ClipboardRestored,
    /// Internal: the clipboard changed. Handled inside the platform.
    ClipboardChanged,
    /// Internal: the tray icon was clicked. Handled inside the platform.
    TrayMenu,
    /// Internal: the restore timer fired. Handled inside the platform.
    RestoreDue,
    /// The message loop should stop.
    Quit,
}

/// Why [`Platform::start`] failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformError {
    AlreadyRunning,
    HotkeyUnavailable,
    Win32(&'static str),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "pastebox is already running"),
            Self::HotkeyUnavailable => write!(
                f,
                "no usable global hotkey (tried Win+Alt+V, Win+Shift+V, Ctrl+Alt+V)"
            ),
            Self::Win32(what) => write!(f, "{what} failed"),
        }
    }
}

impl std::error::Error for PlatformError {}

/// Startup configuration.
pub struct Config {
    pub capacity: usize,
    pub pins_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            capacity: DEFAULT_CAPACITY,
            pins_path: default_pins_path(),
        }
    }
}

/// `%APPDATA%\Pastebox\pins.txt`, or `./Pastebox/pins.txt` when the environment
/// is missing (portable runs).
pub fn default_pins_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map_or_else(|| PathBuf::from("."), PathBuf::from);
    base.join("Pastebox").join("pins.txt")
}

thread_local! {
    /// Filled by the window procedure, drained by the platform.
    static QUEUE: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
    /// Clipboard content to put back once the paste has landed.
    static PENDING: RefCell<Option<Previous>> = const { RefCell::new(None) };
}

fn push(event: Event) {
    QUEUE.with(|queue| queue.borrow_mut().push_back(event));
}

/// Safe `extern "system"` function: it coerces to `WNDPROC` and keeps every
/// `unsafe` block explicit and commented.
extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            push(Event::ClipboardChanged);
            0
        }
        WM_HOTKEY => {
            push(Event::HotkeyPressed);
            0
        }
        WM_TIMER if wparam == TIMER_RESTORE => {
            push(Event::RestoreDue);
            0
        }
        WM_CLOSE => {
            // SAFETY: hwnd is the window this procedure was registered for.
            unsafe {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            push(Event::Quit);
            // SAFETY: ends the GetMessageW loop on this thread.
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ if msg == crate::tray::TRAY_MSG => {
            // Only react to a click, not to every hover/mouse move message.
            let button = lparam as u32;
            if button == WM_LBUTTONUP || button == WM_RBUTTONUP {
                push(Event::TrayMenu);
            }
            0
        }
        _ => {
            // SAFETY: default handling for every message we do not own.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

/// The hidden message-only window. Destroyed (and its class unregistered) on
/// drop.
struct HiddenWindow {
    hwnd: HWND,
    hinst: windows_sys::Win32::Foundation::HINSTANCE,
    class: Vec<u16>,
}

impl HiddenWindow {
    fn create() -> Option<Self> {
        // SAFETY: a null module name asks for the current executable.
        let hinst = unsafe { GetModuleHandleW(ptr::null()) };
        if hinst.is_null() {
            return None;
        }
        let class = to_wide(WINDOW_CLASS);
        let classdef = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            // SAFETY: both are system resources and a null module means the
            // system image.
            hIcon: unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) },
            hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        // SAFETY: `classdef` is fully initialised and `class` outlives the call.
        if unsafe { RegisterClassW(&classdef) } == 0 {
            // SAFETY: read immediately after the failing call.
            let last = unsafe { GetLastError() };
            if last != ERROR_CLASS_ALREADY_EXISTS {
                return None;
            }
        }
        let title = to_wide(WINDOW_TITLE);
        // SAFETY: HWND_MESSAGE makes this a message-only window: no taskbar
        // entry, no painting, but it still receives WM_CLIPBOARDUPDATE,
        // WM_HOTKEY and our tray callback.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                hinst,
                ptr::null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        Some(Self { hwnd, hinst, class })
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        // SAFETY: both handles are the ones we created in `create`.
        unsafe {
            DestroyWindow(self.hwnd);
            UnregisterClassW(self.class.as_ptr(), self.hinst);
        }
    }
}

/// Outcome of looking at the event queue once.
enum Step {
    Event(Event),
    Handled,
    Empty,
}

/// The running application service.
pub struct Platform {
    window: HiddenWindow,
    /// Held only for its lifetime: dropping it releases the single-instance lock.
    _instance: SingleInstance,
    hotkey: Option<Registered>,
    tray: Option<Tray>,
    store: Store,
    pins_path: PathBuf,
    images: ImageCache,
    paused: bool,
    /// Sequence number of the clipboard write we caused and must not re-capture.
    ignore_seq: Option<u32>,
    last_seq: u32,
}

impl Platform {
    /// Take the single-instance lock, create the hidden window, register the
    /// clipboard listener and the hotkey, add the tray icon and load pins.
    pub fn start(config: Config) -> Result<Self, PlatformError> {
        let instance = SingleInstance::acquire(MUTEX_NAME).map_err(|err| match err {
            GuardError::AlreadyRunning => PlatformError::AlreadyRunning,
            GuardError::Win32 => PlatformError::Win32("CreateMutexW"),
        })?;

        let window = HiddenWindow::create().ok_or(PlatformError::Win32("CreateWindowExW"))?;
        // SAFETY: `window.hwnd` is a live window owned by this thread.
        if unsafe { AddClipboardFormatListener(window.hwnd) } == 0 {
            return Err(PlatformError::Win32("AddClipboardFormatListener"));
        }
        let hotkey = hotkey::register(window.hwnd).ok_or(PlatformError::HotkeyUnavailable)?;

        let mut store = Store::new(config.capacity);
        for item in load_pinned(&config.pins_path) {
            store.insert(item);
        }

        let tray = Tray::add(window.hwnd, &tray_tip(hotkey.combo().label(), false));

        Ok(Self {
            window,
            _instance: instance,
            hotkey: Some(hotkey),
            tray,
            store,
            pins_path: config.pins_path,
            images: ImageCache::default(),
            paused: false,
            ignore_seq: None,
            last_seq: clipboard::sequence(),
        })
    }

    /// Which combo actually registered.
    pub fn hotkey_label(&self) -> Option<&'static str> {
        self.hotkey.as_ref().map(|h| h.combo().label())
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Pause/resume capture; the tray tooltip reflects it.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        let label = self.hotkey_label().unwrap_or("hotkey");
        let tip = tray_tip(label, paused);
        if let Some(tray) = self.tray.as_mut() {
            tray.set_tip(&tip);
        }
    }

    /// Pin or unpin a row and persist the pins file.
    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> io::Result<()> {
        self.store.pin(id, pinned);
        self.persist_pins()
    }

    /// Delete one row and persist the pins file.
    pub fn remove(&mut self, id: u64) -> io::Result<()> {
        self.store.remove(id);
        self.images.forget(id);
        self.persist_pins()
    }

    /// Drop every unpinned row and persist.
    pub fn clear_unpinned(&mut self) -> io::Result<()> {
        self.store.clear_unpinned();
        self.images = ImageCache::default();
        self.persist_pins()
    }

    /// Write the pinned items to disk.
    pub fn persist_pins(&self) -> io::Result<()> {
        save_pinned(&self.pins_path, self.store.items())
    }

    /// Put `id` on the clipboard, send Ctrl+V, then restore what was there.
    pub fn paste(&mut self, id: u64) -> bool {
        let Some(previous) = self.write_item(id) else {
            return false;
        };
        if !crate::paste::send_ctrl_v() {
            previous.restore();
            return false;
        }
        PENDING.with(|pending| *pending.borrow_mut() = Some(previous));
        // SAFETY: `hwnd` is ours and the timer posts WM_TIMER to this thread,
        // which is where the restore is safe to run.
        unsafe {
            SetTimer(self.window.hwnd, TIMER_RESTORE, RESTORE_DELAY_MS, None);
        }
        true
    }

    /// Put `id` on the clipboard and leave it there (Enter with Ctrl, or the
    /// row's Copy action). The history does not re-capture our own write.
    pub fn copy(&mut self, id: u64) -> bool {
        self.write_item(id).is_some()
    }

    /// Write one clip to the clipboard. Returns the content it displaced, or
    /// `None` when nothing could be written.
    fn write_item(&mut self, id: u64) -> Option<Previous> {
        let item = self.store.get(id)?.clone();
        let previous = clipboard::capture_previous();
        let wrote = match item.kind {
            ClipKind::Text | ClipKind::Link => {
                item.text.as_deref().is_some_and(clipboard::write_text)
            }
            ClipKind::Files => clipboard::write_files(&item.files),
            ClipKind::Image => self.images.get(id).is_some_and(clipboard::write_image),
        };
        if !wrote {
            return None;
        }
        // Our own write raises WM_CLIPBOARDUPDATE; remember it so we do not
        // capture our own paste as a new history entry.
        self.ignore_seq = Some(clipboard::sequence());
        Some(previous)
    }

    /// Block until something happens. `None` means the loop should end.
    pub fn next_event(&mut self) -> Option<Event> {
        loop {
            match self.take_event() {
                Step::Event(event) => return Some(event),
                Step::Handled => {}
                Step::Empty => {
                    let mut msg: MSG = unsafe { mem::zeroed() };
                    // SAFETY: `msg` is a valid out-parameter; a null hwnd means
                    // "any window of this thread".
                    let ret = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
                    if ret <= 0 {
                        return None;
                    }
                    // SAFETY: `msg` was filled in by GetMessageW.
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
    }

    /// Drain everything that is already waiting, without blocking.
    pub fn poll_events(&mut self) -> Vec<Event> {
        loop {
            let mut msg: MSG = unsafe { mem::zeroed() };
            // SAFETY: `msg` is a valid out-parameter and PM_REMOVE drains it.
            let has = unsafe { PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, PM_REMOVE) };
            if has == 0 {
                break;
            }
            if msg.message == WM_QUIT {
                push(Event::Quit);
                break;
            }
            // SAFETY: `msg` was filled in by PeekMessageW.
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let mut events = Vec::new();
        loop {
            match self.take_event() {
                Step::Event(event) => events.push(event),
                Step::Handled => {}
                Step::Empty => break,
            }
        }
        events
    }

    /// Ask the loop to stop (posts WM_CLOSE to our own window).
    pub fn request_quit(&self) {
        // SAFETY: `hwnd` is ours and still alive.
        unsafe {
            PostMessageW(self.window.hwnd, WM_CLOSE, 0, 0);
        }
    }

    fn take_event(&mut self) -> Step {
        let Some(event) = QUEUE.with(|queue| queue.borrow_mut().pop_front()) else {
            return Step::Empty;
        };
        match event {
            Event::RestoreDue => {
                let pending = PENDING.with(|slot| slot.borrow_mut().take());
                if let Some(previous) = pending {
                    // The restore is itself a clipboard write: remember its
                    // sequence number so the old content does not jump to the
                    // top of the history as if the user had copied it.
                    if previous.restore() {
                        self.ignore_seq = Some(clipboard::sequence());
                    }
                }
                Step::Event(Event::ClipboardRestored)
            }
            Event::ClipboardChanged => self.capture().map_or(Step::Handled, Step::Event),
            Event::TrayMenu => {
                let hwnd = self.window.hwnd;
                let paused = self.paused;
                let picked = self
                    .tray
                    .as_ref()
                    .and_then(|tray| tray.show_menu(hwnd, paused));
                match picked {
                    Some(TrayCommand::TogglePause) => {
                        self.set_paused(!paused);
                        Step::Event(Event::Tray(TrayCommand::TogglePause))
                    }
                    Some(TrayCommand::ClearUnpinned) => {
                        // A failed write must not kill the app; the rows are gone
                        // in memory either way.
                        let _ = self.clear_unpinned();
                        Step::Event(Event::Tray(TrayCommand::ClearUnpinned))
                    }
                    Some(TrayCommand::Quit) => {
                        self.request_quit();
                        Step::Event(Event::Quit)
                    }
                    Some(command) => Step::Event(Event::Tray(command)),
                    None => Step::Handled,
                }
            }
            other => Step::Event(other),
        }
    }

    fn capture(&mut self) -> Option<Event> {
        if self.paused {
            return None;
        }
        let seq = clipboard::sequence();
        if seq == self.last_seq {
            return None;
        }
        self.last_seq = seq;
        if self.ignore_seq.take() == Some(seq) {
            return None;
        }
        let captured = clipboard::capture(now_ms())?;
        let id = self.store.insert(captured.item);
        if let Some(bytes) = captured.image {
            self.images.put(id, bytes);
        }
        Some(Event::ClipCaptured(id))
    }
}

impl Drop for Platform {
    fn drop(&mut self) {
        self.tray = None;
        // The hotkey must go before the window it is bound to.
        self.hotkey = None;
        PENDING.with(|slot| slot.borrow_mut().take());
        // SAFETY: `hwnd` is still alive; `window` is dropped after this body.
        unsafe {
            RemoveClipboardFormatListener(self.window.hwnd);
        }
    }
}

fn tray_tip(hotkey: &str, paused: bool) -> String {
    if paused {
        "Pastebox — paused".to_string()
    } else {
        format!("Pastebox — {hotkey}")
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Show a blocking error dialog. Used when startup fails in a GUI subsystem
/// build, where stderr goes nowhere.
pub fn alert(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let text = to_wide(message);
    let caption = to_wide("Pastebox");
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// `-smoke`: prove the Win32 layer works on this machine.
///
/// Checks the hotkey chain, a clipboard write/read/restore round trip and the
/// single-instance guard. The tray icon is reported but not fatal, because CI
/// sessions do not always have a shell.
pub fn smoke() -> bool {
    let mut ok = true;
    ok &= smoke_hotkey();
    ok &= smoke_clipboard();
    ok &= smoke_single_instance();
    let _ = smoke_tray();
    ok
}

fn smoke_hotkey() -> bool {
    match hotkey::register(ptr::null_mut()) {
        Some(registered) => {
            println!("PASS hotkey: registered {}", registered.combo().label());
            true
        }
        None => {
            println!("FAIL hotkey: Win+Alt+V, Win+Shift+V and Ctrl+Alt+V are all taken");
            false
        }
    }
}

fn smoke_clipboard() -> bool {
    let previous = clipboard::capture_previous();
    let probe = format!("pastebox smoke probe {}", std::process::id());
    if !clipboard::write_text(&probe) {
        println!("FAIL clipboard: write failed");
        return false;
    }
    let read_back = clipboard::capture(0).and_then(|captured| captured.item.text);
    let matched = read_back.as_deref() == Some(probe.as_str());
    if matched {
        println!("PASS clipboard: text round trip ({} chars)", probe.len());
    } else {
        println!("FAIL clipboard: read back {read_back:?}");
    }
    let restored = previous.restore();
    if restored {
        println!("PASS clipboard: previous content restored");
    } else {
        println!("FAIL clipboard: could not restore the previous content");
    }
    matched && restored
}

fn smoke_single_instance() -> bool {
    match SingleInstance::acquire("Local\\Pastebox.SmokeTest") {
        Ok(first) => {
            let second = SingleInstance::acquire("Local\\Pastebox.SmokeTest");
            let blocked = matches!(second, Err(GuardError::AlreadyRunning));
            drop(second);
            drop(first);
            if blocked {
                println!("PASS single-instance: second acquire was refused");
                true
            } else {
                println!("FAIL single-instance: second acquire succeeded");
                false
            }
        }
        Err(err) => {
            println!("FAIL single-instance: first acquire failed ({err:?})");
            false
        }
    }
}

fn smoke_tray() -> bool {
    let Some(window) = HiddenWindow::create() else {
        println!("INFO tray: could not create the hidden window");
        return false;
    };
    match Tray::add(window.hwnd, "Pastebox smoke test") {
        Some(tray) => {
            drop(tray);
            println!("PASS tray: icon added and removed");
            true
        }
        None => {
            println!("INFO tray: Shell_NotifyIcon unavailable in this session");
            false
        }
    }
}

/// Keep the clipboard history alive without a window to draw. Used until the UI
/// crate lands, and by the tray-only fallback.
pub fn run_headless(platform: &mut Platform) -> i32 {
    while let Some(event) = platform.next_event() {
        // No UI yet: the history still fills and the tray menu still works.
        if event == Event::Quit {
            break;
        }
    }
    0
}
