mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{files_item, image_item, link_item, text_item};
use pastebox_core::{load_pinned, parse_pinned, save_pinned, ClipItem, ClipKind};

/// Unique scratch directory per test; removed at the end of the test.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("pastebox-core-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn pinned_text(body: &str, id: u64, created_ms: u64) -> ClipItem {
    let mut item = text_item(body, created_ms);
    item.id = id;
    item.pinned = true;
    item
}

#[test]
fn roundtrip_preserves_every_field() {
    let dir = TempDir::new("roundtrip");
    let path = dir.file("pins.json");

    let mut item = pinned_text("line one\nline two\ttabbed", 7, 1_700_000_000_123);
    item.files = vec![
        "C:\\Users\\me\\a b.txt".to_string(),
        "C:\\x\\y.bin".to_string(),
    ];
    item.byte_len = 4096;
    item.preview = "line one line two tabbed".to_string();
    item.kind = ClipKind::Text;

    save_pinned(&path, std::slice::from_ref(&item)).expect("save");
    let loaded = load_pinned(&path);
    assert_eq!(loaded, vec![item]);
}

#[test]
fn roundtrip_keeps_an_absent_text_body_absent() {
    let dir = TempDir::new("none-text");
    let path = dir.file("pins.json");

    let mut item = image_item("Screenshot 2026-11-08 141233.png", 319_488, 99);
    item.id = 3;
    item.pinned = true;

    save_pinned(&path, std::slice::from_ref(&item)).expect("save");
    let loaded = load_pinned(&path);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text, None);
    assert_eq!(loaded[0].kind, ClipKind::Image);
    assert_eq!(loaded[0].preview, "Screenshot 2026-11-08 141233.png");
    assert_eq!(loaded[0].byte_len, 319_488);
}

#[test]
fn roundtrip_survives_separator_and_backslash_characters() {
    let dir = TempDir::new("specials");
    let path = dir.file("pins.json");

    let body = "unit\u{1f}sep and record\u{1e}sep and back\\slash\r\nnext";
    let mut item = pinned_text(body, 1, 10);
    item.files = vec!["C:\\odd\u{1f}name\u{1e}.txt".to_string()];
    item.preview = "pre\u{1f}view".to_string();

    save_pinned(&path, std::slice::from_ref(&item)).expect("save");
    let loaded = load_pinned(&path);
    assert_eq!(loaded.len(), 1, "one record per clip, newlines escaped");
    assert_eq!(loaded[0], item);
}

#[test]
fn roundtrip_handles_unicode_and_emoji() {
    let dir = TempDir::new("unicode");
    let path = dir.file("pins.json");

    let body = "Café 🚀 日本語 — “quoted”";
    let item = pinned_text(body, 42, 5);

    save_pinned(&path, std::slice::from_ref(&item)).expect("save");
    assert_eq!(load_pinned(&path), vec![item]);
}

#[test]
fn roundtrip_handles_a_large_body() {
    let dir = TempDir::new("large");
    let path = dir.file("pins.json");

    let body = "αβγ🚀".repeat(20_000);
    let mut item = pinned_text(&body, 1, 1);
    item.byte_len = body.len();
    item.preview = "αβγ🚀".repeat(30);

    save_pinned(&path, std::slice::from_ref(&item)).expect("save");
    let loaded = load_pinned(&path);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].text.as_deref(), Some(body.as_str()));
}

#[test]
fn save_writes_only_pinned_items() {
    let dir = TempDir::new("only-pins");
    let path = dir.file("pins.json");

    let secret = text_item("UNPINNED-SECRET-1234", 1000);
    let pin = pinned_text("keep me", 5, 2000);
    let other = text_item("ANOTHER-UNPINNED", 3000);

    save_pinned(&path, &[secret, pin.clone(), other]).expect("save");

    let raw = fs::read_to_string(&path).expect("read back");
    assert!(!raw.contains("UNPINNED-SECRET-1234"));
    assert!(!raw.contains("ANOTHER-UNPINNED"));
    assert!(raw.contains("keep me"));
    assert_eq!(raw.lines().count(), 2, "header + one pin");
    assert_eq!(load_pinned(&path), vec![pin]);
}

#[test]
fn save_with_no_pins_writes_only_the_header() {
    let dir = TempDir::new("no-pins");
    let path = dir.file("pins.json");

    save_pinned(&path, &[text_item("a", 1), text_item("b", 2)]).expect("save");
    let raw = fs::read_to_string(&path).expect("read back");
    assert_eq!(raw.lines().count(), 1);
    assert!(raw.starts_with("pastebox-pins"));
    assert!(load_pinned(&path).is_empty());
}

#[test]
fn save_creates_missing_parent_directories() {
    let dir = TempDir::new("mkdir");
    let path = dir.file("nested/deeper/pins.json");
    let pin = pinned_text("x", 1, 1);

    save_pinned(&path, std::slice::from_ref(&pin)).expect("save");
    assert!(path.exists());
    assert_eq!(load_pinned(&path), vec![pin]);
}

#[test]
fn save_overwrites_previous_content() {
    let dir = TempDir::new("overwrite");
    let path = dir.file("pins.json");

    let first = pinned_text("first", 1, 1);
    let second = pinned_text("second", 2, 2);
    save_pinned(&path, &[first.clone(), second]).expect("save");
    save_pinned(&path, std::slice::from_ref(&first)).expect("save again");

    let raw = fs::read_to_string(&path).expect("read back");
    assert_eq!(raw.lines().count(), 2);
    assert!(!raw.contains("second"));
    assert_eq!(load_pinned(&path), vec![first]);
}

#[test]
fn load_missing_file_is_empty() {
    let dir = TempDir::new("missing");
    assert!(load_pinned(&dir.file("nope.json")).is_empty());
}

#[test]
fn load_empty_file_is_empty() {
    let dir = TempDir::new("empty");
    let path = dir.file("pins.json");
    fs::write(&path, b"").expect("write");
    assert!(load_pinned(&path).is_empty());
}

#[test]
fn load_file_with_only_a_header_is_empty() {
    let dir = TempDir::new("header-only");
    let path = dir.file("pins.json");
    fs::write(&path, "pastebox-pins\u{1f}1\n").expect("write");
    assert!(load_pinned(&path).is_empty());
}

#[test]
fn corrupt_lines_are_skipped_not_fatal() {
    let dir = TempDir::new("corrupt");
    let path = dir.file("pins.json");

    let good_a = pinned_text("alpha", 1, 10);
    let good_b = pinned_text("beta", 2, 20);
    let mut body = String::new();
    body.push_str("pastebox-pins\u{1f}1\n");
    body.push_str(&encode_for_test(&good_a));
    body.push('\n');
    body.push_str("this is not a record at all\n");
    body.push_str("1\u{1f}text\u{1f}1\n"); // truncated field list
    body.push_str("3\u{1f}text\u{1f}1\u{1f}x\u{1f}y\u{1f}\u{1f}not-a-number\u{1f}0\u{1f}1\n");
    body.push_str("4\u{1f}unknownkind\u{1f}1\u{1f}x\u{1f}y\u{1f}\u{1f}1\u{1f}0\u{1f}1\n");
    body.push('\n');
    body.push_str(&encode_for_test(&good_b));
    body.push('\n');
    fs::write(&path, body).expect("write");

    let loaded = load_pinned(&path);
    assert_eq!(loaded, vec![good_a, good_b]);
}

#[test]
fn header_is_optional() {
    let dir = TempDir::new("no-header");
    let path = dir.file("pins.json");
    let pin = pinned_text("alpha", 1, 10);
    fs::write(&path, format!("{}\n", encode_for_test(&pin))).expect("write");
    assert_eq!(load_pinned(&path), vec![pin]);
}

#[test]
fn crlf_line_endings_are_tolerated() {
    let dir = TempDir::new("crlf");
    let path = dir.file("pins.json");
    let pin = pinned_text("alpha", 1, 10);
    let body = format!("pastebox-pins\u{1f}1\r\n{}\r\n", encode_for_test(&pin));
    fs::write(&path, body).expect("write");
    assert_eq!(load_pinned(&path), vec![pin]);
}

#[test]
fn load_drops_records_marked_unpinned() {
    let dir = TempDir::new("unpinned-record");
    let path = dir.file("pins.json");

    let mut unpinned = text_item("should not load", 1);
    unpinned.id = 1;
    unpinned.pinned = false;
    let body = format!("pastebox-pins\u{1f}1\n{}\n", encode_for_test(&unpinned));
    fs::write(&path, &body).expect("write");

    assert!(load_pinned(&path).is_empty());
    // The record itself is well formed, so the parser still returns it.
    assert_eq!(parse_pinned(&body).len(), 1);
}

#[test]
fn parse_tolerates_garbage_and_never_panics() {
    for body in [
        "",
        "\n\n\n",
        "pastebox-pins",
        "\u{1f}\u{1f}\u{1f}",
        "1\u{1f}text\u{1f}1\u{1f}\u{1f}\u{1f}\u{1f}0\u{1f}0\u{1f}1",
        "99999999999999999999999999\u{1f}text\u{1f}1\u{1f}x\u{1f}y\u{1f}\u{1f}1\u{1f}0\u{1f}1",
        "1\u{1f}text\u{1f}2\u{1f}x\u{1f}y\u{1f}\u{1f}1\u{1f}0\u{1f}1",
        "1\u{1f}text\u{1f}1\u{1f}unterminated\\",
    ] {
        let _ = parse_pinned(body);
    }
}

#[test]
fn save_reports_io_errors_instead_of_panicking() {
    let dir = TempDir::new("io-error");
    // A directory where the file should be: rename onto it must fail cleanly.
    let path = dir.file("pins.json");
    fs::create_dir_all(&path).expect("mkdir");
    let pin = pinned_text("x", 1, 1);
    assert!(save_pinned(&path, &[pin]).is_err());
}

#[test]
fn pins_file_is_utf8_text() {
    let dir = TempDir::new("utf8");
    let path = dir.file("pins.json");
    let pin = pinned_text("Café 🚀", 1, 1);
    save_pinned(&path, &[pin]).expect("save");

    let bytes = fs::read(&path).expect("read");
    let text = String::from_utf8(bytes).expect("valid utf8");
    assert!(text.starts_with("pastebox-pins\u{1f}1\n"));
    assert!(text.ends_with('\n'));
}

#[test]
fn load_keeps_file_order() {
    let dir = TempDir::new("order");
    let path = dir.file("pins.json");

    let a = pinned_text("a", 1, 10);
    let b = pinned_text("b", 2, 20);
    let c = pinned_text("c", 3, 30);
    save_pinned(&path, &[a.clone(), b.clone(), c.clone()]).expect("save");

    let loaded = load_pinned(&path);
    assert_eq!(
        loaded.iter().map(|i| i.id).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(loaded, vec![a, b, c]);
}

#[test]
fn links_and_files_roundtrip() {
    let dir = TempDir::new("kinds");
    let path = dir.file("pins.json");

    let mut link = link_item("https://example.com/invoices/2026", 1);
    link.id = 1;
    link.pinned = true;

    let mut files = files_item(&["C:\\a\\one.pdf", "C:\\b\\two.pdf"], 2048, 2);
    files.id = 2;
    files.pinned = true;

    let expected = vec![link, files];
    save_pinned(&path, &expected).expect("save");
    assert_eq!(load_pinned(&path), expected);
}

#[test]
fn save_is_atomic_leaving_no_temp_file() {
    let dir = TempDir::new("atomic");
    let path = dir.file("pins.json");
    save_pinned(&path, &[pinned_text("x", 1, 1)]).expect("save");

    let leftovers: Vec<_> = fs::read_dir(&dir.0)
        .expect("read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "pins.json")
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
    assert!(Path::new(&path).is_file());
}

// Mirrors the on-disk encoder so the tests can hand-build fixture files.
fn encode_for_test(item: &ClipItem) -> String {
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\u{1f}', "\\x1f")
            .replace('\u{1e}', "\\x1e")
    };
    [
        item.id.to_string(),
        item.kind.label().to_string(),
        if item.text.is_some() { "1" } else { "0" }.to_string(),
        esc(item.text.as_deref().unwrap_or("")),
        esc(&item.preview),
        item.files
            .iter()
            .map(|f| esc(f))
            .collect::<Vec<_>>()
            .join("\u{1e}"),
        item.byte_len.to_string(),
        item.created_ms.to_string(),
        if item.pinned { "1" } else { "0" }.to_string(),
    ]
    .join("\u{1f}")
}
