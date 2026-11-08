//! Paste-back: synthesize Ctrl+V into whatever window has focus.

use std::mem::size_of;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};

/// One key event. `up` selects the key-release flag.
fn key(vk: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Send Ctrl down, V down, V up, Ctrl up. Returns true only when all four
/// events were accepted by the input queue.
pub fn send_ctrl_v() -> bool {
    let events = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    let count = u32::try_from(events.len()).unwrap_or(0);
    // SAFETY: `events` is a valid array of `count` fully initialised INPUT
    // structs, and `size_of::<INPUT>()` is the size the API expects.
    let sent = unsafe { SendInput(count, events.as_ptr(), size_of::<INPUT>() as i32) };
    sent == count
}
