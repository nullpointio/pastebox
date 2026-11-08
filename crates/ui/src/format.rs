//! Pure view-model helpers: relative ages, meta lines, width-aware truncation.
//!
//! Everything here is clock-injected and font-agnostic so it can be unit tested
//! on Linux without a window (`cargo test -p pastebox-ui`).

use pastebox_core::{format_bytes, ClipItem, ClipKind};

/// Month names for the long-tail age format (`12 Oct`).
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The right-hand age column: `now`, `2 min`, `1 h`, `Yesterday`, `3 d`,
/// `12 Oct`, `12 Oct 2025`.
#[must_use]
pub fn relative_time(now_ms: u64, then_ms: u64) -> String {
    let delta_ms = now_ms.saturating_sub(then_ms);
    let secs = delta_ms / 1000;
    if secs < 45 {
        return "now".to_string();
    }
    let mins = (secs / 60).max(1);
    if mins < 60 {
        return format!("{mins} min");
    }
    let now_day = (now_ms / 86_400_000) as i64;
    let then_day = (then_ms / 86_400_000) as i64;
    let days = (now_day - then_day).max(0);
    if days == 0 {
        return format!("{} h", secs / 3600);
    }
    if days == 1 {
        return "Yesterday".to_string();
    }
    if days < 7 {
        return format!("{days} d");
    }
    let (year, month, day) = civil_from_days(then_day);
    let (now_year, _, _) = civil_from_days(now_day);
    let name = MONTHS[(month as usize).saturating_sub(1).min(11)];
    if year == now_year {
        format!("{day} {name}")
    } else {
        format!("{day} {name} {year}")
    }
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch → (y, m, d).
/// Kept local so the crate needs no date dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `280 characters`, `1 character`, `2.1 MB`, `1440×900 · 312 KB`.
#[must_use]
pub fn detail(clip: &ClipItem, dims: Option<(u32, u32)>) -> String {
    match clip.kind {
        ClipKind::Text => count_label(clip, "character"),
        ClipKind::Link => url_host(clip.text.as_deref().unwrap_or(clip.preview.as_str()))
            .unwrap_or_else(|| count_label(clip, "character")),
        ClipKind::Files => {
            let n = clip.files.len();
            if n > 1 {
                format!("{n} files · {}", format_bytes(clip.byte_len))
            } else {
                format_bytes(clip.byte_len)
            }
        }
        ClipKind::Image => match dims.or_else(|| image_dims(&clip.preview)) {
            Some((w, h)) => format!("{w}×{h} · {}", format_bytes(clip.byte_len)),
            None => format_bytes(clip.byte_len),
        },
    }
}

fn count_label(clip: &ClipItem, unit: &str) -> String {
    let body = clip.text.as_deref().unwrap_or(clip.preview.as_str());
    let n = body.chars().count();
    if n == 1 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// `OUTLOOK.EXE · 280 characters`. A missing source collapses to the detail.
#[must_use]
pub fn meta_line(source_app: &str, clip: &ClipItem, dims: Option<(u32, u32)>) -> String {
    let detail = detail(clip, dims);
    if source_app.is_empty() {
        detail
    } else {
        format!("{source_app} · {detail}")
    }
}

/// `TEXT · 280 CHARACTERS` — the preview pane's eyebrow.
#[must_use]
pub fn preview_label(clip: &ClipItem, dims: Option<(u32, u32)>) -> String {
    format!(
        "{} · {}",
        kind_word(clip.kind).to_uppercase(),
        detail(clip, dims).to_uppercase()
    )
}

/// Human kind word, sentence case.
#[must_use]
pub fn kind_word(kind: ClipKind) -> &'static str {
    match kind {
        ClipKind::Text => "text",
        ClipKind::Image => "image",
        ClipKind::Files => "files",
        ClipKind::Link => "link",
    }
}

/// `linear.app` from `https://linear.app/pastebox/issue/PAS-142`.
#[must_use]
pub fn url_host(url: &str) -> Option<String> {
    let rest = url
        .split_once("://")
        .map_or(url, |(_, tail)| tail)
        .trim_start();
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .rsplit('@')
        .next()
        .unwrap_or(rest)
        .split(':')
        .next()
        .unwrap_or(rest);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() || !host.contains('.') {
        None
    } else {
        Some(host.to_string())
    }
}

/// `1440×900` out of a title that carries it as a standalone token.
#[must_use]
pub fn image_dims(preview: &str) -> Option<(u32, u32)> {
    let chars: Vec<char> = preview.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if i < chars.len() && matches!(chars[i], '×' | 'x' | 'X') {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            let before_ok = start == 0 || !chars[start - 1].is_alphanumeric();
            let after_ok = j == chars.len() || !chars[j].is_alphanumeric();
            if j > i + 1 && before_ok && after_ok {
                let w: String = chars[start..i].iter().collect();
                let h: String = chars[i + 1..j].iter().collect();
                if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                    if w > 0 && h > 0 {
                        return Some((w, h));
                    }
                }
            }
        }
        if i == start {
            i += 1;
        }
    }
    None
}

/// Whether a body should be set in monospace in the preview pane.
///
/// Single-line shell commands count (they are what people copy from a
/// terminal); multi-line bodies need the PRD's 60% structural-signal rule.
#[must_use]
pub fn is_code_like(text: &str) -> bool {
    const VERBS: [&str; 16] = [
        "cargo", "git", "npm", "pnpm", "yarn", "docker", "kubectl", "curl", "ssh", "python", "pip",
        "make", "rustc", "dotnet", "brew", "sudo",
    ];
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let Some(first) = lines.first() else {
        return false;
    };
    if lines.len() == 1 {
        let trimmed = first.trim_start();
        let verb = trimmed.split_whitespace().next().unwrap_or("");
        return VERBS.contains(&verb) || trimmed.starts_with('$') || trimmed.starts_with("./");
    }
    let flagged = lines
        .iter()
        .filter(|line| {
            line.starts_with(' ')
                || line.starts_with('\t')
                || line.contains("=>")
                || line.contains(';')
                || line.contains('{')
                || line.contains('}')
        })
        .count();
    flagged * 10 >= lines.len() * 6
}

/// Cut `text` to the widest prefix that still fits `max_w` once a single `…` is
/// appended. `measure` is injected so this is testable without a font atlas.
#[must_use]
pub fn truncate_to_width(text: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let budget = (max_w - measure("…")).max(0.0);
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect();
        if measure(&candidate) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut out: String = chars[..lo].iter().collect();
    let trimmed = out.trim_end().len();
    out.truncate(trimmed);
    out.push('…');
    out
}

/// `0 B on disk`, `1.2 MB on disk`.
#[must_use]
pub fn disk_label(bytes: usize) -> String {
    format!("{} on disk", format_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_791_000_000_000;

    fn clip(kind: ClipKind, text: &str) -> ClipItem {
        ClipItem::new(kind, Some(text.to_string()), Vec::new(), text.len(), NOW)
    }

    #[test]
    fn ages_use_the_fixture_vocabulary() {
        assert_eq!(relative_time(NOW, NOW), "now");
        assert_eq!(relative_time(NOW, NOW - 20_000), "now");
        assert_eq!(relative_time(NOW, NOW - 120_000), "2 min");
        assert_eq!(relative_time(NOW, NOW - 840_000), "14 min");
        assert_eq!(relative_time(NOW, NOW - 2_460_000), "41 min");
        assert_eq!(relative_time(NOW, NOW - 3_600_000), "1 h");
        assert_eq!(relative_time(NOW, NOW - 7_200_000), "2 h");
    }

    #[test]
    fn a_day_old_reads_as_yesterday() {
        let now = 20_765 * 86_400_000 + 13 * 3_600_000;
        assert_eq!(relative_time(now, now - 29 * 3_600_000), "Yesterday");
        assert_eq!(relative_time(now, now - 3 * 86_400_000), "3 d");
        assert_eq!(relative_time(now, now - 10 * 86_400_000), "29 Oct");
    }

    #[test]
    fn civil_dates_are_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_765), (2026, 11, 8));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn detail_reads_like_the_spec() {
        let text = "a".repeat(280);
        assert_eq!(detail(&clip(ClipKind::Text, &text), None), "280 characters");
        assert_eq!(detail(&clip(ClipKind::Text, "x"), None), "1 character");
        assert_eq!(
            detail(
                &clip(ClipKind::Link, "https://linear.app/pastebox/issue/PAS-142"),
                None
            ),
            "linear.app"
        );
        let img = ClipItem::new(ClipKind::Image, None, Vec::new(), 319_488, NOW)
            .with_preview("Screenshot 1440x900.png");
        assert_eq!(detail(&img, None), "1440×900 · 312 KB");
        assert_eq!(detail(&img, Some((1440, 900))), "1440×900 · 312 KB");
    }

    #[test]
    fn meta_line_joins_source_and_detail() {
        let c = clip(ClipKind::Text, &"a".repeat(32));
        assert_eq!(meta_line("Chrome", &c, None), "Chrome · 32 characters");
        assert_eq!(meta_line("", &c, None), "32 characters");
    }

    #[test]
    fn hosts_are_stripped_of_scheme_and_path() {
        assert_eq!(
            url_host("https://linear.app/pastebox/issue/PAS-142").as_deref(),
            Some("linear.app")
        );
        assert_eq!(
            url_host("http://www.example.com:8080/a").as_deref(),
            Some("example.com")
        );
        assert_eq!(url_host("not a url"), None);
    }

    #[test]
    fn code_detection_covers_the_hero_rows() {
        assert!(is_code_like(
            "cargo build --release --target x86_64-pc-windows-msvc"
        ));
        assert!(is_code_like("fn main() {\n    println!(\"hi\");\n}"));
        assert!(!is_code_like(
            "Hi Sarah — attaching the signed lease for unit 4B. Rent is €1,840/month."
        ));
        assert!(!is_code_like("IBAN DE89 3704 0044 0532 0130 00"));
    }

    #[test]
    fn truncation_is_one_line_one_ellipsis() {
        let measure = |s: &str| s.chars().count() as f32 * 10.0;
        let long = "a".repeat(300);
        let cut = truncate_to_width(&long, 512.0, measure);
        assert!(cut.ends_with('…'));
        assert_eq!(cut.chars().filter(|c| *c == '…').count(), 1);
        assert!(measure(&cut) <= 512.0);
        assert!(measure(&cut) > 480.0);
        assert_eq!(truncate_to_width("short", 512.0, measure), "short");
    }

    #[test]
    fn truncation_never_splits_a_multibyte_glyph() {
        let measure = |s: &str| s.chars().count() as f32 * 10.0;
        let text = "€€€€€€€€€€";
        let cut = truncate_to_width(text, 45.0, measure);
        assert!(cut.chars().all(|c| c == '€' || c == '…'));
        assert!(measure(&cut) <= 45.0);
    }

    #[test]
    fn preview_label_is_uppercase() {
        let c = clip(ClipKind::Text, &"a".repeat(280));
        assert_eq!(preview_label(&c, None), "TEXT · 280 CHARACTERS");
    }

    #[test]
    fn disk_label_uses_binary_units() {
        assert_eq!(disk_label(0), "0 B on disk");
    }
}
