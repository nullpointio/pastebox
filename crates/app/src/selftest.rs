//! `-selftest`: the core invariants plus the platform smoke checks.
//!
//! Kept out of `main.rs` so the entry point stays small. Everything here is
//! pure core logic or the platform's own smoke suite; no test framework is
//! involved because this has to run inside the shipped binary on a Windows
//! runner.

use std::env;
use std::fs;

use pastebox_core::{load_pinned, save_pinned, ClipItem, ClipKind, Store};

pub fn run() -> bool {
    let mut ok = true;
    ok &= store_behaviour();
    ok &= pins_roundtrip();
    ok &= platform();
    println!("selftest: {}", if ok { "OK" } else { "FAILED" });
    ok
}

fn check(name: &str, passed: bool) -> bool {
    println!("{} {name}", if passed { "PASS" } else { "FAIL" });
    passed
}

/// Insert, dedupe, pin protection and the capacity cap.
fn store_behaviour() -> bool {
    let mut store = Store::new(4);
    let text = |body: &str, at: u64| {
        ClipItem::new(
            ClipKind::Text,
            Some(body.to_string()),
            Vec::new(),
            body.len(),
            at,
        )
    };

    let first = store.insert(text("alpha", 10));
    let again = store.insert(text("alpha", 20));
    let deduped = again == first && store.len() == 1;

    store.pin(first, true);
    for n in 0..8u64 {
        store.insert(text(&format!("filler {n}"), 30 + n));
    }
    // 4 slots for unpinned rows plus the pinned one that may never be evicted.
    let capped = store.len() <= 5 && store.get(first).is_some();
    let searchable = !store.search("alpha").is_empty();

    store.clear_unpinned();
    let cleared = store.len() == 1 && store.get(first).is_some();

    // Evaluate every check so one failure does not hide the others.
    let dedupe = check("core: insert + dedupe", deduped);
    let cap = check("core: capacity cap keeps the pin", capped);
    let found = check("core: search finds the pinned row", searchable);
    let clear = check("core: clear unpinned keeps the pin", cleared);
    dedupe && cap && found && clear
}

/// Pinned-only persistence survives a write/read cycle and drops unpinned rows.
fn pins_roundtrip() -> bool {
    let dir = env::temp_dir().join(format!("pastebox-selftest-{}", std::process::id()));
    let path = dir.join("pins.txt");
    let mut store = Store::new(10);
    let kept = store.insert(ClipItem::new(
        ClipKind::Text,
        Some("keep me".to_string()),
        Vec::new(),
        7,
        1,
    ));
    store.insert(ClipItem::new(
        ClipKind::Text,
        Some("drop me".to_string()),
        Vec::new(),
        7,
        2,
    ));
    store.pin(kept, true);

    let saved = save_pinned(&path, store.items()).is_ok();
    let loaded = load_pinned(&path);
    let _ = fs::remove_dir_all(&dir);

    let only_the_pin = loaded.len() == 1
        && loaded
            .first()
            .is_some_and(|item| item.pinned && item.text.as_deref() == Some("keep me"));
    check("core: pins round trip", saved && only_the_pin)
}

#[cfg(windows)]
fn platform() -> bool {
    pastebox_platform::smoke()
}

#[cfg(not(windows))]
fn platform() -> bool {
    println!("SKIP platform: Windows only");
    true
}
