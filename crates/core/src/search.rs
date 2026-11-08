//! Ranking. [`score`] is pure: same item + query + clock, same number, forever.

use crate::clip::ClipItem;

/// Returned by [`score`] when the item does not match at all.
pub const NO_MATCH: i64 = i64::MIN;

// Tier floors: a whole tier always outranks everything in the tier below,
// because the modifiers (recency 0..=1000, pin 500, position 0..=400) cannot
// add up to a tier step of 1_000_000.
const TIER_SUBSTRING: i64 = 3_000_000;
const TIER_ALL_TOKENS: i64 = 2_000_000;
const TIER_ANY_TOKEN: i64 = 1_000_000;

/// "Modest": enough to win a near-tie, not enough to beat something four days
/// newer. Deliberately smaller than half the recency span.
const PIN_BOOST: i64 = 500;
const RECENCY_SPAN: i64 = 1_000;
const RECENCY_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const POSITION_BONUS_MAX: i64 = 400;

/// Rank a whole snapshot for `query`: indices, best first.
///
/// "Now" is the newest item's timestamp rather than a wall clock, so the result
/// depends only on the data handed in — no hidden time source, no flapping
/// between two calls a millisecond apart.
#[must_use]
pub fn rank(items: &[ClipItem], query: &str) -> Vec<usize> {
    let now_ms = items.iter().map(|item| item.created_ms).max().unwrap_or(0);
    let mut scored: Vec<(usize, i64)> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let value = score(item, query, now_ms);
            (value != NO_MATCH).then_some((index, value))
        })
        .collect();

    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| items[b.0].created_ms.cmp(&items[a.0].created_ms))
            .then_with(|| items[b.0].id.cmp(&items[a.0].id))
            .then_with(|| a.0.cmp(&b.0))
    });

    scored.into_iter().map(|(index, _)| index).collect()
}

/// Rank one item against a query.
///
/// * empty / whitespace query: every item scores (recency + pin boost)
/// * otherwise, case- and accent-insensitive:
///   1. the whole query is a substring → [`TIER_SUBSTRING`]
///   2. every whitespace-separated token is a substring → [`TIER_ALL_TOKENS`]
///   3. at least one token is a substring → [`TIER_ANY_TOKEN`]
///   4. nothing matches → [`NO_MATCH`]
///
/// Within a tier: earlier first match wins, then newer, then the pin boost.
#[must_use]
pub fn score(item: &ClipItem, query: &str, now_ms: u64) -> i64 {
    let folded = fold(query);
    let query = folded.trim();
    let base = recency_bonus(item.created_ms, now_ms) + if item.pinned { PIN_BOOST } else { 0 };
    if query.is_empty() {
        return base;
    }

    let hay = haystack(item);
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return base;
    }

    let (tier, pos) = match hay.find(query) {
        Some(p) => (TIER_SUBSTRING, p),
        None => {
            let mut all = true;
            let mut best: Option<usize> = None;
            for token in &tokens {
                match hay.find(token) {
                    Some(p) => best = Some(best.map_or(p, |b: usize| b.min(p))),
                    None => all = false,
                }
            }
            match best {
                None => return NO_MATCH,
                Some(p) if all => (TIER_ALL_TOKENS, p),
                Some(p) => (TIER_ANY_TOKEN, p),
            }
        }
    };

    tier + base + position_bonus(&hay, pos)
}

/// Everything searchable about a clip: body, title, file paths, kind label.
pub(crate) fn haystack(item: &ClipItem) -> String {
    let mut raw = String::new();
    if let Some(text) = &item.text {
        raw.push_str(text);
        raw.push(' ');
    }
    if !item.preview.is_empty() {
        raw.push_str(&item.preview);
        raw.push(' ');
    }
    for file in &item.files {
        raw.push_str(file);
        raw.push(' ');
    }
    raw.push_str(item.kind_label());
    fold(&raw)
}

fn recency_bonus(created_ms: u64, now_ms: u64) -> i64 {
    let age = now_ms
        .saturating_sub(created_ms)
        .min(RECENCY_WINDOW_MS as u64) as i64;
    RECENCY_SPAN - (RECENCY_SPAN * age / RECENCY_WINDOW_MS)
}

fn position_bonus(hay: &str, byte_pos: usize) -> i64 {
    let boundary = byte_pos.min(hay.len());
    // `byte_pos` always comes from `str::find`, so this is a char boundary.
    let chars = hay[..boundary].chars().count() as i64;
    (POSITION_BONUS_MAX - chars * 2).max(0)
}

/// Lowercase plus a small Latin-1 accent fold (PRD R-10 wants diacritic
/// insensitivity; this covers the accents that show up in real clip text
/// without pulling in a Unicode table).
pub(crate) fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(fold_char)
        .collect()
}

fn fold_char(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => 'a',
        'ç' => 'c',
        'è' | 'é' | 'ê' | 'ë' | 'ē' => 'e',
        'ì' | 'í' | 'î' | 'ï' | 'ī' => 'i',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' => 'o',
        'ù' | 'ú' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        other => other,
    }
}
