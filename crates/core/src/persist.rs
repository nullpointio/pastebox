//! Pinned-only persistence: a hand-rolled, line-based format, no serde.
//!
//! ```text
//! pastebox-pins<US>1
//! id<US>kind<US>has_text<US>text<US>preview<US>files<US>byte_len<US>created_ms<US>pinned
//! ```
//!
//! `<US>` is `\u{1f}` (unit separator), files inside one field are joined with
//! `\u{1e}` (record separator), and backslash, newline, CR and both separators
//! are escaped inside field bodies. Reading is total: a line that does not parse
//! is skipped, never fatal, and a missing or empty file yields an empty list.
//!
//! Only items with `pinned == true` are written. That is the product's trust
//! promise: unpinned clipboard content never reaches the disk.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::clip::{ClipItem, ClipKind};

const HEADER: &str = "pastebox-pins\u{1f}1";
const HEADER_PREFIX: &str = "pastebox-pins";
const FIELD_SEP: char = '\u{1f}';
const FILE_SEP: char = '\u{1e}';
const FIELD_COUNT: usize = 9;

/// Write the pinned items to `path`, atomically (temp file in the same
/// directory, then rename).
///
/// Unpinned items in `items` are ignored, not an error. Parent directories are
/// created if missing so a first run in a fresh `%APPDATA%\Pastebox` works.
pub fn save_pinned(path: &Path, items: &[ClipItem]) -> io::Result<()> {
    let mut out = String::new();
    out.push_str(HEADER);
    out.push('\n');
    for item in items.iter().filter(|item| item.pinned) {
        out.push_str(&encode_record(item));
        out.push('\n');
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let tmp = temp_path(path);
    fs::write(&tmp, out.as_bytes())?;
    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

/// Read pins back. A missing file, an unreadable file, an empty file or a
/// corrupt line all degrade to "fewer pins", never to a panic.
#[must_use]
pub fn load_pinned(path: &Path) -> Vec<ClipItem> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    parse_pinned(&text)
        .into_iter()
        .filter(|item| item.pinned)
        .collect()
}

/// Parse the whole file body. Valid records come back in file order; anything
/// unparseable is skipped. Exposed so tests can exercise the format without
/// touching the filesystem.
#[must_use]
pub fn parse_pinned(text: &str) -> Vec<ClipItem> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(HEADER_PREFIX) {
            continue;
        }
        if let Some(item) = decode_record(line) {
            out.push(item);
        }
    }
    out
}

fn encode_record(item: &ClipItem) -> String {
    let text = item.text.as_deref().unwrap_or("");
    let files = item
        .files
        .iter()
        .map(|file| escape(file))
        .collect::<Vec<_>>()
        .join("\u{1e}");
    [
        item.id.to_string(),
        item.kind.label().to_string(),
        if item.text.is_some() { "1" } else { "0" }.to_string(),
        escape(text),
        escape(&item.preview),
        files,
        item.byte_len.to_string(),
        item.created_ms.to_string(),
        if item.pinned { "1" } else { "0" }.to_string(),
    ]
    .join(&FIELD_SEP.to_string())
}

fn decode_record(line: &str) -> Option<ClipItem> {
    let fields: Vec<&str> = line.split(FIELD_SEP).collect();
    if fields.len() != FIELD_COUNT {
        return None;
    }
    let id = fields[0].parse::<u64>().ok()?;
    let kind = ClipKind::parse(fields[1])?;
    let has_text = match fields[2] {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    let text = if has_text {
        Some(unescape(fields[3]))
    } else {
        None
    };
    let preview = unescape(fields[4]);
    let files = if fields[5].is_empty() {
        Vec::new()
    } else {
        fields[5].split(FILE_SEP).map(unescape).collect()
    };
    let byte_len = fields[6].parse::<usize>().ok()?;
    let created_ms = fields[7].parse::<u64>().ok()?;
    let pinned = match fields[8] {
        "1" => true,
        "0" => false,
        _ => return None,
    };
    Some(ClipItem {
        id,
        kind,
        text,
        preview,
        files,
        byte_len,
        created_ms,
        pinned,
    })
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            FIELD_SEP => out.push_str("\\x1f"),
            FILE_SEP => out.push_str("\\x1e"),
            other => out.push(other),
        }
    }
    out
}

/// Total, left-to-right unescape. Unknown escapes keep their backslash rather
/// than dropping bytes, so a hand-edited file degrades predictably.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('x') => match (chars.next(), chars.next()) {
                (Some('1'), Some('f')) => out.push(FIELD_SEP),
                (Some('1'), Some('e')) => out.push(FILE_SEP),
                // Truncated or unknown hex escape: drop it, keep going.
                _ => {}
            },
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "pins.json".to_string());
    let tmp = format!(".{name}.{}.tmp", std::process::id());
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(tmp),
        _ => PathBuf::from(tmp),
    }
}
