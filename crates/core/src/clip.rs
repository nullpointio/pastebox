//! The clip model: kind, identity, and the two display strings the UI needs.

/// Longest preview we keep for a row title, in characters (not bytes).
pub const PREVIEW_MAX_CHARS: usize = 160;

/// What was on the clipboard. `TooLarge` stubs from the PRD are represented by
/// the platform layer as a `Text` clip with an empty body, so core stays at four
/// kinds: the four the product can actually paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ClipKind {
    Text,
    Image,
    Files,
    Link,
}

impl ClipKind {
    /// Every kind, in the order the filter chips use.
    pub const ALL: [ClipKind; 4] = [
        ClipKind::Text,
        ClipKind::Image,
        ClipKind::Files,
        ClipKind::Link,
    ];

    /// Lowercase, stable, and also the on-disk token in the pins file.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Image => "image",
            ClipKind::Files => "files",
            ClipKind::Link => "link",
        }
    }

    /// Inverse of [`ClipKind::label`]; `None` for anything unrecognised so a
    /// corrupt pins line can be skipped instead of panicking.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "text" => Some(ClipKind::Text),
            "image" => Some(ClipKind::Image),
            "files" => Some(ClipKind::Files),
            "link" => Some(ClipKind::Link),
            _ => None,
        }
    }
}

/// One clipboard row.
///
/// `id` is assigned by [`crate::Store::insert`]; the value passed in is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipItem {
    pub id: u64,
    pub kind: ClipKind,
    /// Plain text body. `None` for images and for file lists.
    pub text: Option<String>,
    /// Single-line row title, already collapsed and truncated.
    pub preview: String,
    /// Absolute paths, in clipboard order, duplicates removed.
    pub files: Vec<String>,
    /// Accounted size in bytes (decoded for images, UTF-8 length for text).
    pub byte_len: usize,
    /// Unix epoch milliseconds, supplied by the caller so core has no clock.
    pub created_ms: u64,
    pub pinned: bool,
}

impl ClipItem {
    /// Build a clip and derive its preview. `id` is zero; the store assigns the
    /// real one.
    #[must_use]
    pub fn new(
        kind: ClipKind,
        text: Option<String>,
        files: Vec<String>,
        byte_len: usize,
        created_ms: u64,
    ) -> Self {
        let preview = match kind {
            ClipKind::Text | ClipKind::Link => {
                text.as_deref().map(preview_from_text).unwrap_or_default()
            }
            ClipKind::Files => files
                .first()
                .map(|p| file_name(p).to_string())
                .unwrap_or_default(),
            // Images carry no text; the caller sets the row title with
            // `with_preview` (e.g. "Screenshot 2026-11-08 141233.png").
            ClipKind::Image => String::new(),
        };
        Self {
            id: 0,
            kind,
            text,
            preview,
            files,
            byte_len,
            created_ms,
            pinned: false,
        }
    }

    /// Override the derived preview (images, and file clips with a nicer title).
    #[must_use]
    pub fn with_preview(mut self, preview: impl Into<String>) -> Self {
        self.preview = preview.into();
        self
    }

    #[must_use]
    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// `"text"`, `"image"`, `"files"`, `"link"`.
    #[must_use]
    pub fn kind_label(&self) -> &'static str {
        self.kind.label()
    }

    /// The meta line under the title, e.g. `text · 412 chars`,
    /// `3 files · 2.1 MB`, `image · 620×480`.
    ///
    /// Images have no dimension field in the model, so the dimensions are read
    /// back out of `preview` when the platform layer put a `WxH` / `W×H` token
    /// there; otherwise the decoded byte size is shown.
    #[must_use]
    pub fn subtitle(&self) -> String {
        match self.kind {
            ClipKind::Text | ClipKind::Link => {
                let body = self.text.as_deref().unwrap_or(self.preview.as_str());
                let n = body.chars().count();
                let unit = if n == 1 { "char" } else { "chars" };
                format!("{} · {} {}", self.kind_label(), n, unit)
            }
            ClipKind::Files => {
                let n = self.files.len();
                let unit = if n == 1 { "file" } else { "files" };
                format!("{} {} · {}", n, unit, format_bytes(self.byte_len))
            }
            ClipKind::Image => match dims_from_preview(&self.preview) {
                Some((w, h)) => format!("image · {w}×{h}"),
                None => format!("image · {}", format_bytes(self.byte_len)),
            },
        }
    }

    /// Identity used for dedupe: same kind + same normalised content.
    ///
    /// * text and links: CRLF collapsed, trailing whitespace trimmed per line
    /// * files: order preserved, duplicates removed (an ordered set)
    /// * images: the model has no pixel data, so title + size stand in
    pub(crate) fn dedupe_key(&self) -> (ClipKind, String) {
        match self.kind {
            ClipKind::Text | ClipKind::Link => {
                let body = self.text.as_deref().unwrap_or(self.preview.as_str());
                (self.kind, normalize_text(body))
            }
            ClipKind::Files => {
                let mut seen: Vec<&str> = Vec::with_capacity(self.files.len());
                for f in &self.files {
                    let f = f.trim();
                    if !f.is_empty() && !seen.contains(&f) {
                        seen.push(f);
                    }
                }
                (self.kind, seen.join("\u{1f}"))
            }
            ClipKind::Image => (
                self.kind,
                format!("{}\u{1f}{}", self.preview.trim(), self.byte_len),
            ),
        }
    }
}

/// `1.2 KB`, `312 KB`, `2.1 MB`, `0 B`. Binary units, one decimal below ten.
#[must_use]
pub fn format_bytes(n: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let f = n as f64;
    if f < KB {
        format!("{n} B")
    } else if f < MB {
        scaled(f / KB, "KB")
    } else {
        scaled(f / MB, "MB")
    }
}

fn scaled(value: f64, unit: &str) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded < 10.0 {
        format!("{rounded:.1} {unit}")
    } else {
        format!("{:.0} {unit}", value.round())
    }
}

/// PRD R-07 normalisation: `\r\n` → `\n`, trailing whitespace trimmed.
fn normalize_text(s: &str) -> String {
    let unified = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(unified.len());
    for (i, line) in unified.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end());
    }
    let end = out.trim_end().len();
    out.truncate(end);
    out
}

/// Collapse every run of whitespace to one space and cut to
/// [`PREVIEW_MAX_CHARS`] with a single ellipsis.
fn preview_from_text(text: &str) -> String {
    let mut collapsed = String::with_capacity(text.len().min(PREVIEW_MAX_CHARS * 4));
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = !collapsed.is_empty();
            continue;
        }
        if pending_space {
            collapsed.push(' ');
            pending_space = false;
        }
        collapsed.push(c);
    }
    truncate_chars(&collapsed, PREVIEW_MAX_CHARS)
}

pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn file_name(path: &str) -> &str {
    // Both separators are ASCII, so the byte index after them is `i + 1`.
    path.rfind(['\\', '/']).map_or(path, |i| &path[i + 1..])
}

/// Pull `1440×900` / `1440x900` out of a title, if it is a standalone token.
fn dims_from_preview(preview: &str) -> Option<(u32, u32)> {
    let chars: Vec<char> = preview.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i < chars.len() && matches!(chars[i], '×' | 'x' | 'X') {
            let sep = i;
            let mut j = sep + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > sep + 1 {
                let before_ok = start == 0 || !chars[start - 1].is_alphanumeric();
                let after_ok = j == chars.len() || !chars[j].is_alphanumeric();
                if before_ok && after_ok {
                    let w: String = chars[start..sep].iter().collect();
                    let h: String = chars[sep + 1..j].iter().collect();
                    if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                        if w > 0 && h > 0 {
                            return Some((w, h));
                        }
                    }
                }
            }
        }
        if i == start {
            i += 1;
        }
    }
    None
}
