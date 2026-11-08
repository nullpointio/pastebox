//! The in-memory history: newest first, bounded, dedupe-on-insert.

use crate::clip::ClipItem;
use crate::search::rank;

/// PRD default history size.
pub const DEFAULT_CAPACITY: usize = 500;

/// A bounded, newest-first list of clips.
///
/// The only retention rule core enforces is the item cap: when full, the oldest
/// **unpinned** clip is evicted. Pinned clips are never evicted — if every slot
/// is pinned, the store grows rather than dropping a pin (the pin cap of 100 is
/// a UI/settings concern).
#[derive(Debug, Clone)]
pub struct Store {
    items: Vec<ClipItem>,
    capacity: usize,
    next_id: u64,
}

impl Store {
    /// `capacity` is clamped to at least 1 so a zero never produces a store that
    /// silently discards everything.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            items: Vec::new(),
            capacity: capacity.max(1),
            next_id: 1,
        }
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Insert a clip, or refresh the existing row if the content is identical.
    ///
    /// Dedupe compares kind + normalised content (see
    /// [`ClipItem::dedupe_key`]): the existing row keeps its `id` and pin state,
    /// takes the newer timestamp and the new display fields, and moves to the
    /// top. Returns the id of the row that now exists.
    pub fn insert(&mut self, mut item: ClipItem) -> u64 {
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
            existing.byte_len = item.byte_len;
            let id = existing.id;
            self.insert_sorted(existing);
            return id;
        }

        item.id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let id = item.id;
        self.insert_sorted(item);
        self.evict_to_capacity();
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

    fn evict_to_capacity(&mut self) {
        while self.items.len() > self.capacity {
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
