mod common;

use common::{files_item, image_item, link_item, text_item};
use pastebox_core::{score, ClipItem, Store, NO_MATCH};

const MINUTE: u64 = 60_000;
const DAY: u64 = 24 * 60 * MINUTE;

fn fixture() -> Store {
    let mut store = Store::new(50);
    store.insert(text_item("invoice-march.pdf", 9_000));
    store.insert(text_item("INV-2291", 8_000));
    store.insert(image_item(
        "Screenshot 2026-11-08 141233.png",
        319_488,
        7_000,
    ));
    store.insert(files_item(&["C:\\Users\\me\\report.pdf"], 2048, 6_000));
    store.insert(link_item("https://example.com/invoices/2026", 5_000));
    store.insert(text_item("Café notes", 4_000));
    store
}

#[test]
fn ranking_table_is_exact() {
    let store = fixture();
    let cases: &[(&str, &[usize])] = &[
        ("inv", &[0, 1, 4]),
        ("pdf", &[3, 0]),
        ("cafe", &[5]),
        ("image", &[2]),
        ("report", &[3]),
        ("2026", &[2, 4]),
        ("zzz", &[]),
        ("", &[0, 1, 2, 3, 4, 5]),
    ];
    for (query, expected) in cases {
        assert_eq!(&store.search(query)[..], *expected, "query {query:?}");
    }
}

#[test]
fn search_is_case_insensitive() {
    let mut store = Store::new(10);
    store.insert(text_item("Hello World", 1000));
    assert_eq!(store.search("HELLO"), vec![0]);
    assert_eq!(store.search("hElLo wOrLd"), vec![0]);
}

#[test]
fn search_folds_accents() {
    let mut store = Store::new(10);
    store.insert(text_item("Café notes", 1000));
    assert_eq!(store.search("cafe"), vec![0]);
    assert_eq!(store.search("CAFÉ"), vec![0]);
}

#[test]
fn search_handles_emoji() {
    let mut store = Store::new(10);
    store.insert(text_item("🚀 launch checklist", 1000));
    assert_eq!(store.search("🚀"), vec![0]);
    assert_eq!(store.search("launch"), vec![0]);
    assert_eq!(store.search("🛰"), Vec::<usize>::new());
}

#[test]
fn search_matches_file_names_and_paths() {
    let mut store = Store::new(10);
    store.insert(files_item(
        &["C:\\Users\\me\\Documents\\invoice-march.pdf"],
        100,
        1000,
    ));
    assert_eq!(store.search("invoice"), vec![0]);
    assert_eq!(store.search("documents"), vec![0]);
    assert_eq!(store.search("pdf"), vec![0]);
}

#[test]
fn search_matches_the_kind_label() {
    let mut store = Store::new(10);
    store.insert(image_item("shot.png", 10, 1000));
    assert_eq!(store.search("image"), vec![0]);
    assert_eq!(store.search("shot"), vec![0]);
}

#[test]
fn substring_beats_all_tokens_beats_any_token() {
    let mut store = Store::new(10);
    // Newest first: the weakest match is the newest, to prove tier beats recency.
    store.insert(text_item("mar only", 9000));
    store.insert(text_item("mar and inv", 5000));
    store.insert(text_item("inv mar", 1000));
    assert_eq!(store.search("inv mar"), vec![2, 1, 0]);
}

#[test]
fn all_tokens_beat_a_single_token() {
    let mut store = Store::new(10);
    store.insert(text_item("red only", 9000));
    store.insert(text_item("car and red", 1000));
    assert_eq!(store.search("red car"), vec![1, 0]);
}

#[test]
fn empty_query_returns_everything_in_recency_order() {
    let mut store = Store::new(10);
    store.insert(text_item("a", 1000));
    store.insert(text_item("b", 2000));
    store.insert(text_item("c", 3000));
    assert_eq!(store.search(""), vec![0, 1, 2]);
    assert_eq!(store.search("   "), vec![0, 1, 2]);
}

#[test]
fn empty_query_lifts_pins_to_the_top() {
    let mut store = Store::new(10);
    store.insert(text_item("newest", 3000));
    store.insert(text_item("pinned", 2000).with_pinned(true));
    store.insert(text_item("oldest", 1000));
    assert_eq!(store.search(""), vec![1, 0, 2]);
}

#[test]
fn pinned_wins_a_near_tie() {
    let mut store = Store::new(10);
    store.insert(text_item("alpha one", DAY));
    store.insert(text_item("alpha two", 2 * DAY).with_pinned(true));
    assert_eq!(store.search("alpha"), vec![0, 1]);
}

#[test]
fn pin_boost_is_modest_and_loses_to_a_much_newer_clip() {
    let now = 10 * DAY;
    let pinned = text_item("alpha", 0).with_pinned(true);
    let fresh = text_item("alpha", now);
    assert!(score(&pinned, "alpha", now) < score(&fresh, "alpha", now));
}

#[test]
fn newer_wins_when_everything_else_ties() {
    let mut store = Store::new(10);
    store.insert(text_item("abc older", 1000));
    store.insert(text_item("abc newer", 2000));
    assert_eq!(store.search("abc"), vec![0, 1]);
}

#[test]
fn earlier_match_position_wins_at_equal_recency() {
    let mut store = Store::new(10);
    store.insert(text_item("target", 1000));
    store.insert(text_item("zzzz target", 1000));
    assert_eq!(store.search("target"), vec![1, 0]);
}

#[test]
fn no_match_is_an_empty_result() {
    let store = fixture();
    assert!(store.search("definitely-not-here").is_empty());
    assert_eq!(score(&text_item("alpha", 1), "beta", 1), NO_MATCH);
}

#[test]
fn empty_query_never_returns_no_match() {
    let item = text_item("alpha", 0);
    assert_ne!(score(&item, "", 1_000_000), NO_MATCH);
    assert_ne!(score(&item, "  \t ", 1_000_000), NO_MATCH);
}

#[test]
fn score_prefers_newer_at_equal_content() {
    let now = 10 * DAY;
    let newer = text_item("alpha", now);
    let older = text_item("alpha", now - DAY);
    assert!(score(&newer, "alpha", now) > score(&older, "alpha", now));
}

#[test]
fn score_is_pure_and_deterministic() {
    let item = text_item("the quick brown fox", 1234);
    let first = score(&item, "quick", 5000);
    for _ in 0..10 {
        assert_eq!(score(&item, "quick", 5000), first);
    }
    let mut store = Store::new(10);
    store.insert(item);
    let a = store.search("quick");
    let b = store.search("quick");
    assert_eq!(a, b);
}

#[test]
fn score_ignores_extra_whitespace_in_the_query() {
    let mut store = Store::new(10);
    store.insert(text_item("alpha beta", 1000));
    assert_eq!(store.search("   alpha    beta   "), vec![0]);
}

#[test]
fn search_indices_are_valid_and_unique() {
    let store = fixture();
    for query in ["", "inv", "2026", "pdf", "zzz"] {
        let hits = store.search(query);
        let mut seen = hits.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), hits.len(), "duplicate index for {query:?}");
        assert!(hits.iter().all(|&i| i < store.len()));
    }
}

#[test]
fn long_text_is_searchable_beyond_the_preview() {
    let mut body = "x".repeat(5000);
    body.push_str(" needle-in-the-haystack");
    let mut store = Store::new(10);
    store.insert(text_item(&body, 1000));
    assert_eq!(store.search("needle"), vec![0]);
    assert!(!store.items()[0].preview.contains("needle"));
}

#[test]
fn search_does_not_mutate_the_store() {
    let store = fixture();
    let before: Vec<u64> = store.items().iter().map(|i| i.id).collect();
    let _ = store.search("inv");
    let after: Vec<u64> = store.items().iter().map(|i| i.id).collect();
    assert_eq!(before, after);
}

#[test]
fn score_of_a_pinned_item_is_higher_than_the_same_unpinned_item() {
    let now = 1000;
    let base = text_item("alpha", now);
    let pinned = base.clone().with_pinned(true);
    assert_eq!(
        score(&pinned, "alpha", now) - score(&base, "alpha", now),
        500
    );
}

#[test]
fn image_clips_are_searchable_by_title_and_kind() {
    let mut store = Store::new(10);
    let mut item = image_item("Screenshot 2026-11-08 141233.png", 100, 1000);
    item.pinned = false;
    store.insert(item);
    assert_eq!(store.search("screenshot"), vec![0]);
    assert_eq!(store.search("png"), vec![0]);
    assert_eq!(store.search("image"), vec![0]);
}

#[test]
fn clip_with_no_text_or_files_is_still_found_by_kind() {
    let mut store = Store::new(10);
    store.insert(ClipItem::new(
        pastebox_core::ClipKind::Image,
        None,
        Vec::new(),
        0,
        1,
    ));
    assert_eq!(store.search("image"), vec![0]);
}
