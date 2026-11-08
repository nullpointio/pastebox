//! Notification-area icon and its context menu.

use std::mem::size_of;
use std::ptr;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetSystemMetrics, LoadImageW,
    PostMessageW, SetForegroundWindow, TrackPopupMenu, HICON, IMAGE_ICON, LR_DEFAULTCOLOR,
    MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_NULL,
};

use crate::wide::to_wide_buf;

/// Callback message delivered to the hidden window for tray mouse events.
pub const TRAY_MSG: u32 = WM_APP + 1;
/// Icon id inside our window; arbitrary but stable.
pub const TRAY_ID: u32 = 1;

/// What the user picked from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    TogglePause,
    ClearUnpinned,
    Quit,
}

impl TrayCommand {
    fn id(self) -> usize {
        match self {
            Self::Open => 1,
            Self::TogglePause => 2,
            Self::ClearUnpinned => 3,
            Self::Quit => 4,
        }
    }

    fn from_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::Open),
            2 => Some(Self::TogglePause),
            3 => Some(Self::ClearUnpinned),
            4 => Some(Self::Quit),
            _ => None,
        }
    }
}

/// The app icon embedded in the executable at build time, sized for the
/// notification area. Resource id 1 is the `RT_GROUP_ICON` declared in
/// `assets/pastebox.rc`; Windows picks the frame that fits.
fn app_icon() -> HICON {
    // SAFETY: a null module handle means "this executable"; the id and flags are
    // the documented ones and the returned handle is only read by Shell_NotifyIconW.
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON);
        LoadImageW(
            GetModuleHandleW(ptr::null()),
            1 as *const u16,
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
}

pub struct Tray {
    data: NOTIFYICONDATAW,
    added: bool,
}

impl Tray {
    /// Add the icon. Returns `None` when there is no shell to talk to (for
    /// example a service session), which is not fatal for the rest of the app.
    pub fn add(hwnd: HWND, tip: &str) -> Option<Self> {
        // SAFETY: NOTIFYICONDATAW is a plain C struct with a union field; all
        // zeroes is a valid starting state and every field we care about is
        // assigned below.
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = u32::try_from(size_of::<NOTIFYICONDATAW>()).ok()?;
        data.hWnd = hwnd;
        data.uID = TRAY_ID;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MSG;
        // `app_icon` loads our own compiled-in icon (resource id 1 in
        // assets/pastebox.rc) at the small-icon metric, so the tray matches the
        // title bar and taskbar.
        data.hIcon = app_icon();
        if data.hIcon.is_null() {
            return None;
        }
        data.szTip = to_wide_buf(tip);
        // SAFETY: `data` is fully initialised and lives across the call.
        if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
            return None;
        }
        Some(Self { data, added: true })
    }

    /// Update the tooltip (used to show the paused state).
    pub fn set_tip(&mut self, tip: &str) -> bool {
        if !self.added {
            return false;
        }
        self.data.szTip = to_wide_buf(tip);
        self.data.uFlags = NIF_TIP;
        // SAFETY: `data` describes the icon we added above.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.data) != 0 }
    }

    /// Show the context menu at the cursor and return the chosen command.
    /// Blocks until the menu closes, so call it from the message loop, never
    /// from inside the window procedure.
    pub fn show_menu(&self, hwnd: HWND, paused: bool) -> Option<TrayCommand> {
        // SAFETY: menu handles are owned by the calling thread.
        let menu = unsafe { CreatePopupMenu() };
        if menu.is_null() {
            return None;
        }

        let pause_label = if paused {
            "Resume history"
        } else {
            "Pause history"
        };
        let items = [
            (TrayCommand::Open, "Open"),
            (TrayCommand::TogglePause, pause_label),
            (TrayCommand::ClearUnpinned, "Clear unpinned"),
            (TrayCommand::Quit, "Quit"),
        ];
        for (index, (command, label)) in items.iter().enumerate() {
            if index == 3 {
                // SAFETY: the menu is live; a separator needs no text.
                unsafe { AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null()) };
            }
            let wide = to_wide_buf::<64>(label);
            // SAFETY: `wide` is NUL-terminated and outlives the call.
            unsafe { AppendMenuW(menu, MF_STRING, command.id(), wide.as_ptr()) };
        }

        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `point` is a valid out-parameter.
        unsafe { GetCursorPos(&mut point) };
        // A tray menu needs its owner in the foreground or it will not dismiss
        // when the user clicks elsewhere; failure here is harmless.
        unsafe {
            SetForegroundWindow(hwnd);
        }
        // SAFETY: `menu` is live, `hwnd` is ours, and TPM_RETURNCMD makes the
        // return value the selected id instead of posting WM_COMMAND.
        let picked = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            )
        };
        // SAFETY: dismiss the menu's leftover messages, then free the menu.
        unsafe {
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu);
        }
        TrayCommand::from_id(picked as u32)
    }

    /// Remove the icon from the notification area.
    pub fn remove(&mut self) {
        if self.added {
            // SAFETY: `data` still identifies the icon we added.
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &self.data);
            }
            self.added = false;
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}
