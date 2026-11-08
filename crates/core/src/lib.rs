//! Pastebox core — the clipboard model, the search ranking, and pinned-only
//! persistence.
//!
//! Pure Rust, no Win32, no egui, no clock, no network. The only I/O in this
//! crate is the explicit [`save_pinned`] / [`load_pinned`] pair; nothing else
//! touches the filesystem, and nothing is ever persisted unless `save_pinned` is
//! called with an item whose `pinned` flag is `true`.
//!
//! ```no_run
//! use pastebox_core::{ClipItem, ClipKind, Store};
//!
//! let mut store = Store::new(500);
//! let id = store.insert(ClipItem::new(
//!     ClipKind::Text,
//!     Some("hello world".to_string()),
//!     Vec::new(),
//!     11,
//!     1_700_000_000_000,
//! ));
//! assert_eq!(store.search("hello"), vec![0]);
//! assert_eq!(store.items()[0].id, id);
//! ```
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![forbid(unsafe_code)]

mod clip;
mod image;
mod persist;
mod search;
mod store;

pub use clip::{format_bytes, ClipItem, ClipKind, PREVIEW_MAX_CHARS};
pub use image::{
    ClipImage, ImageOrigin, Thumbnail, MAX_IMAGE_BYTES, MAX_TEXT_BYTES, THUMB_MAX_SIDE,
};
pub use persist::{load_pinned, parse_pinned, save_pinned};
pub use search::{rank, score, NO_MATCH};
pub use store::{Store, DEFAULT_BYTE_BUDGET, DEFAULT_CAPACITY};
