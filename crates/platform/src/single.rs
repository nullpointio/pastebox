//! Single-instance guard: a named kernel mutex held for the process lifetime.

use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

use crate::wide::to_wide;

/// Why the guard could not be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardError {
    /// Another process already holds the mutex.
    AlreadyRunning,
    /// `CreateMutexW` itself failed.
    Win32,
}

/// Owns the mutex handle; dropping it releases the single-instance lock.
pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Create or open `name` (use a `Local\`-prefixed name for a per-session
    /// guard) and report whether this process is the first one.
    pub fn acquire(name: &str) -> Result<Self, GuardError> {
        let wide = to_wide(name);
        // SAFETY: null attributes means default security; `wide` outlives the
        // call. A null return means the mutex could not be created at all.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            return Err(GuardError::Win32);
        }
        // SAFETY: read immediately after the call, as GetLastError requires.
        let last = unsafe { GetLastError() };
        if last == ERROR_ALREADY_EXISTS {
            // SAFETY: we own this handle and are abandoning it.
            unsafe {
                CloseHandle(handle);
            }
            return Err(GuardError::AlreadyRunning);
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed exactly once.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}
