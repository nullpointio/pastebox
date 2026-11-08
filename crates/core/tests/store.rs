mod common;

use common::{files_item, link_item, text_item};
use pastebox_core::{ClipImage, ClipItem, ClipKind, Store, DEFAULT_CAPACITY, MAX_TEXT_BYTES};

#[test]
fn insert_assigns_increasing_ids() {
    let mut store = Store::new(10);
    let a = store.insert(text_item("a", 1000));
    let b = store.insert(text_item("b", 2000));
    let c = store.insert(text_item("c", 3000));
    assert!(a < b && b < c);
    assert_eq!(store.items()[0].id, c);
}

#[test]
fn insert_ignores_a_caller_supplied_id() {
    let mut store = Store::new(10);
    let mut item = text_item("a", 1000);
    item.id = 999;
    let id = store.insert(item);
    assert_ne!(id, 999);
    assert_eq!(store.items()[0].id, id);
}

#[test]
fn items_are_newest_first() {
    let mut store = Store::new(10);
    store.insert(text_item("old", 1000));
    store.insert(text_item("mid", 2000));
    store.insert(text_item("new", 3000));
    let previews: Vec<&str> = store.items().iter().map(|i| i.preview.as_str()).collect();
    assert_eq!(previews, vec!["new", "mid", "old"]);
}

#[test]
fn dedupe_keeps_id_and_moves_to_top() {
    let mut store = Store::new(10);
    let a = store.insert(text_item("alpha", 1000));
    store.insert(text_item("beta", 2000));
    let again = store.insert(text_item("alpha", 3000));
    assert_eq!(again, a);
    assert_eq!(store.len(), 2);
    assert_eq!(store.items()[0].id, a);
    assert_eq!(store.items()[0].preview, "alpha");
}

#[test]
fn dedupe_keeps_pin_state() {
    let mut store = Store::new(10);
    let id = store.insert(text_item("alpha", 1000).with_pinned(true));
    let again = store.insert(text_item("alpha", 2000));
    assert_eq!(again, id);
    assert_eq!(store.len(), 1);
    assert!(store.items()[0].pinned);
}

#[test]
fn dedupe_refreshes_the_timestamp() {
    let mut store = Store::new(10);
    let id = store.insert(text_item("alpha", 1000));
    store.insert(text_item("alpha", 5000));
    assert_eq!(store.get(id).map(|i| i.created_ms), Some(5000));
}

#[test]
fn dedupe_never_moves_time_backwards() {
    let mut store = Store::new(10);
    let id = store.insert(text_item("alpha", 5000));
    store.insert(text_item("alpha", 1000));
    assert_eq!(store.get(id).map(|i| i.created_ms), Some(5000));
}

#[test]
fn dedupe_ignores_trailing_whitespace_and_crlf() {
    let mut store = Store::new(10);
    store.insert(text_item("line one  \r\nline two\r\n", 1000));
    store.insert(text_item("line one\nline two", 2000));
    assert_eq!(store.len(), 1);
    assert_eq!(store.items()[0].created_ms, 2000);
}

#[test]
fn dedupe_keeps_leading_whitespace_significant() {
    let mut store = Store::new(10);
    store.insert(text_item("    indented", 1000));
    store.insert(text_item("indented", 2000));
    assert_eq!(store.len(), 2);
}

#[test]
fn dedupe_does_not_merge_different_kinds() {
    let mut store = Store::new(10);
    store.insert(text_item("https://example.com", 1000));
    store.insert(link_item("https://example.com", 2000));
    assert_eq!(store.len(), 2);
    assert_eq!(store.items()[0].kind, ClipKind::Link);
    assert_eq!(store.items()[1].kind, ClipKind::Text);
}

#[test]
fn dedupe_treats_files_as_an_ordered_set() {
    let mut store = Store::new(10);
    store.insert(files_item(&["a.txt", "b.txt"], 10, 1000));
    store.insert(files_item(&["a.txt", "a.txt", "b.txt"], 20, 2000));
    assert_eq!(store.len(), 1);

    store.insert(files_item(&["b.txt", "a.txt"], 20, 3000));
    assert_eq!(store.len(), 2);
}

#[test]
fn dedupe_refreshes_files_and_preview() {
    let mut store = Store::new(10);
    store.insert(files_item(&["C:\\a\\old.txt"], 10, 1000));
    store.insert(files_item(&["C:\\a\\new.txt"], 10, 1000));
    assert_eq!(store.len(), 2);

    store.insert(files_item(&["C:\\a\\old.txt", "C:\\a\\new.txt"], 20, 2000));
    let row = &store.items()[0];
    assert_eq!(row.files.len(), 2);
    assert_eq!(row.preview, "old.txt");
    assert_eq!(row.byte_len, 20);
}

#[test]
fn capacity_evicts_the_oldest_unpinned_item() {
    let mut store = Store::new(3);
    let a = store.insert(text_item("a", 1000));
    let b = store.insert(text_item("b", 2000));
    let c = store.insert(text_item("c", 3000));
    let d = store.insert(text_item("d", 4000));
    assert_eq!(store.len(), 3);
    assert!(store.get(a).is_none());
    for id in [b, c, d] {
        assert!(store.get(id).is_some());
    }
}

#[test]
fn capacity_never_evicts_a_pin_even_when_it_is_oldest() {
    let mut store = Store::new(3);
    let pinned = store.insert(text_item("pin", 1000).with_pinned(true));
    let u1 = store.insert(text_item("u1", 2000));
    let u2 = store.insert(text_item("u2", 3000));
    let u3 = store.insert(text_item("u3", 4000));

    assert_eq!(store.len(), 3);
    assert!(store.get(pinned).is_some());
    assert!(store.get(u1).is_none());
    assert!(store.get(u2).is_some() && store.get(u3).is_some());
}

#[test]
fn capacity_all_pinned_grows_instead_of_dropping_pins() {
    let mut store = Store::new(2);
    for i in 0..5 {
        store.insert(text_item(&format!("pin{i}"), i * 1000).with_pinned(true));
    }
    assert_eq!(store.len(), 5);
}

#[test]
fn capacity_zero_is_clamped_to_one() {
    let mut store = Store::new(0);
    assert_eq!(store.capacity(), 1);
    let a = store.insert(text_item("a", 1000));
    let b = store.insert(text_item("b", 2000));
    assert_eq!(store.len(), 1);
    assert!(store.get(a).is_none());
    assert!(store.get(b).is_some());
}

#[test]
fn capacity_counts_only_unpinned_items_for_eviction() {
    let mut store = Store::new(4);
    let p1 = store.insert(text_item("p1", 1000).with_pinned(true));
    let p2 = store.insert(text_item("p2", 2000).with_pinned(true));
    let u1 = store.insert(text_item("u1", 3000));
    let u2 = store.insert(text_item("u2", 4000));
    let u3 = store.insert(text_item("u3", 5000));

    assert_eq!(store.len(), 4);
    assert!(store.get(p1).is_some() && store.get(p2).is_some());
    assert!(store.get(u1).is_none());
    assert!(store.get(u2).is_some() && store.get(u3).is_some());
}

#[test]
fn clear_unpinned_keeps_pins_only() {
    let mut store = Store::new(10);
    store.insert(text_item("a", 1000));
    let pinned = store.insert(text_item("b", 2000).with_pinned(true));
    store.insert(text_item("c", 3000));

    store.clear_unpinned();
    assert_eq!(store.len(), 1);
    assert_eq!(store.items()[0].id, pinned);
}

#[test]
fn remove_deletes_only_that_id() {
    let mut store = Store::new(10);
    let a = store.insert(text_item("a", 1000));
    let b = store.insert(text_item("b", 2000));
    store.remove(a);
    assert_eq!(store.len(), 1);
    assert_eq!(store.items()[0].id, b);
    store.remove(9999);
    assert_eq!(store.len(), 1);
}

#[test]
fn pin_toggles_and_unknown_ids_are_a_noop() {
    let mut store = Store::new(10);
    let id = store.insert(text_item("a", 1000));
    store.pin(id, true);
    assert!(store.items()[0].pinned);
    store.pin(id, false);
    assert!(!store.items()[0].pinned);
    store.pin(424242, true);
    assert_eq!(store.len(), 1);
}

#[test]
fn get_len_and_is_empty() {
    let mut store = Store::new(10);
    assert!(store.is_empty());
    assert_eq!(store.len(), 0);
    assert!(store.get(1).is_none());
    let id = store.insert(text_item("a", 1000));
    assert!(!store.is_empty());
    assert_eq!(store.len(), 1);
    assert_eq!(store.get(id).map(|i| i.preview.as_str()), Some("a"));
}

#[test]
fn default_capacity_is_five_hundred() {
    assert_eq!(Store::default().capacity(), DEFAULT_CAPACITY);
    assert_eq!(DEFAULT_CAPACITY, 500);
}

#[test]
fn pinned_items_returns_pins_newest_first() {
    let mut store = Store::new(10);
    store.insert(text_item("u1", 1000));
    let p1 = store.insert(text_item("p1", 2000).with_pinned(true));
    store.insert(text_item("u2", 3000));
    let p2 = store.insert(text_item("p2", 4000).with_pinned(true));

    let pins = store.pinned_items();
    let ids: Vec<u64> = pins.iter().map(|i| i.id).collect();
    assert_eq!(ids, vec![p2, p1]);
    assert!(pins.iter().all(|i| i.pinned));
}

#[test]
fn dedupe_still_works_at_capacity() {
    let mut store = Store::new(2);
    let a = store.insert(text_item("alpha", 1000));
    store.insert(text_item("beta", 2000));
    let again = store.insert(text_item("alpha", 3000));
    assert_eq!(again, a);
    assert_eq!(store.len(), 2);
    assert_eq!(store.items()[0].id, a);
}

#[test]
fn inserted_item_keeps_its_fields() {
    let mut store = Store::new(10);
    let item = ClipItem::new(
        ClipKind::Files,
        None,
        vec!["C:\\a\\b.txt".to_string()],
        4096,
        1234,
    );
    let id = store.insert(item);
    let row = store.get(id).expect("just inserted");
    assert_eq!(row.byte_len, 4096);
    assert_eq!(row.created_ms, 1234);
    assert_eq!(row.files, vec!["C:\\a\\b.txt".to_string()]);
    assert_eq!(row.preview, "b.txt");
}

#[test]
fn text_over_one_megabyte_is_truncated_on_the_way_in() {
    let mut store = Store::new(10);
    let body = "a".repeat(MAX_TEXT_BYTES + 5000);
    let id = store.insert(text_item(&body, 1000));
    let row = store.get(id).expect("stored");
    let kept = row.text.as_deref().expect("text body");
    assert_eq!(kept.len(), MAX_TEXT_BYTES);
    assert!(row.payload_bytes() <= MAX_TEXT_BYTES);
}

#[test]
fn a_multibyte_char_is_never_split_by_the_text_cap() {
    let mut store = Store::new(10);
    // U+00E9 is two bytes, so a cap landing mid-character must step back to it.
    let body = "\u{e9}".repeat(MAX_TEXT_BYTES / 2 + 10);
    let id = store.insert(text_item(&body, 1000));
    let kept = store
        .get(id)
        .and_then(|r| r.text.clone())
        .expect("text body");
    assert!(kept.len() <= MAX_TEXT_BYTES);
    assert!(kept.chars().all(|c| c == '\u{e9}'));
}

/// A budget evicts the oldest unpinned rows, not just the oldest rows.
#[test]
fn the_byte_budget_evicts_the_oldest_unpinned_rows() {
    let mut store = Store::with_budget(100, 2400);
    store.insert(text_item(&"a".repeat(1000), 1000));
    store.insert(text_item(&"b".repeat(1000), 2000));
    store.insert(text_item(&"c".repeat(1000), 3000));
    // 3000 bytes cannot fit in 2400, so the oldest row is already gone.
    assert_eq!(store.len(), 2);
    assert!(store.payload_bytes() <= 2400);
    // The budget keeps holding as new rows arrive: oldest first, pins last.
    store.insert(text_item(&"d".repeat(1000), 4000));
    assert!(store.payload_bytes() <= 2400);
    let previews: Vec<&str> = store.items().iter().map(|i| i.preview.as_str()).collect();
    assert_eq!(previews.len(), 2);
    assert!(previews[0].starts_with('d'));
}

#[test]
fn the_byte_budget_never_evicts_a_pin() {
    let mut store = Store::with_budget(100, 1500);
    let pinned = store.insert(text_item(&"p".repeat(1000), 1000).with_pinned(true));
    store.insert(text_item(&"u".repeat(1000), 2000));
    assert_eq!(store.len(), 1);
    assert_eq!(store.items()[0].id, pinned);
    assert_eq!(store.payload_bytes(), 1000);
}

#[test]
fn payload_bytes_counts_text_files_and_images() {
    let mut store = Store::new(10);
    store.insert(text_item("hello", 1000));
    assert_eq!(store.payload_bytes(), 5);
    store.insert(ClipItem::new(
        ClipKind::Files,
        None,
        vec!["C:\\one.txt".to_string(), "C:\\two.txt".to_string()],
        0,
        2000,
    ));
    assert_eq!(store.payload_bytes(), 5 + 10 + 10);

    let image = small_image();
    let item = ClipItem::new(ClipKind::Image, None, Vec::new(), image.encoded_len(), 3000)
        .with_preview("Image 32\u{d7}24")
        .with_image(image);
    let id = store.insert(item);
    let row = store.get(id).expect("image row");
    let retained = row.image.as_ref().expect("payload").retained_bytes();
    assert!(retained > 0);
    assert_eq!(store.payload_bytes(), 25 + retained);
    // The row's displayed size is the encoded size, not the raw pixels.
    assert_eq!(
        row.byte_len,
        retained - row.image.as_ref().expect("payload").thumb().bytes()
    );
}

/// A 32 x 24 32-bit DIB, encoded the way the clipboard listener does it.
fn small_image() -> ClipImage {
    let (w, h) = (32u32, 24u32);
    let stride = w as usize * 4;
    let mut dib = vec![0u8; 40 + stride * h as usize];
    dib[0..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&(w as i32).to_le_bytes());
    dib[8..12].copy_from_slice(&(h as i32).to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    for (i, byte) in dib[40..].iter_mut().enumerate() {
        *byte = (i % 251) as u8;
    }
    ClipImage::from_dib(&dib).expect("a plain 32-bit DIB")
}
