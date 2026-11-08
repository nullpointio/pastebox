//! Layout metrics. Every number here is from the redlines in PRD §6.3 and
//! `screenshot-spec.md` §2; nothing in the renderer invents a value.

use egui::{Pos2, Rect, Vec2};
use pastebox_core::ClipKind;

/// The hero composition: 960 × 600 logical px.
pub const PANEL_W: f32 = 960.0;
pub const PANEL_H: f32 = 600.0;
/// Below this width the preview pane is dropped (PRD §6.3 responsive rule).
pub const PREVIEW_BREAKPOINT: f32 = 880.0;

pub const HEADER_H: f32 = 64.0;
pub const CHIPS_H: f32 = 40.0;
pub const FOOTER_H: f32 = 46.0;
/// `600 − 64 − 40 − 1 − 46 − 1`
pub const BODY_H: f32 = 448.0;

pub const ROW_H: f32 = 56.0;
pub const LIST_W: f32 = 600.0;
pub const PREVIEW_W: f32 = 359.0;

pub const GUTTER: f32 = 24.0;
pub const ICON: f32 = 18.0;
pub const ICON_GAP: f32 = 12.0;
/// `24 gutter + 18 icon + 12 gap`
pub const TITLE_X: f32 = 54.0;
/// Right edge of the age column, from the panel's left edge.
pub const RIGHT_EDGE: f32 = 576.0;
/// Width available to a row title before the age column.
pub const TITLE_W: f32 = 480.0;
/// Width available to a row's meta line.
pub const META_W: f32 = 420.0;
/// Breathing room between the text column and the age column.
pub const COLUMN_GAP: f32 = 12.0;

/// The three horizontal columns of a history row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowColumns {
    /// The kind glyph (18 × 18, 19 px below the row's top).
    pub icon: Rect,
    /// Title and meta line. Text is clipped to this rect, so it can never
    /// reach the glyph column on the left or the age column on the right.
    pub text: Rect,
    /// The right-aligned age label's column.
    pub age: Rect,
}

/// Split `row` into its columns. `age_w` is the *measured* width of the age
/// label, so `now` leaves the title more room than `12 Oct 2025` does.
#[must_use]
pub fn row_columns(row: Rect, age_w: f32) -> RowColumns {
    let icon = Rect::from_min_size(
        Pos2::new(row.left() + GUTTER, row.top() + 19.0),
        Vec2::new(ICON, ICON),
    );
    let age_right = row.left() + RIGHT_EDGE;
    let age_left = (age_right - age_w.max(0.0)).max(icon.right() + COLUMN_GAP);
    let text_left = row.left() + TITLE_X;
    let text_right = (age_left - COLUMN_GAP).max(text_left);
    RowColumns {
        icon,
        text: Rect::from_min_max(
            Pos2::new(text_left, row.top()),
            Pos2::new(text_right, row.bottom()),
        ),
        age: Rect::from_min_max(
            Pos2::new(age_left, row.top()),
            Pos2::new(age_right, row.bottom()),
        ),
    }
}

pub const CHIP_H: f32 = 28.0;
pub const CHIP_GAP: f32 = 8.0;
pub const KEYCAP_H: f32 = 20.0;
pub const BUTTON_H: f32 = 32.0;

/// Exact row count of the 600 px hero: 448 / 56.
pub const HERO_ROWS: usize = 8;

/// The centered panel, clamped so a small window still gets a usable surface.
#[must_use]
pub fn panel_rect(canvas: Rect, width: f32, height: f32) -> Rect {
    let w = width.min(canvas.width()).max(1.0);
    let h = height.min(canvas.height()).max(1.0);
    Rect::from_center_size(canvas.center(), Vec2::new(w, h))
}

/// `true` when the composition is wide enough for the preview pane.
#[must_use]
pub fn shows_preview(panel_w: f32) -> bool {
    panel_w >= PREVIEW_BREAKPOINT
}

/// Width of the row list for a panel of `panel_w`.
#[must_use]
pub fn list_width(panel_w: f32) -> f32 {
    if shows_preview(panel_w) {
        LIST_W.min(panel_w - PREVIEW_W - 1.0)
    } else {
        panel_w
    }
}

/// Body height for a panel of `panel_h`: everything between the chips divider
/// and the footer divider.
#[must_use]
pub fn body_height(panel_h: f32) -> f32 {
    (panel_h - HEADER_H - CHIPS_H - 2.0 - FOOTER_H).max(0.0)
}

/// The column an empty or no-results composition is centred in: the whole
/// content width, list plus preview, so the block sits on the palette's axis
/// instead of in the 600 px list column.
#[must_use]
pub fn state_column(list: Rect, panel: Rect) -> Rect {
    Rect::from_min_size(
        list.min,
        Vec2::new((panel.right() - list.left()).max(1.0), list.height()),
    )
}

/// Whole rows that fit in a list of `list_h`.
#[must_use]
pub fn whole_rows(list_h: f32) -> usize {
    if list_h <= 0.0 {
        0
    } else {
        (list_h / ROW_H).floor() as usize
    }
}

/// The slot rectangle for a row, `slot` counted from the top of the visible list.
#[must_use]
pub fn row_rect(list: Rect, slot: usize) -> Rect {
    let top = list.top() + slot as f32 * ROW_H;
    Rect::from_min_size(Pos2::new(list.left(), top), Vec2::new(list.width(), ROW_H))
}

/// Largest valid `top_row` so the list never scrolls past its content.
#[must_use]
pub fn max_top_row(count: usize, visible: usize) -> usize {
    count.saturating_sub(visible)
}

/// Keep `top_row` inside `0..=max` and keep `selected` visible with at least
/// one row of context where the list allows it.
#[must_use]
pub fn scroll_to_show(top_row: usize, selected: usize, visible: usize, count: usize) -> usize {
    let max = max_top_row(count, visible);
    if visible == 0 || count == 0 {
        return 0;
    }
    let mut top = top_row.min(max);
    if selected < top {
        top = selected;
    } else if selected >= top + visible {
        top = selected + 1 - visible;
    }
    top.min(max)
}

/// Preview eyebrow copy for a kind, before the detail is appended.
#[must_use]
pub fn kind_slot(kind: ClipKind) -> usize {
    match kind {
        ClipKind::Text => 0,
        ClipKind::Image => 1,
        ClipKind::Files => 2,
        ClipKind::Link => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::on_scale;

    fn hero_panel() -> Rect {
        panel_rect(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0)),
            PANEL_W,
            PANEL_H,
        )
    }

    #[test]
    fn the_hero_panel_lands_at_160_100() {
        let p = hero_panel();
        assert_eq!((p.left(), p.top()), (160.0, 100.0));
        assert_eq!((p.width(), p.height()), (960.0, 600.0));
    }

    #[test]
    fn the_body_is_eight_rows_of_56() {
        assert_eq!(body_height(PANEL_H), BODY_H);
        assert_eq!(BODY_H, ROW_H * HERO_ROWS as f32);
        assert_eq!(whole_rows(BODY_H), HERO_ROWS);
    }

    #[test]
    fn a_440_panel_shows_five_whole_rows_and_a_partial() {
        let body = body_height(440.0);
        assert_eq!(whole_rows(body), 5);
        assert!(body > 5.0 * ROW_H && body < 6.0 * ROW_H);
    }

    #[test]
    fn title_offset_is_the_sum_of_the_gutter() {
        assert!((TITLE_X - (GUTTER + ICON + ICON_GAP)).abs() < f32::EPSILON);
        assert!(on_scale(TITLE_X - GUTTER - ICON));
    }

    #[test]
    fn every_layout_constant_sits_on_the_grid() {
        for v in [
            HEADER_H, CHIPS_H, FOOTER_H, BODY_H, ROW_H, LIST_W, GUTTER, ICON, ICON_GAP, CHIP_H,
            CHIP_GAP, KEYCAP_H, BUTTON_H, META_W,
        ] {
            // Heights come from the redlines (footer 46 is a spec value); every
            // padding and gap must be on the spacing scale instead.
            assert!((v / 2.0).fract() == 0.0, "{v} is off the 2 px grid");
        }
        assert_eq!(PANEL_W, LIST_W + 1.0 + PREVIEW_W);
        assert!(on_scale(GUTTER) && on_scale(ICON_GAP) && on_scale(CHIP_GAP));
        assert!(on_scale(KEYCAP_H) && on_scale(BUTTON_H));
    }

    #[test]
    fn rows_stack_on_exact_56_px_pitches() {
        let list = Rect::from_min_size(Pos2::new(160.0, 205.0), Vec2::new(600.0, BODY_H));
        let first = row_rect(list, 0);
        let last = row_rect(list, 7);
        assert_eq!(first.top(), 205.0);
        assert_eq!(last.top(), 205.0 + 7.0 * 56.0);
        assert_eq!(last.bottom(), 653.0);
        assert_eq!(last.bottom(), list.bottom());
    }

    #[test]
    fn preview_hides_below_the_breakpoint() {
        assert!(shows_preview(960.0));
        assert!(shows_preview(880.0));
        assert!(!shows_preview(879.0));
        assert_eq!(list_width(760.0), 760.0);
        assert_eq!(list_width(960.0), 600.0);
    }

    #[test]
    fn row_text_never_leaves_its_column() {
        let list = Rect::from_min_size(Pos2::new(160.0, 205.0), Vec2::new(600.0, BODY_H));
        let row = row_rect(list, 4);
        for age_w in [18.0, 38.0, 78.0, 120.0] {
            let c = row_columns(row, age_w);
            assert!(
                c.text.left() >= c.icon.right(),
                "title starts over the glyph"
            );
            assert!(
                c.text.left() >= row.left() + TITLE_X,
                "title starts left of the text column"
            );
            assert!(
                c.text.right() <= c.age.left() - COLUMN_GAP + 0.001,
                "text column runs into the age column"
            );
            assert!(c.age.right() <= row.right(), "age leaves the row");
        }
    }

    #[test]
    fn a_wide_window_keeps_the_same_row_columns() {
        let list = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(760.0, BODY_H));
        let c = row_columns(row_rect(list, 0), 30.0);
        assert_eq!(c.text.width(), RIGHT_EDGE - 30.0 - COLUMN_GAP - TITLE_X);
        assert_eq!(c.age.right(), RIGHT_EDGE);
    }

    #[test]
    fn scrolling_keeps_the_selection_inside_the_window() {
        assert_eq!(scroll_to_show(0, 0, 8, 30), 0);
        assert_eq!(scroll_to_show(0, 9, 8, 30), 2);
        assert_eq!(scroll_to_show(10, 3, 8, 30), 3);
        assert_eq!(scroll_to_show(30, 29, 8, 30), 22);
        assert_eq!(scroll_to_show(5, 0, 8, 3), 0);
    }

    #[test]
    fn state_blocks_centre_on_the_palette_not_the_list_column() {
        let panel = Rect::from_min_size(Pos2::new(160.0, 100.0), Vec2::new(PANEL_W, PANEL_H));
        let list = Rect::from_min_size(
            Pos2::new(panel.left(), 200.0),
            Vec2::new(list_width(panel.width()), 300.0),
        );
        let state = state_column(list, panel);
        assert_eq!(state.center().x, panel.center().x);
        assert_eq!(state.width(), PANEL_W);
        assert_eq!(state.top(), list.top());
        assert_eq!(state.height(), list.height());
        // A narrow panel has no preview to reclaim, so it keeps the list width.
        let narrow = Rect::from_min_size(Pos2::ZERO, Vec2::new(PREVIEW_BREAKPOINT - 40.0, 440.0));
        let narrow_list = Rect::from_min_size(
            Pos2::new(0.0, 60.0),
            Vec2::new(list_width(narrow.width()), 300.0),
        );
        assert_eq!(
            state_column(narrow_list, narrow).center().x,
            narrow.center().x
        );
    }
}
