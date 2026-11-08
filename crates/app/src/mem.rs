//! `-mem`: a scripted workload that prints peak RSS, so the memory number in a
//! report is measured rather than estimated.
//!
//! The script mimics a real session: a burst of screenshots copied one after
//! another (each arriving as the 4-byte-per-pixel DIB the clipboard delivers),
//! a tail of text clips, then a few "panel opens" where the rows are walked and
//! the selected image is decoded for the preview pane.
//!
//! It runs on any host: no window, no GPU, no clipboard. The storage path it
//! exercises is exactly the one the app uses, so the peak working set it reports
//! is the memory the same session would cost the real palette.

use std::hint::black_box;

use pastebox_core::{ClipImage, ClipItem, ClipKind, Store};

/// Screenshots in the scripted burst. 1440 × 900 is the common laptop
/// screenshot; as a DIB it is 5.2 MB, which is why twenty-odd of them used to
/// dominate the process.
const IMAGE_COUNT: usize = 60;
const IMAGE_W: u32 = 1440;
const IMAGE_H: u32 = 900;
/// Ordinary text clips mixed into the history.
const TEXT_COUNT: usize = 40;
/// How many times the palette is opened and the list walked.
const FRAMES: usize = 20;
/// Rows visible at once in a 960 × 600 panel.
const VISIBLE_ROWS: usize = 18;

/// Run the scripted session and print the numbers.
pub fn run() {
    let (_, baseline_peak) = rss_kb();
    let mut store = Store::new(500);
    let start = std::time::Instant::now();
    let mut encode_total = std::time::Duration::ZERO;
    let mut encoded_bytes = 0usize;

    for i in 0..IMAGE_COUNT {
        let dib = make_dib(IMAGE_W, IMAGE_H, i as u32);
        let encode_start = std::time::Instant::now();
        let image = ClipImage::from_dib(&dib);
        encode_total += encode_start.elapsed();
        // The raw block dies here: after this line the only copy of the picture
        // is the encoded one.
        drop(dib);
        encoded_bytes += image.as_ref().map_or(0, ClipImage::encoded_len);
        let mut item = ClipItem::new(
            ClipKind::Image,
            None,
            Vec::new(),
            image.as_ref().map_or(0, ClipImage::encoded_len),
            1_700_000_000_000 + i as u64,
        )
        .with_preview(format!(
            "Screenshot 2026-11-08 14{:02}{:02}.png",
            i / 60,
            i % 60
        ));
        if let Some(image) = image {
            item = item.with_image(image);
        }
        store.insert(item);
    }

    for i in 0..TEXT_COUNT {
        let body = format!(
            "clip {i}: {}",
            "the quick brown fox jumps over the lazy dog ".repeat(24)
        );
        let len = body.len();
        store.insert(ClipItem::new(
            ClipKind::Text,
            Some(body),
            Vec::new(),
            len,
            1_700_000_500_000 + i as u64,
        ));
    }

    // "Open the panel" repeatedly: walk the visible window of rows, touch every
    // thumbnail, and decode whichever image is selected for the preview pane.
    for frame in 0..FRAMES {
        let items = store.items();
        if items.is_empty() {
            break;
        }
        for row in 0..VISIBLE_ROWS {
            let index = (frame * 3 + row) % items.len();
            let clip = &items[index];
            if let Some(image) = clip.image.as_ref() {
                black_box(image.thumb().rgba().len());
            }
        }
        if let Some(clip) = items.iter().find(|c| c.kind == ClipKind::Image) {
            if let Some((w, h, rgba)) = clip.image.as_ref().and_then(ClipImage::decode_rgba) {
                black_box((w, h, rgba.len()));
            }
        }
    }

    let elapsed = start.elapsed();
    let (steady_kb, peak_kb) = rss_kb();
    let payload = store.payload_bytes();

    println!("pastebox -mem: scripted session");
    println!(
        "  clips={} images={} retained={:.1} MB",
        store.len(),
        store
            .items()
            .iter()
            .filter(|c| c.kind == ClipKind::Image)
            .count(),
        payload as f64 / (1024.0 * 1024.0)
    );
    println!(
        "  encoded {} screenshots: avg {:.0} KB each from {:.1} MB of DIBs ({:.2} s encode)",
        IMAGE_COUNT,
        encoded_bytes as f64 / IMAGE_COUNT as f64 / 1024.0,
        (IMAGE_COUNT * IMAGE_W as usize * IMAGE_H as usize * 4) as f64 / (1024.0 * 1024.0),
        encode_total.as_secs_f64()
    );
    println!("  steady RSS {:.1} MB", steady_kb as f64 / 1024.0);
    println!(
        "  peak RSS {:.1} MB (before the session {:.1} MB), scripted run {:.2} s",
        peak_kb as f64 / 1024.0,
        baseline_peak as f64 / 1024.0,
        elapsed.as_secs_f64()
    );
    println!("PEAK_RSS_MB={:.1}", peak_kb as f64 / 1024.0);
}

/// A `BITMAPINFOHEADER` DIB of `width × height`, 32 bpp bottom-up, filled with a
/// screenshot-looking gradient plus banded "text". Compressible, like a real
/// screenshot, but every seed differs so the rows are distinct clips.
fn make_dib(width: u32, height: u32, seed: u32) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut dib = vec![0u8; 40 + stride * height as usize];
    dib[0..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&(width as i32).to_le_bytes());
    dib[8..12].copy_from_slice(&(height as i32).to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    for y in 0..height as usize {
        let row = 40 + (height as usize - 1 - y) * stride;
        for x in 0..width as usize {
            let at = row + x * 4;
            // Flat background with a few horizontal bands of "text".
            let band = (y / 24) % 7;
            let texty = band == 3 && (x + seed as usize * 7) % 37 < 18;
            let (r, g, b) = if texty {
                (32, 34, 40)
            } else {
                let fade = ((x * 255) / width as usize) as u8;
                (236 - fade / 8, 238 - fade / 8, 244 - fade / 10)
            };
            dib[at] = b;
            dib[at + 1] = g;
            dib[at + 2] = r;
            dib[at + 3] = 0xFF;
        }
    }
    dib
}

/// `(current, peak)` working set in KiB.
#[cfg(target_os = "linux")]
fn rss_kb() -> (u64, u64) {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return (0, 0);
    };
    let read = |key: &str| -> u64 {
        status
            .lines()
            .find(|line| line.starts_with(key))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    };
    (read("VmRSS:"), read("VmHWM:"))
}

#[cfg(windows)]
fn rss_kb() -> (u64, u64) {
    use std::mem::size_of;

    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: the struct is plain data and `cb` is set to its own size, which
    // is the contract GetProcessMemoryInfo documents.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the current process pseudo-handle is always valid and the
    // out-parameter is a correctly sized, initialised struct.
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        return (0, 0);
    }
    (
        counters.WorkingSetSize as u64 / 1024,
        counters.PeakWorkingSetSize as u64 / 1024,
    )
}

#[cfg(not(any(target_os = "linux", windows)))]
fn rss_kb() -> (u64, u64) {
    (0, 0)
}
