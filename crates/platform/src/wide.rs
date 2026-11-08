//! UTF-16 helpers shared by the Win32 modules.
//!
//! Win32 is UTF-16; every string crossing the boundary goes through here so the
//! rest of the crate only ever sees `String` / `&str`.

/// NUL-terminated UTF-16 copy of `s`, ready for a `PCWSTR` parameter.
pub fn to_wide(s: &str) -> Vec<u16> {
    let mut buf: Vec<u16> = s.encode_utf16().collect();
    buf.push(0);
    buf
}

/// Copy `s` into a fixed-size NUL-padded buffer (`szTip`-style fields).
///
/// Truncates on a UTF-16 unit boundary; the buffer is always NUL-terminated
/// because at most `N - 1` units are written.
pub fn to_wide_buf<const N: usize>(s: &str) -> [u16; N] {
    let mut buf = [0u16; N];
    for (n, unit) in s.encode_utf16().enumerate() {
        if n + 1 >= N {
            break;
        }
        buf[n] = unit;
    }
    buf
}

/// Read a NUL-terminated UTF-16 string of at most `max` units from `ptr`.
///
/// # Safety
///
/// `ptr` must point to at least `max` readable `u16` values (fewer is fine only
/// if a NUL terminator is reached first).
pub unsafe fn from_wide(ptr: *const u16, max: usize) -> String {
    if ptr.is_null() || max == 0 {
        return String::new();
    }
    // SAFETY: the caller guarantees `max` readable units; we never index past it.
    let units = unsafe { std::slice::from_raw_parts(ptr, max) };
    let end = units.iter().position(|&u| u == 0).unwrap_or(max);
    String::from_utf16_lossy(&units[..end])
}
