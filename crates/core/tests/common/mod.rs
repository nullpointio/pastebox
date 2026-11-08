#![allow(dead_code)]

use pastebox_core::{ClipItem, ClipKind};

pub fn text_item(text: &str, created_ms: u64) -> ClipItem {
    ClipItem::new(
        ClipKind::Text,
        Some(text.to_string()),
        Vec::new(),
        text.len(),
        created_ms,
    )
}

pub fn link_item(url: &str, created_ms: u64) -> ClipItem {
    ClipItem::new(
        ClipKind::Link,
        Some(url.to_string()),
        Vec::new(),
        url.len(),
        created_ms,
    )
}

pub fn files_item(files: &[&str], byte_len: usize, created_ms: u64) -> ClipItem {
    ClipItem::new(
        ClipKind::Files,
        None,
        files.iter().map(|f| (*f).to_string()).collect(),
        byte_len,
        created_ms,
    )
}

pub fn image_item(preview: &str, byte_len: usize, created_ms: u64) -> ClipItem {
    ClipItem::new(ClipKind::Image, None, Vec::new(), byte_len, created_ms).with_preview(preview)
}

pub fn indices_to_ids(store: &pastebox_core::Store, indices: &[usize]) -> Vec<u64> {
    indices.iter().map(|&i| store.items()[i].id).collect()
}
