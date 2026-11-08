//! The in-memory history: newest first, bounded, dedupe-on-insert.
//!
//! Two limits apply, because a count limit alone is not a memory policy: 500
//! clips of 1 MB text would be half a gigabyte. The item cap bounds how many
//! rows the list holds; the byte budget bounds what those rows actually cost.
//!
//! Per-item payloads are capped on the way in ([`MAX_TEXT_BYTES`],
//! [`MAX_IMAGE_BYTES`]). Text over the limit is truncated; an image over the
//! limit keeps its row and thumbnail but loses its pasteable pixels. Then the
//! oldest unpinned rows are evicted until the store fits both limits.

use crate::clip::ClipItem;
use crate::image::{MAX_IMAGE_BYTES, MAX_TEXT_BYTES};
use crate::search::rank;

/// PRD default history size.
pub const DEFAULT_CAPACITY: usize = 500;

/// Bytes of payload the history may hold before the oldest unpinned rows are
/// evicted. Sized so a screenful of screenshots plus a long tail of text fits
/// comfortably inside the product's memory budget.
pub const DEFAULT_BYTE_BUDGET: usize = 48 * 1024 * 1024;

/// A bounded, newest-first list of clips.
///
/// Eviction walks from the oldest row backwards and stops at pins: pinned clips
/// are never evicted, so if every remaining row is pinned the store grows rather
/// than dropping one (the pin cap of 100 is a UI/settings concern).
#[derive(Debug, Clone)]
pub struct Store {
    items: Vec<ClipItem>,
    capacity: usize,
    byte_budget: usize,
    next_id: u64,
}

impl Store {
    /// `capacity` is clamped to at least 1 so a zero never produces a store that
    /// silently discards everything.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self::with_budget(capacity, DEFAULT_BYTE_BUDGET)
    }

    /// A store with an explicit payload budget, for tests and for callers that
    /// want a tighter memory ceiling than the default.
    #[must_use]
    pub fn with_budget(capacity: usize, byte_budget: usize) -> Self {
        Self {
            items: Vec::new(),
            capacity: capacity.max(1),
            byte_budget: byte_budget.max(1),
            next_id: 1,
        }
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    #[must_use]
    pub fn byte_budget(&self) -> usize {
        self.byte_budget
    }

    /// Payload bytes currently held, as [`ClipItem::payload_bytes`] counts them.
    #[must_use]
    pub fn payload_bytes(&self) -> usize {
        self.items.iter().map(ClipItem::payload_bytes).sum()
    }

    /// Insert a clip, or refresh the existing row if the content is identical.
    ///
    /// Dedupe compares kind + normalised content (see
    /// [`ClipItem::dedupe_key`]): the existing row keeps its `id` and pin state,
    /// takes the newer timestamp and the new display fields, and moves to the
    /// top. Returns the id of the row that now exists.
    ///
    /// The payload caps are applied here rather than at capture time so every
    /// caller — the clipboard listener, the demo, a future importer — gets the
    /// same guarantee.
    pub fn insert(&mut self, mut item: ClipItem) -> u64 {
        apply_caps(&mut item);
        let (kind, key) = item.dedupe_key();

        if let Some(pos) = self
            .items
            .iter()
            .position(|existing| existing.kind == kind && existing.dedupe_key().1 == key)
        {
            let mut existing = self.items.remove(pos);
            existing.created_ms = existing.created_ms.max(item.created_ms);
            existing.text = item.text;
            existing.preview = item.preview;
            existing.files = item.files;
            existing.image = item.image;
            existing.byte_len = item.byte_len;
            let id = existing.id;
            self.insert_sorted(existing);
            self.evict_to_limits();
            return id;
        }

        item.id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = item.id;
        self.insert_sorted(item);
        self.evict_to_limits();
        id
    }

    /// Keeps `items` ordered by `created_ms` descending, so `items()[0]` really
    /// is the newest clip even if a caller hands us timestamps out of order.
    /// Equal timestamps put the most recent insertion first, which is what
    /// "dedupe moves it to the top" means for a clock with 1 ms resolution.
    fn insert_sorted(&mut self, item: ClipItem) {
        let pos = self
            .items
            .iter()
            .position(|existing| existing.created_ms <= item.created_ms)
            .unwrap_or(self.items.len());
        self.items.insert(pos, item);
    }

    fn evict_to_limits(&mut self) {
        while self.items.len() > self.capacity || self.payload_bytes() > self.byte_budget {
            match self.items.iter().rposition(|item| !item.pinned) {
                Some(pos) => {
                    self.items.remove(pos);
                }
                None => break,
            }
        }
    }

    /// Newest first.
    #[must_use]
    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }

    #[must_use]
    pub fn get(&self, id: u64) -> Option<&ClipItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Indices into [`Store::items`], best first.
    ///
    /// "Now" is the newest clip's timestamp rather than a wall clock, so the
    /// result depends only on the store's contents — no hidden time source, no
    /// flapping between calls.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<usize> {
        rank(&self.items, query)
    }

    pub fn pin(&mut self, id: u64, pinned: bool) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.pinned = pinned;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
    }

    /// Delete every unpinned clip. Pins survive.
    pub fn clear_unpinned(&mut self) {
        self.items.retain(|item| item.pinned);
    }

    /// Delete everything, pins included. Only for an explicit user "forget
    /// everything" action; the UI's clear-history path uses
    /// [`Store::clear_unpinned`].
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Pinned clips, newest first, cloned — the exact argument for
    /// [`crate::save_pinned`].
    #[must_use]
    pub fn pinned_items(&self) -> Vec<ClipItem> {
        self.items
            .iter()
            .filter(|item| item.pinned)
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

/// Enforce the per-item payload caps.
fn apply_caps(item: &mut ClipItem) {
    if let Some(text) = item.text.as_mut() {
        if text.len() > MAX_TEXT_BYTES {
            let cut = floor_char_boundary(text, MAX_TEXT_BYTES);
            text.truncate(cut);
            text.shrink_to_fit();
        }
    }
    if let Some(image) = item.image.as_mut() {
        if image.encoded_len() > MAX_IMAGE_BYTES {
            image.drop_payload();
        }
    }
}

/// Largest index `<= max` that lies on a `char` boundary.
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}
