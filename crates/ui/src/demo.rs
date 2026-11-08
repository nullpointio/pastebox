//! The demo fixture: twelve plausible clips, aged against a frozen clock so the
//! hero screenshot is deterministic forever (`screenshot-spec.md` §6).

use std::collections::HashMap;

use pastebox_core::{ClipItem, ClipKind, Store};

use crate::{ClipMeta, ClipboardSource};

/// 2026-11-08T14:16:00+01:00, the fixture's injected `now`.
pub const DEMO_NOW_MS: u64 = 1_794_143_760_000;

/// How many clips the history claims to hold (`Search 187 clips…`).
pub const DEMO_TOTAL: usize = 187;

const LEASE: &str = "Hi Sarah — attaching the signed lease for unit 4B. Rent is €1,840/month starting 1 December, deposit is two months, and the parking space is included. Could you confirm receipt and send the countersigned copy back before Friday? I'll drop the keys at the agency on Monday morning.";

/// A deterministic [`ClipboardSource`] for the demo and the hero screenshot.
pub struct DemoSource {
    store: Store,
    meta: HashMap<u64, ClipMeta>,
    total: usize,
}

impl DemoSource {
    /// The twelve-item fixture: the eight hero rows first, then four older
    /// clips so the list is genuinely scrollable.
    #[must_use]
    pub fn hero() -> Self {
        let mut source = Self {
            store: Store::new(200),
            meta: HashMap::new(),
            total: DEMO_TOTAL,
        };

        source.push_text(LEASE, "OUTLOOK.EXE", 120, 3_000);
        source.push_image(
            "Screenshot 2026-11-08 141233.png",
            (1440, 900),
            319_488,
            "Snipping Tool",
            840,
        );
        source.push_link(
            "https://linear.app/pastebox/issue/PAS-142",
            "Chrome",
            1_560,
            43,
        );
        source.push_files(
            vec!["C:\\Users\\ada\\Documents\\Q4-forecast-v7.xlsx".to_string()],
            2_202_009,
            "EXCEL.EXE",
            2_460,
        );
        source.push_text(
            "cargo build --release --target x86_64-pc-windows-msvc",
            "WindowsTerminal.exe",
            3_600,
            53,
        );
        source.push_text("IBAN DE89 3704 0044 0532 0130 00", "Chrome", 7_200, 32);
        source.push_image(
            "receipt-2026-11-06.jpg",
            (3024, 4032),
            1_887_437,
            "Photos.exe",
            104_400,
        );
        source.push_text("#4F46E5", "Figma.exe", 104_460, 7);

        source.push_text(
            "Notes from the call with the landlord: the rent review is due in March, the building insurance moved to Aviva, and the alarm code changed on Tuesday. He'll send the new lease annex by post rather than email.",
            "Notion.exe",
            3 * 86_400,
            215,
        );
        source.push_files(
            vec![
                "C:\\Users\\ada\\Downloads\\invoice-march.pdf".to_string(),
                "C:\\Users\\ada\\Downloads\\receipt-march.pdf".to_string(),
                "C:\\Users\\ada\\Downloads\\statement-q1.csv".to_string(),
            ],
            1_933_312,
            "Slack.exe",
            4 * 86_400,
        );
        source.push_text(
            "fn main() {\n    let store = Store::new(200);\n    for clip in store.items() {\n        println!(\"{}\", clip.preview);\n    }\n}",
            "Code.exe",
            5 * 86_400,
            118,
        );
        source.push_link(
            "https://news.ycombinator.com/item?id=39120044",
            "Firefox",
            6 * 86_400,
            46,
        );

        source
    }

    /// An empty history, for the empty-state composition.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            store: Store::new(200),
            meta: HashMap::new(),
            total: 0,
        }
    }

    /// The frozen clock the fixture was aged against.
    #[must_use]
    pub fn now_ms(&self) -> u64 {
        DEMO_NOW_MS
    }

    fn at(&self, age_s: u64) -> u64 {
        DEMO_NOW_MS.saturating_sub(age_s * 1000)
    }

    fn record(&mut self, item: ClipItem, source_app: &str, dims: Option<(u32, u32)>) {
        let id = self.store.insert(item);
        self.meta.insert(
            id,
            ClipMeta {
                source_app: source_app.to_string(),
                dims,
                too_large: false,
            },
        );
    }

    fn push_text(&mut self, text: &str, source_app: &str, age_s: u64, bytes: usize) {
        let item = ClipItem::new(
            ClipKind::Text,
            Some(text.to_string()),
            Vec::new(),
            bytes,
            self.at(age_s),
        );
        self.record(item, source_app, None);
    }

    fn push_link(&mut self, url: &str, source_app: &str, age_s: u64, bytes: usize) {
        let item = ClipItem::new(
            ClipKind::Link,
            Some(url.to_string()),
            Vec::new(),
            bytes,
            self.at(age_s),
        );
        self.record(item, source_app, None);
    }

    fn push_files(&mut self, files: Vec<String>, bytes: usize, source_app: &str, age_s: u64) {
        let item = ClipItem::new(ClipKind::Files, None, files, bytes, self.at(age_s));
        self.record(item, source_app, None);
    }

    fn push_image(
        &mut self,
        title: &str,
        dims: (u32, u32),
        bytes: usize,
        source_app: &str,
        age_s: u64,
    ) {
        let item = ClipItem::new(ClipKind::Image, None, Vec::new(), bytes, self.at(age_s))
            .with_preview(title);
        self.record(item, source_app, Some(dims));
    }
}

impl ClipboardSource for DemoSource {
    fn clips(&self) -> &[ClipItem] {
        self.store.items()
    }

    fn search(&self, query: &str) -> Vec<usize> {
        self.store.search(query)
    }

    fn total(&self) -> usize {
        self.total
    }

    fn meta(&self, id: u64) -> ClipMeta {
        self.meta.get(&id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::relative_time;

    #[test]
    fn the_fixture_has_twelve_clips_newest_first() {
        let demo = DemoSource::hero();
        assert_eq!(demo.clips().len(), 12);
        let ages: Vec<String> = demo
            .clips()
            .iter()
            .map(|c| relative_time(demo.now_ms(), c.created_ms))
            .collect();
        assert_eq!(
            ages,
            [
                "2 min",
                "14 min",
                "26 min",
                "41 min",
                "1 h",
                "2 h",
                "Yesterday",
                "Yesterday",
                "3 d",
                "4 d",
                "5 d",
                "6 d"
            ]
        );
    }

    #[test]
    fn the_hero_rows_are_the_spec_rows() {
        let demo = DemoSource::hero();
        let kinds: Vec<ClipKind> = demo.clips().iter().take(8).map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            [
                ClipKind::Text,
                ClipKind::Image,
                ClipKind::Link,
                ClipKind::Files,
                ClipKind::Text,
                ClipKind::Text,
                ClipKind::Image,
                ClipKind::Text,
            ]
        );
        assert_eq!(demo.clips()[0].text.as_deref(), Some(LEASE));
        assert_eq!(demo.clips()[1].preview, "Screenshot 2026-11-08 141233.png");
        assert_eq!(demo.clips()[3].preview, "Q4-forecast-v7.xlsx");
        assert_eq!(demo.total(), 187);
    }

    #[test]
    fn search_finds_the_lease() {
        let demo = DemoSource::hero();
        let hits = demo.search("lease");
        assert!(hits.contains(&0), "the lease email must match: {hits:?}");
    }
}
