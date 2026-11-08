//! Global hotkey registration with an explicit fallback chain.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_V,
};

/// Id shared by registration and the `WM_HOTKEY` message.
pub const HOTKEY_ID: i32 = 0x5042; // "PB"

/// The combos we try, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyCombo {
    WinAltV,
    WinShiftV,
    CtrlAltV,
}

impl HotkeyCombo {
    /// Fallback order: the documented default first, then two progressively
    /// less likely collisions.
    pub const FALLBACK_ORDER: [Self; 3] = [Self::WinAltV, Self::WinShiftV, Self::CtrlAltV];

    pub fn label(self) -> &'static str {
        match self {
            Self::WinAltV => "Win+Alt+V",
            Self::WinShiftV => "Win+Shift+V",
            Self::CtrlAltV => "Ctrl+Alt+V",
        }
    }

    fn modifiers(self) -> u32 {
        match self {
            Self::WinAltV => MOD_WIN | MOD_ALT,
            Self::WinShiftV => MOD_WIN | MOD_SHIFT,
            Self::CtrlAltV => MOD_CONTROL | MOD_ALT,
        }
    }
}

/// A registered hotkey; unregisters itself on drop.
pub struct Registered {
    combo: HotkeyCombo,
    hwnd: HWND,
}

impl Registered {
    pub fn combo(&self) -> HotkeyCombo {
        self.combo
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: the id/hwnd pair is exactly what we registered successfully.
        unsafe {
            UnregisterHotKey(self.hwnd, HOTKEY_ID);
        }
    }
}

/// Try each combo in [`HotkeyCombo::FALLBACK_ORDER`] and return the first one
/// the system accepts. `hwnd` may be null, which binds the hotkey to the calling
/// thread's message queue instead of a window.
pub fn register(hwnd: HWND) -> Option<Registered> {
    for combo in HotkeyCombo::FALLBACK_ORDER {
        // SAFETY: no memory is involved; a taken combo simply fails with 0.
        let ok = unsafe {
            RegisterHotKey(
                hwnd,
                HOTKEY_ID,
                combo.modifiers() | MOD_NOREPEAT,
                u32::from(VK_V),
            )
        };
        if ok != 0 {
            return Some(Registered { combo, hwnd });
        }
    }
    None
}
