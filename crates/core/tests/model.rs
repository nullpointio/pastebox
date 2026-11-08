mod common;

use common::{files_item, image_item, link_item, text_item};
use pastebox_core::{format_bytes, ClipItem, ClipKind, PREVIEW_MAX_CHARS};

#[test]
fn kind_labels_and_parse_roundtrip() {
    assert_eq!(ClipKind::Text.label(), "text");
    assert_eq!(ClipKind::Image.label(), "image");
    assert_eq!(ClipKind::Files.label(), "files");
    assert_eq!(ClipKind::Link.label(), "link");
    for kind in ClipKind::ALL {
        assert_eq!(ClipKind::parse(kind.label()), Some(kind));
    }
    assert_eq!(ClipKind::parse("TooLarge"), None);
    assert_eq!(ClipKind::parse(""), None);
}

#[test]
fn kind_label_method_matches_enum() {
    let item = text_item("hello", 1);
    assert_eq!(item.kind_label(), "text");
    assert_eq!(image_item("shot.png", 10, 1).kind_label(), "image");
    assert_eq!(files_item(&["a.txt"], 10, 1).kind_label(), "files");
    assert_eq!(link_item("https://a.b", 1).kind_label(), "link");
}

#[test]
fn subtitle_text_counts_characters() {
    let item = text_item(&"a".repeat(412), 1);
    assert_eq!(item.subtitle(), "text · 412 chars");
}

#[test]
fn subtitle_text_singular_char() {
    assert_eq!(text_item("x", 1).subtitle(), "text · 1 char");
    assert_eq!(text_item("", 1).subtitle(), "text · 0 chars");
}

#[test]
fn subtitle_text_counts_chars_not_bytes() {
    // "é" is two UTF-8 bytes, one char.
    let item = text_item("ééé", 1);
    assert_eq!(item.byte_len, 6);
    assert_eq!(item.subtitle(), "text · 3 chars");
}

#[test]
fn subtitle_link_reports_length() {
    let url = "https://example.com/invoices/2026";
    assert_eq!(
        link_item(url, 1).subtitle(),
        format!("link · {} chars", url.chars().count())
    );
}

#[test]
fn subtitle_files_plural_and_megabytes() {
    let item = files_item(&["a.pdf", "b.pdf", "c.pdf"], 2_202_009, 1);
    assert_eq!(item.subtitle(), "3 files · 2.1 MB");
}

#[test]
fn subtitle_files_singular_and_kilobytes() {
    let item = files_item(&["a.pdf"], 1024, 1);
    assert_eq!(item.subtitle(), "1 file · 1.0 KB");
}

#[test]
fn subtitle_files_empty_list() {
    let item = files_item(&[], 0, 1);
    assert_eq!(item.subtitle(), "0 files · 0 B");
}

#[test]
fn subtitle_image_uses_dimensions_from_title() {
    let item = image_item("Screenshot 2026-11-08 141233 1440×900.png", 319_488, 1);
    assert_eq!(item.subtitle(), "image · 1440×900");
}

#[test]
fn subtitle_image_accepts_ascii_x_dimensions() {
    let item = image_item("clip 620x480 final", 10, 1);
    assert_eq!(item.subtitle(), "image · 620×480");
}

#[test]
fn subtitle_image_without_dimensions_falls_back_to_size() {
    let item = image_item("clipboard.png", 319_488, 1);
    assert_eq!(item.subtitle(), "image · 312 KB");
}

#[test]
fn subtitle_image_ignores_dates_that_look_like_dimensions() {
    let item = image_item("Screenshot 2026-11-08 141233.png", 2048, 1);
    assert_eq!(item.subtitle(), "image · 2.0 KB");
}

#[test]
fn format_bytes_boundaries() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(1), "1 B");
    assert_eq!(format_bytes(1023), "1023 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(10 * 1024), "10 KB");
    assert_eq!(format_bytes(319_488), "312 KB");
    assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    assert_eq!(format_bytes(2_202_009), "2.1 MB");
    assert_eq!(format_bytes(10 * 1024 * 1024), "10 MB");
    assert_eq!(format_bytes(64 * 1024 * 1024), "64 MB");
}

#[test]
fn preview_truncates_very_long_text_with_one_ellipsis() {
    let item = text_item(&"a".repeat(300), 1);
    assert_eq!(item.preview.chars().count(), PREVIEW_MAX_CHARS + 1);
    assert!(item.preview.ends_with('…'));
    assert!(!item.preview.contains("……"));
    assert_eq!(item.preview.matches('…').count(), 1);
}

#[test]
fn preview_collapses_whitespace_and_newlines() {
    let item = text_item("  hello \n\n\t world  ", 1);
    assert_eq!(item.preview, "hello world");
}

#[test]
fn preview_keeps_short_text_verbatim() {
    let item = text_item("Hi Sarah — attaching the signed lease", 1);
    assert_eq!(item.preview, "Hi Sarah — attaching the signed lease");
}

#[test]
fn preview_truncation_is_char_safe_for_unicode() {
    let item = text_item(&"🚀".repeat(200), 1);
    assert_eq!(item.preview.chars().count(), PREVIEW_MAX_CHARS + 1);
    assert!(item.preview.ends_with('…'));
    assert_eq!(
        item.preview.chars().filter(|c| *c == '🚀').count(),
        PREVIEW_MAX_CHARS
    );
}

#[test]
fn preview_from_files_uses_the_file_name() {
    let item = files_item(&["C:\\Users\\me\\Desktop\\invoice-march.pdf"], 100, 1);
    assert_eq!(item.preview, "invoice-march.pdf");
}

#[test]
fn preview_from_files_handles_forward_slashes_and_bare_names() {
    assert_eq!(files_item(&["/tmp/a/b.txt"], 1, 1).preview, "b.txt");
    assert_eq!(files_item(&["notes.txt"], 1, 1).preview, "notes.txt");
}

#[test]
fn image_preview_is_empty_until_set() {
    let bare = ClipItem::new(ClipKind::Image, None, Vec::new(), 10, 1);
    assert_eq!(bare.preview, "");
    assert_eq!(bare.with_preview("shot.png").preview, "shot.png");
}

#[test]
fn with_pinned_sets_the_flag() {
    let item = text_item("x", 1).with_pinned(true);
    assert!(item.pinned);
}
