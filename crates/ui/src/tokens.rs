//! The design tokens: one colour table, one type scale, one spacing scale.
//!
//! Nothing in this crate may use a raw hex value outside this module (PRD §6.2).
//! Every text/background pair in [`Palette::LIGHT`] is asserted to clear WCAG
//! 4.5:1 in `tests::contrast`.

use egui::{Color32, FontId};

/// Spacing scale — the only values allowed as padding, margin or gap.
pub const SPACING: [f32; 10] = [2.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 56.0];

/// `true` when `v` is a member of the spacing scale (PRD UI-01).
#[must_use]
pub fn on_scale(v: f32) -> bool {
    SPACING.iter().any(|s| (s - v).abs() < f32::EPSILON)
}

/// Radii (PRD §6.2): keycap 4 · chip/thumb 8 · row highlight 10 · panel 16.
pub mod radius {
    pub const KEYCAP: f32 = 4.0;
    pub const CHIP: f32 = 8.0;
    pub const ROW: f32 = 10.0;
    pub const PANEL: f32 = 16.0;
    pub const PILL: f32 = 999.0;
}

/// Motion durations in seconds (PRD §6.2 / UI-10).
pub mod motion {
    pub const OPEN: f64 = 0.140;
    pub const CLOSE: f64 = 0.090;
    pub const SELECT: f64 = 0.080;
    pub const HOVER: f64 = 0.060;
    pub const FADE_OUT: f64 = 0.200;
}

/// One theme's colour roles. Field names mirror the PRD role table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub panel: Color32,
    pub chrome: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub row_divider: Color32,
    pub hairline: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_tertiary: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub warning_bg: Color32,
    pub warning_text: Color32,
    pub kind_text: Color32,
    pub kind_link: Color32,
    pub kind_image: Color32,
    pub kind_file: Color32,
    pub kind_code: Color32,
    pub keycap_fill: Color32,
    pub thumb: Color32,
    pub shadow_rgb: Color32,
    pub backdrop_top: Color32,
    pub backdrop_bottom: Color32,
    pub scrim: Color32,
    pub doc_fill: Color32,
    pub doc_chrome: Color32,
    pub doc_bar: Color32,
    pub doc_dot: Color32,
}

impl Palette {
    /// Light — the default, because most Windows documents are white.
    ///
    /// The alpha values on the four translucent tints are calibrated so the
    /// *composited pixel* matches the PRD tint: egui blends through a linear
    /// framebuffer, so a nominal `0.07` indigo lands far lighter than a
    /// gamma-space blend would. Opaque roles are exact and asserted in
    /// `tests::contrast_light_pairs_match_the_prd`.
    #[must_use]
    pub fn light() -> Self {
        Self {
            panel: rgb(0xFF, 0xFF, 0xFF),
            chrome: rgb(0xF8, 0xF9, 0xFB),
            row_hover: rgba(16, 24, 40, 0.020),
            row_selected: rgba(79, 70, 229, 0.100),
            row_divider: rgb(0xF2, 0xF3, 0xF6),
            hairline: rgb(0xE9, 0xEA, 0xEE),
            border_strong: rgb(0xD0, 0xD5, 0xDD),
            text_primary: rgb(0x10, 0x18, 0x28),
            text_secondary: rgb(0x47, 0x54, 0x67),
            text_tertiary: rgb(0x66, 0x70, 0x85),
            accent: rgb(0x4F, 0x46, 0xE5),
            accent_hover: rgb(0x43, 0x38, 0xCA),
            accent_soft: rgba(79, 70, 229, 0.150),
            on_accent: rgb(0xFF, 0xFF, 0xFF),
            danger: rgb(0xB4, 0x23, 0x18),
            warning_bg: rgb(0xFF, 0xFA, 0xEB),
            warning_text: rgb(0xB5, 0x47, 0x08),
            kind_text: rgb(0x34, 0x40, 0x54),
            kind_link: rgb(0x17, 0x5C, 0xD3),
            kind_image: rgb(0xB5, 0x47, 0x08),
            kind_file: rgb(0x02, 0x7A, 0x48),
            kind_code: rgb(0x69, 0x41, 0xC6),
            keycap_fill: rgb(0xFF, 0xFF, 0xFF),
            thumb: rgb(0xF2, 0xF4, 0xF7),
            shadow_rgb: rgb(16, 24, 40),
            backdrop_top: rgb(0xED, 0xEF, 0xF3),
            backdrop_bottom: rgb(0xE3, 0xE7, 0xED),
            scrim: rgba(16, 24, 40, 0.300),
            doc_fill: rgb(0xFF, 0xFF, 0xFF),
            doc_chrome: rgb(0xF7, 0xF8, 0xFA),
            doc_bar: rgb(0xDF, 0xE3, 0xEA),
            doc_dot: rgb(0xD5, 0xD9, 0xE0),
        }
    }

    /// Dark — the same roles, inverted weights, same accent family.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            panel: rgb(0x17, 0x19, 0x1F),
            chrome: rgb(0x1E, 0x21, 0x29),
            row_hover: rgba(255, 255, 255, 0.045),
            row_selected: rgba(124, 124, 247, 0.120),
            row_divider: rgba(255, 255, 255, 0.040),
            hairline: rgb(0x2A, 0x2E, 0x38),
            border_strong: rgb(0x3A, 0x3F, 0x4B),
            text_primary: rgb(0xF0, 0xF2, 0xF5),
            text_secondary: rgb(0xA3, 0xAA, 0xB8),
            text_tertiary: rgb(0x8B, 0x93, 0xA3),
            accent: rgb(0x8B, 0x8B, 0xF9),
            accent_hover: rgb(0xA0, 0xA0, 0xFF),
            accent_soft: rgba(124, 124, 247, 0.140),
            on_accent: rgb(0x17, 0x19, 0x1F),
            danger: rgb(0xF9, 0x70, 0x66),
            warning_bg: rgb(0x2B, 0x21, 0x10),
            warning_text: rgb(0xFD, 0xB0, 0x22),
            kind_text: rgb(0xC3, 0xC8, 0xD2),
            kind_link: rgb(0x84, 0xAD, 0xFF),
            kind_image: rgb(0xF7, 0x90, 0x09),
            kind_file: rgb(0x47, 0xCD, 0x89),
            kind_code: rgb(0xB6, 0x92, 0xF6),
            keycap_fill: rgb(0x1E, 0x21, 0x29),
            thumb: rgb(0x22, 0x26, 0x2E),
            shadow_rgb: rgb(0, 0, 0),
            backdrop_top: rgb(0x0E, 0x10, 0x14),
            backdrop_bottom: rgb(0x0A, 0x0B, 0x0E),
            scrim: rgba(0, 0, 0, 0.420),
            doc_fill: rgb(0x1B, 0x1E, 0x25),
            doc_chrome: rgb(0x21, 0x24, 0x2C),
            doc_bar: rgb(0x2A, 0x2E, 0x38),
            doc_dot: rgb(0x33, 0x38, 0x44),
        }
    }

    /// The tint a kind glyph takes when it is allowed to be colourful.
    #[must_use]
    pub fn kind(&self, kind: pastebox_core::ClipKind) -> Color32 {
        use pastebox_core::ClipKind;
        match kind {
            ClipKind::Text => self.kind_text,
            ClipKind::Image => self.kind_image,
            ClipKind::Files => self.kind_file,
            ClipKind::Link => self.kind_link,
        }
    }
}

/// The five-step type scale (PRD §6.2). Sizes only; line height is applied by
/// the layout constants.
pub mod type_size {
    pub const SEARCH: f32 = 17.0;
    pub const ROW_TITLE: f32 = 15.0;
    pub const ROW_META: f32 = 12.0;
    pub const PREVIEW_LABEL: f32 = 11.0;
    pub const PREVIEW_BODY: f32 = 13.0;
    pub const PREVIEW_MONO: f32 = 12.0;
    pub const CHIP: f32 = 12.0;
    pub const KBD: f32 = 11.0;
    pub const EMPTY_TITLE: f32 = 16.0;
    pub const EMPTY_BODY: f32 = 13.0;
    pub const BUTTON: f32 = 13.0;
}

/// A UI font at one of the scale sizes.
#[must_use]
pub fn ui(size: f32) -> FontId {
    FontId::proportional(size)
}

/// The built-in monospace face, used only for code-shaped preview bodies.
#[must_use]
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// `#RRGGBB`.
pub const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// `rgba(r,g,b,a)` with `a` in 0..=1.
#[must_use]
pub fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    let a = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// WCAG 2.1 relative luminance of an opaque colour.
#[must_use]
pub fn luminance(c: Color32) -> f32 {
    fn channel(v: u8) -> f32 {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG 2.1 contrast ratio between two opaque colours.
#[must_use]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() < 0.06
    }

    #[test]
    fn contrast_light_pairs_match_the_prd() {
        let p = Palette::light();
        let white = p.panel;
        assert!(close(contrast(p.text_primary, white), 17.75));
        assert!(close(contrast(p.text_secondary, white), 7.69));
        assert!(close(contrast(p.text_tertiary, white), 4.97));
        assert!(close(contrast(p.text_tertiary, p.chrome), 4.72));
        assert!(close(contrast(p.accent, white), 6.29));
        assert!(close(contrast(p.kind_link, white), 5.99));
        assert!(close(contrast(p.kind_image, white), 5.43));
        assert!(close(contrast(p.kind_file, white), 5.41));
        assert!(close(contrast(p.kind_code, white), 6.62));
    }

    #[test]
    fn contrast_dark_pairs_match_the_prd() {
        let p = Palette::dark();
        let panel = p.panel;
        assert!(close(contrast(p.text_primary, panel), 15.67));
        assert!(close(contrast(p.text_secondary, panel), 7.53));
        assert!(close(contrast(p.text_tertiary, panel), 5.69));
        assert!(close(contrast(p.text_tertiary, p.chrome), 5.21));
        assert!(close(contrast(p.accent, panel), 5.99));
        assert!(close(contrast(p.kind_link, panel), 7.87));
        assert!(close(contrast(p.kind_image, panel), 7.49));
        assert!(close(contrast(p.kind_file, panel), 8.67));
        assert!(close(contrast(p.kind_code, panel), 7.05));
    }

    #[test]
    fn every_body_pair_clears_four_and_a_half() {
        for p in [Palette::light(), Palette::dark()] {
            for fg in [
                p.text_primary,
                p.text_secondary,
                p.text_tertiary,
                p.accent,
                p.danger,
            ] {
                assert!(contrast(fg, p.panel) >= 4.5, "{fg:?} on panel");
                assert!(contrast(fg, p.chrome) >= 4.5, "{fg:?} on chrome");
            }
        }
    }

    #[test]
    fn spacing_scale_only() {
        for v in [2.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 32.0, 40.0, 56.0] {
            assert!(on_scale(v));
        }
        assert!(!on_scale(3.0));
        assert!(!on_scale(14.0));
    }

    #[test]
    fn durations_are_the_spec() {
        assert!((motion::OPEN - 0.140).abs() < 1e-9);
        assert!((motion::CLOSE - 0.090).abs() < 1e-9);
        assert!((motion::SELECT - 0.080).abs() < 1e-9);
        assert!((motion::HOVER - 0.060).abs() < 1e-9);
    }
}
