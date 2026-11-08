//! Clipboard read/write.
//!
//! Reads `CF_UNICODETEXT`, `CF_HDROP` (file lists) and `CF_DIB` / `CF_DIBV5` /
//! `PNG`. Images are never decoded: only the header is parsed for the pixel
//! dimensions, and the raw block is kept aside (bounded, in memory only) so the
//! item can be put back on the clipboard later.
//!
//! Every read copies out of the clipboard's `HGLOBAL` and releases the lock
//! before the clipboard is closed; the clipboard owns its handles, so nothing
//! here frees them.

use std::mem::size_of;
use std::ptr;
use std::thread;
use std::time::Duration;

use pastebox_core::{ClipItem, ClipKind};
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, POINT};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_HDROP, CF_UNICODETEXT};
use windows_sys::Win32::UI::Shell::{DragQueryFileW, DROPFILES};

use crate::wide::from_wide;

/// Largest text clip we keep (UTF-16 units). Anything bigger is ignored rather
/// than half-stored; the product never promises to hold a novel.
const MAX_TEXT_UNITS: usize = 2 * 1024 * 1024;
/// Largest raw image block we keep for paste-back.
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
/// Largest image we are willing to snapshot for clipboard restoration.
const MAX_SNAPSHOT_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Upper bound on files taken from one `CF_HDROP`.
const MAX_FILES: u32 = 512;

const OPEN_RETRIES: usize = 6;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(15);

/// `PNG` as a NUL-terminated wide literal; the registered id is looked up at
/// runtime because it is assigned per-session.
const PNG_NAME: [u16; 4] = [b'P' as u16, b'N' as u16, b'G' as u16, 0];

/// A clipboard read that produced a row, plus the raw image bytes (if any) that
/// the model deliberately does not carry.
pub struct Captured {
    pub item: ClipItem,
    pub image: Option<Vec<u8>>,
}

/// Keeps the clipboard open for the duration of a read/write and always closes
/// it, including on early return.
struct Open {
    _private: (),
}

impl Open {
    fn acquire() -> Option<Self> {
        for _ in 0..OPEN_RETRIES {
            // SAFETY: no preconditions; a null owner associates the clipboard
            // with the calling thread.
            if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
                return Some(Self { _private: () });
            }
            // Another process is holding the clipboard; retry briefly.
            thread::sleep(OPEN_RETRY_DELAY);
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        // SAFETY: pairs with the successful OpenClipboard in `acquire`.
        unsafe {
            CloseClipboard();
        }
    }
}

/// Copy `len` bytes out of a clipboard `HGLOBAL`, bounded by `max`.
///
/// The handle stays owned by the clipboard: it is locked, copied and unlocked,
/// never freed.
///
/// # Safety
///
/// The clipboard must be open on this thread and `handle` must come from
/// `GetClipboardData` (or be null).
unsafe fn read_block(handle: HANDLE, max: usize) -> Option<Vec<u8>> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: `handle` is a live global block owned by the open clipboard.
    let size = unsafe { GlobalSize(handle) }.min(max);
    if size == 0 {
        return None;
    }
    // SAFETY: same handle; GlobalLock pins the block for this thread.
    let ptr = unsafe { GlobalLock(handle) }.cast::<u8>();
    if ptr.is_null() {
        return None;
    }
    // SAFETY: GlobalLock returned a pointer to at least `size` readable bytes.
    let out = unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec();
    // SAFETY: balances the GlobalLock above; the clipboard keeps ownership.
    unsafe {
        GlobalUnlock(handle);
    }
    Some(out)
}

/// # Safety
///
/// The clipboard must be open on this thread.
unsafe fn read_text() -> Option<String> {
    // SAFETY: `CF_UNICODETEXT` is only requested when it is available.
    let handle = unsafe { GetClipboardData(u32::from(CF_UNICODETEXT)) };
    if handle.is_null() {
        return None;
    }
    // SAFETY: a CF_UNICODETEXT handle is a NUL-terminated UTF-16 string.
    let units = unsafe { GlobalSize(handle) } / 2;
    let max = units.min(MAX_TEXT_UNITS);
    let ptr = unsafe { GlobalLock(handle) }.cast::<u16>();
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the lock gives us `max` readable units (bounded by the block size).
    let text = unsafe { from_wide(ptr, max) };
    // SAFETY: balances the GlobalLock above.
    unsafe {
        GlobalUnlock(handle);
    }
    Some(text)
}

/// # Safety
///
/// The clipboard must be open on this thread.
unsafe fn read_files() -> Option<Vec<String>> {
    // SAFETY: `CF_HDROP` is only requested when it is available.
    let handle = unsafe { GetClipboardData(u32::from(CF_HDROP)) };
    if handle.is_null() {
        return None;
    }
    // Passing a null buffer with cch = 0 asks for the count / length.
    // SAFETY: `handle` is a live HDROP while the clipboard is open.
    let count = unsafe { DragQueryFileW(handle, u32::MAX, ptr::null_mut(), 0) };
    if count == 0 {
        return None;
    }
    let mut files: Vec<String> = Vec::new();
    for index in 0..count.min(MAX_FILES) {
        // SAFETY: `index` is below the count reported above.
        let len = unsafe { DragQueryFileW(handle, index, ptr::null_mut(), 0) };
        if len == 0 {
            continue;
        }
        let mut buf = vec![0u16; len as usize + 1];
        // SAFETY: the buffer is `len + 1` units, which is what DragQueryFileW
        // writes at most (path + NUL).
        let written = unsafe { DragQueryFileW(handle, index, buf.as_mut_ptr(), buf.len() as u32) };
        if written == 0 {
            continue;
        }
        buf.truncate(written as usize);
        let path = String::from_utf16_lossy(&buf);
        if !path.is_empty() && !files.contains(&path) {
            files.push(path);
        }
    }
    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

/// `BITMAPINFOHEADER`: `biWidth` / `biHeight` at offsets 4 and 8 (signed).
fn dib_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let w = i32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let h = i32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let (w, h) = (w.unsigned_abs(), h.unsigned_abs());
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

/// PNG `IHDR`: big-endian width at 16, height at 20.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

/// The id of the registered `PNG` format, or 0 when registration failed.
fn png_format() -> u32 {
    // SAFETY: `PNG_NAME` is NUL-terminated and outlives the call.
    unsafe { RegisterClipboardFormatW(PNG_NAME.as_ptr()) }
}

/// # Safety
///
/// The clipboard must be open on this thread.
unsafe fn read_image(now_ms: u64) -> Option<Captured> {
    let png = png_format();
    let candidates: Vec<u32> = [u32::from(CF_DIB), u32::from(CF_DIBV5), png]
        .into_iter()
        .filter(|&format| format != 0)
        .collect();
    for format in candidates {
        // SAFETY: the clipboard is open; a missing format yields a null handle.
        let handle = unsafe { GetClipboardData(format) };
        // SAFETY: `handle` came from the open clipboard.
        let Some(bytes) = (unsafe { read_block(handle, MAX_IMAGE_BYTES) }) else {
            continue;
        };
        let dims = if format == png {
            png_dimensions(&bytes)
        } else {
            dib_dimensions(&bytes)
        };
        let preview = dims.map_or_else(|| "Image".to_string(), |(w, h)| format!("Image {w}×{h}"));
        let item = ClipItem::new(ClipKind::Image, None, Vec::new(), bytes.len(), now_ms)
            .with_preview(preview);
        return Some(Captured {
            item,
            image: Some(bytes),
        });
    }
    None
}

/// Current clipboard sequence number; changes on every clipboard write by any
/// process, which makes it a cheap "did we cause this?" check.
pub fn sequence() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetClipboardSequenceNumber() }
}

fn available(format: u16) -> bool {
    // SAFETY: no preconditions.
    unsafe { IsClipboardFormatAvailable(u32::from(format)) != 0 }
}

/// Read the clipboard into a row, or `None` when it holds nothing we model.
pub fn capture(now_ms: u64) -> Option<Captured> {
    let _open = Open::acquire()?;

    // Files beat text: Explorer puts both on the clipboard for a file copy.
    if available(CF_HDROP) {
        // SAFETY: the clipboard is open on this thread.
        if let Some(files) = unsafe { read_files() } {
            let bytes = total_file_bytes(&files);
            let item = ClipItem::new(ClipKind::Files, None, files, bytes, now_ms);
            return Some(Captured { item, image: None });
        }
    }

    if available(CF_UNICODETEXT) {
        // SAFETY: the clipboard is open on this thread.
        if let Some(text) = unsafe { read_text() } {
            if !text.trim().is_empty() {
                let kind = if is_link(&text) {
                    ClipKind::Link
                } else {
                    ClipKind::Text
                };
                let len = text.len();
                let item = ClipItem::new(kind, Some(text), Vec::new(), len, now_ms);
                return Some(Captured { item, image: None });
            }
        }
    }

    // SAFETY: the clipboard is open on this thread.
    unsafe { read_image(now_ms) }
}

fn is_link(text: &str) -> bool {
    let t = text.trim();
    if t.chars().any(char::is_whitespace) {
        return false;
    }
    ["http://", "https://", "ftp://", "mailto:"]
        .iter()
        .any(|scheme| t.len() > scheme.len() && t.starts_with(scheme))
}

fn total_file_bytes(files: &[String]) -> usize {
    files
        .iter()
        .map(|f| std::fs::metadata(f).map_or(0, |m| usize::try_from(m.len()).unwrap_or(0)))
        .sum()
}

/// Put `bytes` on the clipboard as `format`, transferring ownership of the
/// global block to the clipboard.
fn alloc_and_set(format: u32, bytes: &[u8]) -> bool {
    // SAFETY: a fresh moveable block; size 0 is not a valid allocation, so we
    // always ask for at least one byte.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)) };
    if handle.is_null() {
        return false;
    }
    // SAFETY: `handle` is a fresh block of at least `bytes.len()` bytes.
    let dst = unsafe { GlobalLock(handle) }.cast::<u8>();
    if dst.is_null() {
        // SAFETY: we still own the block because it never reached the clipboard.
        unsafe {
            GlobalFree(handle);
        }
        return false;
    }
    // SAFETY: `dst` points to the block's full size and `bytes` is readable.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
        GlobalUnlock(handle);
    }
    // SAFETY: with the clipboard open, SetClipboardData takes ownership of the
    // handle on success; on failure the handle is still ours.
    let ok = unsafe { SetClipboardData(format, handle) };
    if ok.is_null() {
        // SAFETY: the clipboard refused the handle, so we must free it.
        unsafe {
            GlobalFree(handle);
        }
        return false;
    }
    true
}

/// Write plain text (`CF_UNICODETEXT`).
pub fn write_text(text: &str) -> bool {
    let Some(_open) = Open::acquire() else {
        return false;
    };
    let mut units: Vec<u16> = text.encode_utf16().collect();
    units.push(0);
    let mut bytes = Vec::with_capacity(units.len() * 2);
    for unit in &units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    // SAFETY: the clipboard is open and owned by this thread. Emptying it first
    // drops every stale format from the previous owner.
    unsafe {
        EmptyClipboard();
    }
    let ok = alloc_and_set(u32::from(CF_UNICODETEXT), &bytes);
    // Do not keep a second copy of the content alive in freed heap memory.
    bytes.fill(0);
    units.fill(0);
    ok
}

/// Write a file list (`CF_HDROP`) with a `DROPFILES` header.
pub fn write_files(paths: &[String]) -> bool {
    if paths.is_empty() {
        return false;
    }
    let Some(_open) = Open::acquire() else {
        return false;
    };
    let mut units: Vec<u16> = Vec::new();
    for path in paths {
        units.extend(path.encode_utf16());
        units.push(0);
    }
    units.push(0); // the list itself is double-NUL terminated

    let header = DROPFILES {
        pFiles: u32::try_from(size_of::<DROPFILES>()).unwrap_or(0),
        pt: POINT { x: 0, y: 0 },
        fNC: 0,
        fWide: 1,
    };
    let mut bytes = Vec::with_capacity(size_of::<DROPFILES>() + units.len() * 2);
    // SAFETY: DROPFILES is a repr(C) plain-old-data struct, so its bytes are
    // readable for exactly `size_of::<DROPFILES>()`.
    bytes.extend_from_slice(unsafe {
        std::slice::from_raw_parts(
            (&header as *const DROPFILES).cast::<u8>(),
            size_of::<DROPFILES>(),
        )
    });
    for unit in &units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    // SAFETY: the clipboard is open and owned by this thread.
    unsafe {
        EmptyClipboard();
    }
    alloc_and_set(u32::from(CF_HDROP), &bytes)
}

/// Put a raw `CF_DIB` block back on the clipboard.
pub fn write_image(dib: &[u8]) -> bool {
    let Some(_open) = Open::acquire() else {
        return false;
    };
    // SAFETY: the clipboard is open and owned by this thread.
    unsafe {
        EmptyClipboard();
    }
    alloc_and_set(u32::from(CF_DIB), dib)
}

/// Drop everything from the clipboard.
pub fn clear() -> bool {
    let Some(_open) = Open::acquire() else {
        return false;
    };
    // SAFETY: the clipboard is open and owned by this thread.
    unsafe { EmptyClipboard() != 0 }
}

/// What was on the clipboard before we replaced it.
pub enum Previous {
    Empty,
    Saved(Snapshot),
}

impl Previous {
    /// Put the previous content back (or leave the clipboard empty, which is a
    /// faithful restoration of an empty clipboard).
    pub fn restore(&self) -> bool {
        match self {
            Self::Empty => clear(),
            Self::Saved(snapshot) => snapshot.restore(),
        }
    }
}

/// A bounded copy of the clipboard, taken before paste-back overwrites it.
pub struct Snapshot {
    text: Option<String>,
    files: Option<Vec<String>>,
    dib: Option<Vec<u8>>,
}

impl Snapshot {
    fn restore(&self) -> bool {
        if let Some(files) = &self.files {
            if !files.is_empty() {
                return write_files(files);
            }
        }
        if let Some(text) = &self.text {
            return write_text(text);
        }
        if let Some(dib) = &self.dib {
            return write_image(dib);
        }
        clear()
    }
}

/// Snapshot the clipboard now. Best effort: a format we cannot read is simply
/// not restored, and images over [`MAX_SNAPSHOT_IMAGE_BYTES`] are dropped.
pub fn capture_previous() -> Previous {
    let Some(_open) = Open::acquire() else {
        return Previous::Empty;
    };
    let text = if available(CF_UNICODETEXT) {
        // SAFETY: the clipboard is open on this thread.
        unsafe { read_text() }
    } else {
        None
    };
    let files = if available(CF_HDROP) {
        // SAFETY: the clipboard is open on this thread.
        unsafe { read_files() }
    } else {
        None
    };
    let dib = if available(CF_DIB) {
        // SAFETY: the clipboard is open on this thread.
        unsafe {
            read_block(
                GetClipboardData(u32::from(CF_DIB)),
                MAX_SNAPSHOT_IMAGE_BYTES,
            )
        }
    } else {
        None
    };
    if text.is_none() && files.is_none() && dib.is_none() {
        Previous::Empty
    } else {
        Previous::Saved(Snapshot { text, files, dib })
    }
}

/// Raw image blocks kept beside the model, keyed by clip id, so an image row can
/// be pasted back. Bounded by both entry count and total bytes.
#[derive(Default)]
pub struct ImageCache {
    entries: std::collections::VecDeque<(u64, Vec<u8>)>,
    bytes: usize,
}

const MAX_CACHED_IMAGES: usize = 24;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;

impl ImageCache {
    pub fn put(&mut self, id: u64, bytes: Vec<u8>) {
        self.forget(id);
        self.bytes += bytes.len();
        self.entries.push_back((id, bytes));
        while self.entries.len() > MAX_CACHED_IMAGES || self.bytes > MAX_CACHE_BYTES {
            match self.entries.pop_front() {
                Some((_, dropped)) => self.bytes -= dropped.len(),
                None => break,
            }
        }
    }

    pub fn get(&self, id: u64) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, bytes)| bytes.as_slice())
    }

    pub fn forget(&mut self, id: u64) {
        if let Some(pos) = self.entries.iter().position(|(key, _)| *key == id) {
            if let Some((_, dropped)) = self.entries.remove(pos) {
                self.bytes -= dropped.len();
            }
        }
    }
}
