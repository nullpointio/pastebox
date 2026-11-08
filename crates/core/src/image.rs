//! Image payloads: PNG bytes plus a small RGBA thumbnail.
//!
//! A clipboard image used to be kept as the raw DIB the clipboard handed over —
//! four bytes per pixel, so a 4K screenshot cost ~33 MB in memory for as long as
//! it lived in the history. This module keeps the *encoded* bytes instead (PNG,
//! typically 5–15x smaller for screenshots) and derives a thumbnail the rows can
//! draw without ever touching the full image. The full pixels are decoded only
//! when the image is previewed or pasted.
//!
//! Both directions stream one row at a time, so neither encoding a clipboard DIB
//! nor decoding for paste-back ever materialises a second full-size buffer.
//!
//! Pure Rust: no Win32, no egui, no I/O beyond an in-memory `Cursor`.

use std::io::{Cursor, Write};
use std::sync::Arc;

use png::{BitDepth, ColorType, Decoder, Encoder, Filter, Info, Transformations};

/// A text body longer than this is capped when it enters the history.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Encoded (PNG) bytes kept for one image. Larger images keep their row and
/// thumbnail but lose the pasteable payload.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Longest side of a row thumbnail, in pixels. 96 px is crisp at 2x for the
/// 32 px row chip and cheap for the 300 px preview plate.
pub const THUMB_MAX_SIDE: u32 = 96;
/// Refuse absurd dimensions outright: a header claiming 100 000 × 100 000 would
/// otherwise size the paste-back buffer at 40 GB.
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// How an image arrived, for the rare caller that needs to know. Everything we
/// keep is PNG once it is in the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOrigin {
    /// Re-encoded from a clipboard `CF_DIB` / `CF_DIBV5`.
    Dib,
    /// Taken from the clipboard's registered `PNG` format as-is.
    Png,
}

/// A small RGBA8 copy of an image, at most [`THUMB_MAX_SIDE`] pixels on its
/// longest side, kept so a row can show the picture without decoding the full
/// payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    width: u32,
    height: u32,
    /// Shared so cloning an item (every history snapshot) copies a pointer, not
    /// the pixels.
    rgba: Arc<[u8]>,
}

impl Thumbnail {
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Tightly packed RGBA8, `width * height * 4` bytes.
    #[must_use]
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Bytes this thumbnail costs.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}

/// One image clip's payload: the encoded bytes, the pixel dimensions and a
/// thumbnail. `encoded` is empty when the payload was dropped for being over
/// [`MAX_IMAGE_BYTES`]; the row and its thumbnail survive so the history still
/// shows what was copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipImage {
    /// Shared for the same reason as the thumbnail: snapshotting the history
    /// must not duplicate megabytes of PNG.
    encoded: Arc<[u8]>,
    origin: ImageOrigin,
    dims: (u32, u32),
    thumb: Thumbnail,
}

impl ClipImage {
    /// Convert a clipboard `CF_DIB` / `CF_DIBV5` block into encoded bytes plus a
    /// thumbnail. `None` when the header is not a layout we can read (exotic
    /// bit depths, RLE compression) — the caller keeps the row without a
    /// pasteable payload rather than guessing at pixels.
    #[must_use]
    pub fn from_dib(dib: &[u8]) -> Option<Self> {
        let header = DibHeader::parse(dib)?;
        let mut image = assemble(header.width, header.height, |y, out| {
            let src = header.row(dib, y)?;
            header.to_rgba(src, out);
            Some(())
        })?;
        image.origin = ImageOrigin::Dib;
        Some(image)
    }

    /// Take a clipboard `PNG` block as-is, validating it and deriving a
    /// thumbnail. `None` when it is not a readable PNG.
    #[must_use]
    pub fn from_png(bytes: &[u8]) -> Option<Self> {
        let info = png_info(bytes)?;
        check_pixels(info.width, info.height)?;
        let mut decoder = Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let color = reader.output_color_type().0;
        let mut image = assemble(info.width, info.height, |_, out| {
            let row = reader.next_row().ok()??;
            expand_row(row.data(), color, out);
            Some(())
        })?;
        image.origin = ImageOrigin::Png;
        Some(image)
    }

    /// A row for an image whose payload could not be encoded: dimensions and a
    /// 1×1 placeholder thumbnail so rendering code has something to draw.
    #[must_use]
    pub fn placeholder(dims: (u32, u32)) -> Self {
        Self {
            encoded: Arc::from(&[][..]),
            origin: ImageOrigin::Dib,
            dims,
            thumb: Thumbnail {
                width: 1,
                height: 1,
                rgba: Arc::from(&[0, 0, 0, 0][..]),
            },
        }
    }

    /// Pixel dimensions, `(width, height)`.
    #[must_use]
    pub fn dims(&self) -> (u32, u32) {
        self.dims
    }

    #[must_use]
    pub fn thumb(&self) -> &Thumbnail {
        &self.thumb
    }

    #[must_use]
    pub fn origin(&self) -> ImageOrigin {
        self.origin
    }

    /// Encoded (PNG) bytes; `0` when the payload was dropped.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        self.encoded.len()
    }

    /// Whether the pixels are still here to paste.
    #[must_use]
    pub fn has_payload(&self) -> bool {
        !self.encoded.is_empty()
    }

    /// Encoded bytes plus thumbnail: what this image really costs in memory.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.encoded.len() + self.thumb.bytes()
    }

    /// Drop the pasteable payload, keeping dimensions and thumbnail. Used when
    /// the encoded image is over [`MAX_IMAGE_BYTES`].
    pub fn drop_payload(&mut self) {
        self.encoded = Arc::from(&[][..]);
    }

    /// Rebuild a `CF_DIB` block (32-bit BI_RGB, bottom-up) for paste-back.
    /// `None` when the payload was dropped or no longer decodes.
    #[must_use]
    pub fn to_dib(&self) -> Option<Vec<u8>> {
        if self.encoded.is_empty() {
            return None;
        }
        let info = png_info(&self.encoded)?;
        let (width, height) = (info.width, info.height);
        check_pixels(width, height)?;
        let stride = width as usize * 4;
        let mut out = vec![0u8; DIB_HEADER_LEN + stride * height as usize];
        write_dib_header(&mut out, width, height, stride);
        self.for_each_row(|y, rgba| {
            // DIB rows run bottom-up unless the height is negative.
            let dst = DIB_HEADER_LEN + (height - 1 - y) as usize * stride;
            let target = out.get_mut(dst..dst + stride)?;
            copy_bgra(rgba, target);
            Some(())
        })?;
        Some(out)
    }

    /// Decode the whole image to tightly packed RGBA8, top-down. Only for the
    /// one row the preview pane is showing; callers must not cache the result.
    #[must_use]
    pub fn decode_rgba(&self) -> Option<(u32, u32, Vec<u8>)> {
        if self.encoded.is_empty() {
            return None;
        }
        let (width, height) = self.dims;
        let stride = width as usize * 4;
        let mut out = vec![0u8; stride * height as usize];
        self.for_each_row(|y, rgba| {
            let start = y as usize * stride;
            let target = out.get_mut(start..start + stride)?;
            target.copy_from_slice(rgba);
            Some(())
        })?;
        Some((width, height, out))
    }

    /// Walk every scanline of the stored PNG as RGBA8, top-down.
    fn for_each_row<F>(&self, mut visit: F) -> Option<()>
    where
        F: FnMut(u32, &[u8]) -> Option<()>,
    {
        let info = png_info(&self.encoded)?;
        check_pixels(info.width, info.height)?;
        let mut decoder = Decoder::new(Cursor::new(&self.encoded));
        decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let color = reader.output_color_type().0;
        let mut rgba = vec![0u8; info.width as usize * 4];
        for y in 0..info.height {
            let row = reader.next_row().ok()??;
            expand_row(row.data(), color, &mut rgba);
            visit(y, &rgba)?;
        }
        Some(())
    }
}

const DIB_HEADER_LEN: usize = 40;

fn write_dib_header(out: &mut [u8], width: u32, height: u32, stride: usize) {
    out[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
    out[4..8].copy_from_slice(&(width as i32).to_le_bytes()); // biWidth
    out[8..12].copy_from_slice(&(height as i32).to_le_bytes()); // biHeight (bottom-up)
    out[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
    out[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
                                                       // biCompression = BI_RGB, biSizeImage, then zeroed resolution/clr fields
    out[20..24].copy_from_slice(&((stride * height as usize) as u32).to_le_bytes());
}

fn copy_bgra(rgba: &[u8], out: &mut [u8]) {
    for (src, dst) in rgba.chunks_exact(4).zip(out.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
}

fn check_pixels(width: u32, height: u32) -> Option<()> {
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return None;
    }
    Some(())
}

/// Dimensions of a PNG without decoding the pixels.
fn png_info(bytes: &[u8]) -> Option<Info<'static>> {
    let decoder = Decoder::new(Cursor::new(bytes));
    let reader = decoder.read_info().ok()?;
    Some(reader.info().clone())
}

/// Widen one decoded scanline to RGBA8.
fn expand_row(row: &[u8], color: ColorType, out: &mut [u8]) {
    match color {
        ColorType::Rgba => out.copy_from_slice(row),
        ColorType::Rgb => {
            for (src, dst) in row.chunks_exact(3).zip(out.chunks_exact_mut(4)) {
                dst[..3].copy_from_slice(src);
                dst[3] = 255;
            }
        }
        ColorType::Grayscale => {
            for (src, dst) in row.iter().zip(out.chunks_exact_mut(4)) {
                dst[0] = *src;
                dst[1] = *src;
                dst[2] = *src;
                dst[3] = 255;
            }
        }
        ColorType::GrayscaleAlpha => {
            for (src, dst) in row.chunks_exact(2).zip(out.chunks_exact_mut(4)) {
                dst[0] = src[0];
                dst[1] = src[0];
                dst[2] = src[0];
                dst[3] = src[1];
            }
        }
        // The decoder only produces the four 8-bit layouts above once
        // `EXPAND | STRIP_16` is set; anything else is left opaque black.
        _ => out.fill(0),
    }
}

/// Encode `width × height` RGBA rows, produced on demand, into PNG bytes while
/// simultaneously averaging a thumbnail. One row buffer, one thumbnail buffer.
fn assemble<F>(width: u32, height: u32, mut fill: F) -> Option<ClipImage>
where
    F: FnMut(u32, &mut [u8]) -> Option<()>,
{
    check_pixels(width, height)?;
    let (tw, th) = thumb_dims(width, height);
    let mut thumb = ThumbBuilder::new(tw, th, width, height);

    let mut buf: Vec<u8> = Vec::new();
    let mut encoder = Encoder::new(&mut buf, width, height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    // Screenshots are flat runs of colour: `Up` filters well and costs one pass,
    // where `Adaptive` would try five filters per row on the UI thread.
    encoder.set_filter(Filter::Up);
    let mut writer = encoder.write_header().ok()?;

    let mut row = vec![0u8; width as usize * 4];
    {
        let mut stream = writer.stream_writer().ok()?;
        for y in 0..height {
            fill(y, &mut row)?;
            thumb.push_row(y, &row);
            stream.write_all(&row).ok()?;
        }
        stream.finish().ok()?;
    }
    writer.finish().ok()?;
    thumb.flush();

    let mut image = ClipImage {
        encoded: Arc::from(buf),
        origin: ImageOrigin::Dib,
        dims: (width, height),
        thumb: thumb.finish(),
    };
    if image.encoded.len() > MAX_IMAGE_BYTES {
        image.drop_payload();
    }
    Some(image)
}

/// Thumbnail size for a source of `width × height`: longest side
/// [`THUMB_MAX_SIDE`], aspect preserved, never zero.
fn thumb_dims(width: u32, height: u32) -> (u32, u32) {
    if width >= height {
        let tw = width.min(THUMB_MAX_SIDE);
        let th = ((u64::from(height) * u64::from(tw)) / u64::from(width)).max(1) as u32;
        (tw, th)
    } else {
        let th = height.min(THUMB_MAX_SIDE);
        let tw = ((u64::from(width) * u64::from(th)) / u64::from(height)).max(1) as u32;
        (tw, th)
    }
}

/// Box-averages source rows into a small RGBA thumbnail.
struct ThumbBuilder {
    tw: u32,
    th: u32,
    width: u32,
    height: u32,
    acc: Vec<u32>,
    count: u32,
    current: Option<u32>,
    out: Vec<u8>,
}

impl ThumbBuilder {
    fn new(tw: u32, th: u32, width: u32, height: u32) -> Self {
        Self {
            tw,
            th,
            width,
            height,
            acc: vec![0; tw as usize * 4],
            count: 0,
            current: None,
            out: vec![0; tw as usize * th as usize * 4],
        }
    }

    fn push_row(&mut self, y: u32, row: &[u8]) {
        let ty = ((u64::from(y) * u64::from(self.th)) / u64::from(self.height)) as u32;
        if let Some(current) = self.current {
            if current != ty {
                self.flush();
            }
        }
        self.current = Some(ty);
        for tx in 0..self.tw {
            let x0 = (u64::from(tx) * u64::from(self.width) / u64::from(self.tw)) as usize;
            let x1 = ((u64::from(tx + 1) * u64::from(self.width) / u64::from(self.tw)) as usize)
                .max(x0 + 1)
                .min(self.width as usize);
            let mut sums = [0u32; 4];
            for x in x0..x1 {
                let px = &row[x * 4..x * 4 + 4];
                for (sum, channel) in sums.iter_mut().zip(px) {
                    *sum += u32::from(*channel);
                }
            }
            let n = (x1 - x0) as u32;
            let base = tx as usize * 4;
            for (i, sum) in sums.iter().enumerate() {
                self.acc[base + i] += sum / n;
            }
        }
        self.count += 1;
    }

    fn flush(&mut self) {
        let Some(ty) = self.current else {
            return;
        };
        if ty >= self.th || self.count == 0 {
            self.acc.fill(0);
            self.count = 0;
            return;
        }
        let base = ty as usize * self.tw as usize * 4;
        for (i, sum) in self.acc.iter().enumerate() {
            self.out[base + i] = (sum / self.count).min(255) as u8;
        }
        self.acc.fill(0);
        self.count = 0;
    }

    fn finish(mut self) -> Thumbnail {
        self.flush();
        Thumbnail {
            width: self.tw,
            height: self.th,
            rgba: Arc::from(self.out),
        }
    }
}

/// The subset of `BITMAPINFOHEADER` / `BITMAPV4HEADER` / `BITMAPV5HEADER` we
/// need to read pixels out of a clipboard DIB.
struct DibHeader {
    width: u32,
    height: u32,
    top_down: bool,
    bpp: u16,
    stride: usize,
    offset: usize,
    masks: [u32; 4],
}

impl DibHeader {
    fn parse(dib: &[u8]) -> Option<Self> {
        let header_len = u32::from_le_bytes(dib.get(0..4)?.try_into().ok()?) as usize;
        if !(40..=124).contains(&header_len) || dib.len() < header_len {
            return None;
        }
        let width = i32::from_le_bytes(dib.get(4..8)?.try_into().ok()?);
        let height = i32::from_le_bytes(dib.get(8..12)?.try_into().ok()?);
        let bpp = u16::from_le_bytes(dib.get(14..16)?.try_into().ok()?);
        let compression = u32::from_le_bytes(dib.get(16..20)?.try_into().ok()?);
        if width <= 0 || height == 0 || height == i32::MIN {
            return None;
        }
        let (width, height) = (width as u32, height.unsigned_abs());
        if !matches!(bpp, 24 | 32) {
            return None;
        }
        let mut masks = [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0];
        match compression {
            // BI_RGB
            0 => {}
            // BI_BITFIELDS
            3 => {
                let at = if header_len >= 108 {
                    40
                } else {
                    // A 40-byte header is followed by three (or four) DWORDs.
                    header_len
                };
                for (i, mask) in masks.iter_mut().take(3).enumerate() {
                    let start = at + i * 4;
                    *mask = u32::from_le_bytes(dib.get(start..start + 4)?.try_into().ok()?);
                }
                if header_len >= 108 {
                    masks[3] = u32::from_le_bytes(dib.get(52..56)?.try_into().ok()?);
                }
            }
            // RLE / JPEG / PNG in a DIB are not layouts we decode.
            _ => return None,
        }
        let top_down = i32::from_le_bytes(dib.get(8..12)?.try_into().ok()?) < 0;
        let stride = ((u32::from(bpp) * width).div_ceil(32) * 4) as usize;
        let palette = if header_len == 40 && compression == 3 {
            12
        } else {
            0
        };
        let offset = header_len + palette;
        let need = offset + stride.checked_mul(height as usize)?;
        if dib.len() < need {
            return None;
        }
        Some(Self {
            width,
            height,
            top_down,
            bpp,
            stride,
            offset,
            masks,
        })
    }

    /// Source bytes for row `y`, counted from the top of the image.
    fn row<'a>(&self, dib: &'a [u8], y: u32) -> Option<&'a [u8]> {
        let src = if self.top_down {
            y
        } else {
            self.height.checked_sub(1)?.checked_sub(y)?
        };
        let start = self.offset + src as usize * self.stride;
        dib.get(start..start + self.stride)
    }

    fn to_rgba(&self, row: &[u8], out: &mut [u8]) {
        let bytes = usize::from(self.bpp / 8);
        let [rm, gm, bm, am] = self.masks;
        for (src, dst) in row.chunks_exact(bytes).zip(out.chunks_exact_mut(4)) {
            let px = u32::from_le_bytes([src[0], src[1], src[2], *src.get(3).unwrap_or(&0)]);
            dst[0] = channel(px, rm);
            dst[1] = channel(px, gm);
            dst[2] = channel(px, bm);
            // A 32-bit BI_RGB DIB leaves alpha undefined (usually zero); only a
            // real alpha mask may make a pixel translucent.
            dst[3] = if am == 0 { 255 } else { channel(px, am) };
        }
    }
}

/// One masked channel, scaled to 8 bits.
fn channel(px: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 255;
    }
    let shift = mask.trailing_zeros();
    let max = mask >> shift;
    let value = (px & mask) >> shift;
    (value * 255 / max) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests below assert that a constructor returned `Some`; this keeps the
    /// crate-level `clippy::expect_used` ban intact while saying so.
    fn must<T>(value: Option<T>, what: &str) -> T {
        match value {
            Some(value) => value,
            None => panic!("expected {what}"),
        }
    }

    /// A 32-bit bottom-up `BITMAPINFOHEADER` DIB of `w × h`, one colour per row.
    fn dib32(w: u32, h: u32) -> Vec<u8> {
        let stride = w as usize * 4;
        let mut dib = vec![0u8; DIB_HEADER_LEN + stride * h as usize];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        dib[8..12].copy_from_slice(&(h as i32).to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        for y in 0..h {
            for x in 0..w {
                let at = DIB_HEADER_LEN + (h - 1 - y) as usize * stride + x as usize * 4;
                dib[at] = (x % 256) as u8; // B
                dib[at + 1] = (y % 256) as u8; // G
                dib[at + 2] = 0x40; // R
                dib[at + 3] = 0xFF; // A
            }
        }
        dib
    }

    #[test]
    fn a_dib_round_trips_through_png_and_back() {
        let dib = dib32(64, 48);
        let image = must(ClipImage::from_dib(&dib), "a plain 32-bit DIB");
        assert_eq!(image.dims(), (64, 48));
        assert!(image.has_payload());
        // Encoded form is much smaller than the raw DIB for a flat image.
        assert!(image.encoded_len() < dib.len());

        let back = must(image.to_dib(), "paste-back block");
        assert_eq!(back.len(), dib.len());
        // biSize, biWidth, biHeight, biPlanes, biBitCount, biCompression.
        assert_eq!(&back[..20], &dib[..20]);
        // Pixel rows match (ignoring padding, which we always fill).
        assert_eq!(back[DIB_HEADER_LEN..], dib[DIB_HEADER_LEN..]);
    }

    #[test]
    fn the_thumbnail_fits_the_cap_and_keeps_the_aspect() {
        let image = must(ClipImage::from_dib(&dib32(300, 150)), "dib");
        let thumb = image.thumb();
        assert_eq!(thumb.width(), THUMB_MAX_SIDE);
        assert_eq!(thumb.height(), THUMB_MAX_SIDE / 2);
        assert_eq!(thumb.rgba().len(), 96 * 48 * 4);
        assert!(thumb.bytes() < image.encoded_len() + 19_000);
    }

    #[test]
    fn a_tall_image_caps_its_height() {
        let image = must(ClipImage::from_dib(&dib32(40, 400)), "dib");
        let thumb = image.thumb();
        assert_eq!(thumb.height(), THUMB_MAX_SIDE);
        assert!(thumb.width() < THUMB_MAX_SIDE);
    }

    #[test]
    fn an_oversized_image_keeps_its_row_but_loses_the_payload() {
        // 1600 × 1600 of noise cannot compress under 8 MB, so the payload is
        // dropped while the thumbnail and dimensions survive.
        let w = 1600u32;
        let stride = w as usize * 4;
        let mut dib = vec![0u8; DIB_HEADER_LEN + stride * w as usize];
        dib[0..4].copy_from_slice(&40u32.to_le_bytes());
        dib[4..8].copy_from_slice(&(w as i32).to_le_bytes());
        dib[8..12].copy_from_slice(&(w as i32).to_le_bytes());
        dib[12..14].copy_from_slice(&1u16.to_le_bytes());
        dib[14..16].copy_from_slice(&32u16.to_le_bytes());
        let mut state = 0x1234_5678u32;
        for byte in &mut dib[DIB_HEADER_LEN..] {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (state >> 24) as u8;
        }
        let image = must(ClipImage::from_dib(&dib), "dib");
        assert_eq!(image.dims(), (w, w));
        assert!(!image.has_payload(), "a noisy 1600² image exceeds 8 MB");
        assert_eq!(image.encoded_len(), 0);
        assert!(image.thumb().width() > 0);
        assert!(image.to_dib().is_none());
    }

    #[test]
    fn a_png_input_keeps_its_pixels() {
        let source = must(ClipImage::from_dib(&dib32(32, 32)), "dib");
        let png = source.encoded.clone();
        let from_png = must(ClipImage::from_png(&png), "PNG bytes are accepted as-is");
        assert_eq!(from_png.dims(), (32, 32));
        assert_eq!(from_png.origin(), ImageOrigin::Png);
        assert_eq!(from_png.encoded_len(), png.len());
        assert_eq!(from_png.thumb(), source.thumb());

        let (w, h, rgba) = must(source.decode_rgba(), "decode");
        assert_eq!((w, h), (32, 32));
        assert_eq!(rgba.len(), 32 * 32 * 4);
        // Top-left pixel: x=0, y=0 → B=0, G=0, R=0x40.
        assert_eq!(&rgba[..4], &[0x40, 0x00, 0x00, 0xFF]);
    }

    #[test]
    fn a_truncated_dib_is_refused() {
        assert!(ClipImage::from_dib(&[]).is_none());
        assert!(ClipImage::from_dib(&dib32(8, 8)[..50]).is_none());
        assert!(ClipImage::from_png(b"not a png").is_none());
    }

    #[test]
    fn a_placeholder_has_no_payload() {
        let image = ClipImage::placeholder((1440, 900));
        assert!(!image.has_payload());
        assert_eq!(image.dims(), (1440, 900));
        assert_eq!(image.retained_bytes(), 4);
    }
}
