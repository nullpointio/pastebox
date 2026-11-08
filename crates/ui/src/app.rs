//! The palette surface: header, filter chips, rows, preview pane, footer.
//!
//! Everything is painted at absolute rects derived from [`crate::layout`]; the
//! only real egui widgets are the search field and the hit-test rectangles, so
//! no default widget chrome can leak into the composition (PRD UI-05).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use egui::{Align2, Color32, FontId, Id, Pos2, Rect, Rounding, Sense, Shape, Stroke, Ui, Vec2};
use pastebox_core::{ClipImage, ClipItem, ClipKind};

use crate::format::{self, truncate_to_width};
use crate::icons::{self, Icon};
use crate::layout::{self, *};
use crate::tokens::{self, radius, type_size, Palette};
use crate::{backdrop, ClipMeta, ClipboardSource};

/// Which palette the surface is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Paper-white, the default.
    #[default]
    Light,
    /// The same roles, inverted.
    Dark,
}

/// What sits behind the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Backdrop {
    /// The demo composition: a blurred document under a scrim.
    #[default]
    Desktop,
    /// The shipped app: a transparent window over the real desktop.
    None,
}

/// The list filter. `Pinned` is reachable from the app crate but has no chip in
/// the five-chip hero row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Text,
    Images,
    Files,
    Links,
    Pinned,
}

impl Filter {
    /// The five chips, in `Ctrl+1..5` order.
    pub const CHIPS: [(Filter, &'static str); 5] = [
        (Filter::All, "All"),
        (Filter::Text, "Text"),
        (Filter::Images, "Images"),
        (Filter::Files, "Files"),
        (Filter::Links, "Links"),
    ];

    /// Chip label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Text => "Text",
            Filter::Images => "Images",
            Filter::Files => "Files",
            Filter::Links => "Links",
            Filter::Pinned => "Pinned",
        }
    }

    /// Whether a clip of `kind` passes this filter.
    #[must_use]
    pub fn accepts(self, kind: ClipKind, pinned: bool) -> bool {
        match self {
            Filter::All => true,
            Filter::Text => kind == ClipKind::Text,
            Filter::Images => kind == ClipKind::Image,
            Filter::Files => kind == ClipKind::Files,
            Filter::Links => kind == ClipKind::Link,
            Filter::Pinned => pinned,
        }
    }

    fn index(self) -> usize {
        match self {
            Filter::All => 0,
            Filter::Text => 1,
            Filter::Images => 2,
            Filter::Files => 3,
            Filter::Links => 4,
            Filter::Pinned => 5,
        }
    }

    /// Plural noun for "no … clips" copy.
    fn noun(self) -> &'static str {
        match self {
            Filter::All => "clips",
            Filter::Text => "text clips",
            Filter::Images => "image clips",
            Filter::Files => "file clips",
            Filter::Links => "link clips",
            Filter::Pinned => "pinned clips",
        }
    }

    /// Headline for a filter that matched nothing in a non-empty history.
    fn empty_title(self) -> &'static str {
        match self {
            Filter::All => "No clips",
            Filter::Text => "No text clips",
            Filter::Images => "No image clips",
            Filter::Files => "No file clips",
            Filter::Links => "No link clips",
            Filter::Pinned => "Nothing pinned",
        }
    }

    /// Second line for a filter that matched nothing.
    fn empty_body(self, total: usize) -> String {
        let none = if total == 1 {
            "Your only clip isn't".to_string()
        } else {
            format!("None of your {total} clips is")
        };
        match self {
            Filter::All => "No clips are hidden by this view.".to_string(),
            Filter::Text => format!("{none} text. Copy some text and it appears here."),
            Filter::Images => format!("{none} an image. Copy a picture and it appears here."),
            Filter::Files => format!("{none} a file. Copy a file and it appears here."),
            Filter::Links => format!("{none} a link. Copy a URL and it appears here."),
            Filter::Pinned => {
                "Select a clip and press Ctrl P to keep it across restarts.".to_string()
            }
        }
    }

    fn from_index(i: usize) -> Self {
        match i {
            1 => Filter::Text,
            2 => Filter::Images,
            3 => Filter::Files,
            4 => Filter::Links,
            5 => Filter::Pinned,
            _ => Filter::All,
        }
    }
}

/// What the user asked for. The UI never performs an effect itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Paste the clip into the window that had focus before the palette opened.
    Paste { id: u64, plain: bool },
    /// Put the clip back on the clipboard without sending a keystroke.
    Copy { id: u64 },
    /// Pin or unpin.
    Pin { id: u64, pinned: bool },
    /// Delete one clip.
    Remove { id: u64 },
    /// Delete every unpinned clip.
    ClearUnpinned,
    /// Dismiss the palette.
    Close,
}

#[derive(Debug, Clone)]
struct Toast {
    text: String,
    danger: bool,
    until: f64,
}

/// Where the surface sends the user's intents.
pub type ActionSink = Box<dyn FnMut(Action) -> Result<(), String>>;

/// The application surface.
pub struct PasteboxApp {
    source: Box<dyn ClipboardSource>,
    sink: Option<ActionSink>,
    theme: Theme,
    backdrop: Backdrop,
    query: String,
    filter: Filter,
    selected: usize,
    top_row: usize,
    matches: Vec<usize>,
    visible_rows: usize,
    now_ms: u64,
    focus_search: bool,
    toast: Option<Toast>,
    action: Option<Action>,
    last_query: String,
    time: f64,
    /// Row thumbnails uploaded to the GPU, keyed by clip id. Only the rows in
    /// the viewport keep a texture; everything else is freed each frame.
    thumbs: RefCell<ThumbCache>,
    /// The one full-size preview texture. Replaced (and the old one freed)
    /// whenever the selection changes.
    preview_tex: RefCell<Option<(u64, egui::TextureHandle)>>,
}

impl PasteboxApp {
    /// Build the surface over any [`ClipboardSource`].
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut source: Box<dyn ClipboardSource>,
        now_ms: u64,
    ) -> Self {
        let ctx = cc.egui_ctx.clone();
        source.attach(Box::new(move || ctx.request_repaint()));
        Self::bare(source, now_ms)
    }

    /// Construct without an egui context, so headless tests can drive the
    /// key handler directly.
    fn bare(source: Box<dyn ClipboardSource>, now_ms: u64) -> Self {
        let mut app = Self {
            source,
            sink: None,
            theme: Theme::Light,
            backdrop: Backdrop::None,
            query: String::new(),
            filter: Filter::All,
            selected: 0,
            top_row: 0,
            matches: Vec::new(),
            visible_rows: HERO_ROWS,
            now_ms,
            focus_search: true,
            toast: None,
            action: None,
            last_query: String::new(),
            time: 0.0,
            thumbs: RefCell::new(ThumbCache::default()),
            preview_tex: RefCell::new(None),
        };
        app.refresh();
        app
    }

    /// The hero composition: demo fixture, desktop backdrop, no caret.
    pub fn demo(cc: &eframe::CreationContext<'_>) -> Self {
        let demo = crate::DemoSource::hero();
        let now = demo.now_ms();
        let mut app = Self::new(cc, Box::new(demo), now);
        app.backdrop = Backdrop::Desktop;
        app.focus_search = false;
        app
    }

    /// Pick a palette.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Pick what sits behind the panel.
    #[must_use]
    pub fn with_backdrop(mut self, backdrop: Backdrop) -> Self {
        self.backdrop = backdrop;
        self
    }

    /// Attach the sink that receives the user's intents.
    #[must_use]
    pub fn with_sink(mut self, sink: ActionSink) -> Self {
        self.sink = Some(sink);
        self
    }

    /// Start with a query already typed (the `-demo --state` hook).
    #[must_use]
    pub fn with_query(mut self, query: &str) -> Self {
        self.query = query.to_string();
        self.last_query = query.to_string();
        self.refresh();
        self
    }

    /// Start with the selection on a given visible row.
    #[must_use]
    pub fn with_selection(mut self, index: usize) -> Self {
        self.selected = index.min(self.matches.len().saturating_sub(1));
        self.top_row = layout::scroll_to_show(
            self.top_row,
            self.selected,
            self.visible_rows.max(1),
            self.matches.len(),
        );
        self
    }

    /// Start on a filter.
    #[must_use]
    pub fn with_filter(mut self, filter: Filter) -> Self {
        self.filter = filter;
        self.selected = 0;
        self.refresh();
        self
    }

    /// Take the pending intent, if any.
    pub fn take_action(&mut self) -> Option<Action> {
        self.action.take()
    }

    /// The currently selected clip id.
    #[must_use]
    pub fn selected_id(&self) -> Option<u64> {
        self.matches
            .get(self.selected)
            .map(|&i| self.source.clips()[i].id)
    }

    fn palette(&self) -> Palette {
        match self.theme {
            Theme::Light => Palette::light(),
            Theme::Dark => Palette::dark(),
        }
    }

    fn refresh(&mut self) {
        let clips = self.source.clips();
        let indices = self.source.search(&self.query);
        self.matches = indices
            .into_iter()
            .filter(|&i| {
                clips
                    .get(i)
                    .is_some_and(|c| self.filter.accepts(c.kind, c.pinned))
            })
            .collect();
        self.selected = self.selected.min(self.matches.len().saturating_sub(1));
        self.top_row = layout::scroll_to_show(
            self.top_row,
            self.selected,
            self.visible_rows.max(1),
            self.matches.len(),
        );
    }

    fn toast(&mut self, text: impl Into<String>, danger: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            danger,
            until: self.time + 2.0,
        });
    }

    fn selected_clip(&self) -> Option<&ClipItem> {
        self.matches
            .get(self.selected)
            .and_then(|&i| self.source.clips().get(i))
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Shortcuts are matched against the modifiers carried by the key event,
        // never the frame-level `InputState::modifiers`. A backend that does
        // not forward `ModifiersChanged` (Wine's winit among them) leaves the
        // frame state all-false while Ctrl is physically held, so a
        // frame-state check silently drops every Ctrl shortcut — including
        // Ctrl+P, Ctrl+1…5 — even though the `Event::Key` says `ctrl: true`.
        let shift = ctx.input(|i| i.modifiers.shift)
            || ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { pressed: true, modifiers, .. } if modifiers.shift)
                })
            });

        let mut moved = false;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) {
                self.selected = (self.selected + 1).min(self.matches.len().saturating_sub(1));
                moved = true;
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
                moved = true;
            }
            if i.key_pressed(egui::Key::Home) {
                self.selected = 0;
                moved = true;
            }
            if i.key_pressed(egui::Key::End) {
                self.selected = self.matches.len().saturating_sub(1);
                moved = true;
            }
            if i.key_pressed(egui::Key::PageDown) {
                self.selected =
                    (self.selected + self.visible_rows).min(self.matches.len().saturating_sub(1));
                moved = true;
            }
            if i.key_pressed(egui::Key::PageUp) {
                self.selected = self.selected.saturating_sub(self.visible_rows);
                moved = true;
            }
        });

        if moved {
            self.top_row = layout::scroll_to_show(
                self.top_row,
                self.selected,
                self.visible_rows.max(1),
                self.matches.len(),
            );
        }

        for (n, key) in [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
        ]
        .into_iter()
        .enumerate()
        {
            if handles_command(ctx, key) {
                self.filter = Filter::from_index(n);
                self.selected = 0;
                self.refresh();
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
            let step: isize = if shift { -1 } else { 1 };
            let next = (self.filter.index() as isize + step).rem_euclid(5) as usize;
            self.filter = Filter::from_index(next);
            self.selected = 0;
            self.refresh();
        }

        if let Some(id) = self.selected_id() {
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                if handles_command(ctx, egui::Key::Enter) {
                    self.action = Some(Action::Copy { id });
                    self.toast("Copied", false);
                } else {
                    self.action = Some(Action::Paste { id, plain: shift });
                    if shift {
                        self.toast("Copied — press Ctrl+V", false);
                    }
                }
            }
            if handles_command(ctx, egui::Key::P) {
                let pinned = self.selected_clip().map(|c| !c.pinned).unwrap_or(false);
                self.action = Some(Action::Pin { id, pinned });
                self.toast(if pinned { "Pinned" } else { "Unpinned" }, false);
            }
            if !handles_command(ctx, egui::Key::Delete)
                && ctx.input(|i| i.key_pressed(egui::Key::Delete))
            {
                self.action = Some(Action::Remove { id });
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.action = Some(Action::Close);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for PasteboxApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        match self.backdrop {
            Backdrop::Desktop => [0.0, 0.0, 0.0, 1.0],
            Backdrop::None => [0.0, 0.0, 0.0, 0.0],
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.time = ctx.input(|i| i.time);
        if self.source.sync() {
            self.refresh();
        }
        if self.source.take_show() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.focus_search = true;
        }
        if self.query != self.last_query {
            self.last_query.clone_from(&self.query);
            self.refresh();
        }
        if self.toast.as_ref().is_some_and(|t| t.until < self.time) {
            self.toast = None;
        }
        if self.toast.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }

        self.handle_keys(ctx);

        let pal = self.palette();
        let mut visuals = match self.theme {
            Theme::Light => egui::Visuals::light(),
            Theme::Dark => egui::Visuals::dark(),
        };
        visuals.override_text_color = Some(pal.text_primary);
        visuals.panel_fill = pal.panel;
        visuals.window_fill = pal.panel;
        visuals.extreme_bg_color = pal.panel;
        visuals.selection.bg_fill = pal.accent.gamma_multiply(0.22);
        visuals.selection.stroke = Stroke::new(1.0_f32, pal.accent);
        visuals.text_cursor.stroke = Stroke::new(1.5_f32, pal.accent);
        visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        ctx.set_visuals(visuals);

        let canvas = ctx.screen_rect();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
            .show(ctx, |ui| {
                self.paint(ui, canvas);
            });

        self.dispatch();
    }
}

impl PasteboxApp {
    /// Hand the pending intent to the sink. Runs on the UI thread: the sink
    /// must post a command, never block on the platform.
    fn dispatch(&mut self) {
        let Some(action) = self.action.take() else {
            return;
        };
        if let Some(sink) = self.sink.as_mut() {
            if let Err(err) = sink(action) {
                self.toast(err, true);
            }
        }
    }
}

/// Whether `key` was pressed this frame while Ctrl (or Cmd) was held.
///
/// Reads the modifiers off the key event itself: the frame-level
/// `InputState::modifiers` is not dependable, because a backend that never
/// delivers `ModifiersChanged` — Wine's winit does not — leaves it all-false
/// while Ctrl is physically down.
fn handles_command(ctx: &egui::Context, key: egui::Key) -> bool {
    ctx.input(|i| {
        i.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key { key: pressed, pressed: true, modifiers, .. }
                    if *pressed == key && (modifiers.command || modifiers.ctrl)
            )
        })
    })
}

impl PasteboxApp {
    fn paint(&mut self, ui: &mut Ui, canvas: Rect) {
        let pal = self.palette();
        let panel = layout::panel_rect(canvas, PANEL_W, PANEL_H);
        let painter = ui.painter().clone();

        if self.backdrop == Backdrop::Desktop {
            backdrop::paint(&painter, canvas, &pal);
        }
        paint_shadow(&painter, panel, &pal);
        painter.rect_filled(panel, Rounding::same(radius::PANEL), pal.panel);
        painter.rect_stroke(
            panel,
            Rounding::same(radius::PANEL),
            Stroke::new(1.0_f32, tokens::rgba(16, 24, 40, 0.06)),
        );

        let w = panel.width();
        let header = Rect::from_min_size(panel.min, Vec2::new(w, HEADER_H));
        painter.rect_filled(
            header,
            Rounding {
                nw: radius::PANEL,
                ne: radius::PANEL,
                sw: 0.0,
                se: 0.0,
            },
            pal.chrome,
        );
        let chips_row = Rect::from_min_size(
            Pos2::new(panel.left(), panel.top() + HEADER_H),
            Vec2::new(w, CHIPS_H),
        );
        painter.rect_filled(chips_row, 0.0, pal.chrome);
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(panel.left(), panel.top() + HEADER_H + CHIPS_H),
                Vec2::new(w, 1.0),
            ),
            0.0,
            pal.hairline,
        );

        self.paint_header(ui, &painter, &pal, panel);
        self.paint_chips(ui, &painter, &pal, panel);

        let body_h = layout::body_height(panel.height());
        let body_top = panel.top() + HEADER_H + CHIPS_H + 1.0;
        let list_w = layout::list_width(panel.width());
        let list =
            Rect::from_min_size(Pos2::new(panel.left(), body_top), Vec2::new(list_w, body_h));
        let preview = layout::shows_preview(panel.width());

        self.visible_rows = layout::whole_rows(body_h).max(1);

        let matches = self.matches.clone();
        let history_empty = self.source.clips().is_empty();
        let history_total = self.source.total();
        let filter_now = self.filter;
        let clips = self.source.clips();
        let mut need_refresh = false;
        // Ids whose row thumbnail stays resident this frame. Anything not in
        // here loses its GPU texture at the end of `paint`.
        let mut onscreen: HashSet<u64> = HashSet::new();

        if matches.is_empty() {
            // Empty states own the whole content width — no preview pane and no
            // divider — so the composition is centred on the palette rather
            // than in the 600 px list column.
            let content = layout::state_column(list, panel);
            if history_empty {
                paint_empty(ui, &painter, &pal, content);
            } else if self.query.trim().is_empty() {
                if paint_filter_empty(ui, &painter, &pal, content, filter_now, history_total) {
                    self.filter = Filter::All;
                    self.selected = 0;
                    need_refresh = true;
                }
            } else if paint_no_results(ui, &painter, &pal, content, &self.query, filter_now) {
                self.query.clear();
                need_refresh = true;
            }
        } else {
            let clip_list = painter.with_clip_rect(list);
            let slots = (body_h / ROW_H).ceil() as usize + 1;
            let mut clicked: Option<usize> = None;
            let mut dbl: Option<usize> = None;

            for slot in 0..slots {
                let idx = self.top_row + slot;
                if idx >= matches.len() {
                    break;
                }
                let clip = &clips[matches[idx]];
                let meta = self.source.meta(clip.id);
                let row = layout::row_rect(list, slot);
                if row.top() >= list.bottom() {
                    break;
                }
                let selected = idx == self.selected;
                let resp = ui.interact(row, Id::new(("row", clip.id)), Sense::click());
                let hovered = resp.hovered();

                onscreen.insert(clip.id);
                paint_row(
                    &clip_list,
                    &pal,
                    row,
                    clip,
                    &meta,
                    selected,
                    hovered,
                    self.now_ms,
                    &self.thumbs,
                    ui.ctx(),
                );

                if resp.clicked() {
                    clicked = Some(idx);
                }
                if resp.double_clicked() {
                    dbl = Some(idx);
                }
            }

            if let Some(i) = clicked {
                self.selected = i;
            }
            if let Some(i) = dbl {
                self.selected = i;
                self.action = Some(Action::Paste {
                    id: clips[matches[i]].id,
                    plain: false,
                });
            }

            // Wheel: three rows per notch, list only.
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                if list.contains(pos) {
                    let delta = ui.input(|i| i.raw_scroll_delta.y);
                    if delta.abs() > 1.0 {
                        let step = if delta > 0.0 { -3isize } else { 3 };
                        let max = layout::max_top_row(matches.len(), self.visible_rows);
                        self.top_row =
                            (self.top_row as isize + step).clamp(0, max as isize) as usize;
                    }
                }
            }

            paint_scroll_hint(
                &painter,
                &pal,
                list,
                self.top_row,
                matches.len(),
                self.visible_rows,
            );
        }

        // Atlas hygiene: a scrolled-away row has no reason to keep a GL
        // texture. Dropping the handle frees it immediately, so the resident
        // set is always "the rows you can see", not "every image you scrolled
        // past".
        self.thumbs.borrow_mut().retain(&onscreen);

        if preview && !matches.is_empty() {
            let div_x = panel.left() + list_w;
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(div_x, body_top), Vec2::new(1.0, body_h)),
                0.0,
                pal.hairline,
            );
            let pane = Rect::from_min_size(
                Pos2::new(div_x + 1.0, body_top),
                Vec2::new(PREVIEW_W, body_h),
            );
            if let Some(action) = self.paint_preview(ui, &painter, &pal, pane, clips, &matches) {
                self.action = Some(action);
            }
        }

        self.paint_footer(&painter, &pal, panel);

        if need_refresh {
            self.refresh();
        }
    }

    fn paint_header(&mut self, ui: &mut Ui, painter: &egui::Painter, pal: &Palette, panel: Rect) {
        let icon_rect = Rect::from_min_size(
            Pos2::new(panel.left() + GUTTER, panel.top() + 23.0),
            Vec2::new(ICON, ICON),
        );
        icons::paint(painter, Icon::Search, icon_rect, pal.text_tertiary);

        let field = Rect::from_min_size(
            Pos2::new(panel.left() + TITLE_X, panel.top() + 20.0),
            Vec2::new(620.0, 24.0),
        );
        let n = self.source.total();
        let placeholder = if n == 1 {
            "Search 1 clip…".to_string()
        } else {
            format!("Search {n} clips…")
        };
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(Id::new("pastebox.search"))
            .frame(false)
            .margin(egui::Margin::ZERO)
            .desired_width(field.width())
            .font(tokens::ui(type_size::SEARCH))
            .text_color(pal.text_primary)
            .hint_text(
                egui::RichText::new(placeholder)
                    .size(type_size::SEARCH)
                    .color(pal.text_tertiary),
            )
            .cursor_at_end(true);
        let resp = ui.put(field, edit);
        if self.focus_search {
            resp.request_focus();
            self.focus_search = false;
        }

        // The Esc keycap closes; it is the only mouse-reachable dismiss control.
        let cap_w = 24.0;
        let cap = Rect::from_min_size(
            Pos2::new(panel.right() - GUTTER - cap_w, panel.top() + 22.0),
            Vec2::new(cap_w, 20.0),
        );
        paint_keycap_body(painter, pal, cap);
        draw_tracked(
            painter,
            Pos2::new(cap.center().x, cap.center().y),
            "Esc",
            tokens::ui(type_size::KBD),
            pal.text_secondary,
            0.0,
            Align2::CENTER_CENTER,
        );
        if ui.interact(cap, Id::new("esc"), Sense::click()).clicked() {
            self.action = Some(Action::Close);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn paint_chips(&mut self, ui: &mut Ui, painter: &egui::Painter, pal: &Palette, panel: Rect) {
        let font = tokens::ui(type_size::CHIP);
        let mut x = panel.left() + GUTTER;
        let y = panel.top() + HEADER_H + 6.0;
        for (i, (filter, label)) in Filter::CHIPS.iter().enumerate() {
            let text_w = painter
                .layout_no_wrap((*label).to_string(), font.clone(), pal.text_primary)
                .size()
                .x;
            let width = text_w + 28.0;
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, CHIP_H));
            let resp = ui.interact(rect, Id::new(("chip", i)), Sense::click());
            let active = self.filter == *filter;
            if active {
                painter.rect_filled(rect, Rounding::same(CHIP_H / 2.0), pal.accent_soft);
            } else if resp.hovered() {
                painter.rect_filled(rect, Rounding::same(CHIP_H / 2.0), pal.row_hover);
            }
            let color = if active {
                pal.accent
            } else {
                pal.text_secondary
            };
            draw_tracked(
                painter,
                rect.center(),
                label,
                font.clone(),
                color,
                0.0,
                Align2::CENTER_CENTER,
            );
            if resp.clicked() {
                self.filter = *filter;
                self.selected = 0;
                self.refresh();
            }
            x += width + CHIP_GAP;
        }
    }

    fn paint_preview(
        &self,
        ui: &mut Ui,
        painter: &egui::Painter,
        pal: &Palette,
        pane: Rect,
        clips: &[ClipItem],
        matches: &[usize],
    ) -> Option<Action> {
        let clip = matches.get(self.selected).map(|&i| &clips[i])?;
        let meta = self.source.meta(clip.id);
        let x = pane.left() + GUTTER;
        let width = pane.width() - GUTTER * 2.0;

        let label = format::preview_label(clip, meta.dims);
        draw_tracked(
            painter,
            Pos2::new(x, pane.top() + GUTTER),
            &label,
            tokens::ui(type_size::PREVIEW_LABEL),
            pal.text_tertiary,
            0.66,
            Align2::LEFT_TOP,
        );

        let body_top = pane.top() + GUTTER + 24.0;
        let buttons_top = pane.bottom() - 52.0;
        let body = Rect::from_min_max(
            Pos2::new(x, body_top),
            Pos2::new(x + width, buttons_top - 16.0),
        );
        // Only the selected row ever gets its full pixels decoded.
        let preview_texture = self.preview_texture(ui.ctx(), clip);
        paint_preview_body(painter, pal, clip, &meta, body, preview_texture);

        // Primary and secondary actions.
        let paste = Rect::from_min_size(Pos2::new(x, buttons_top), Vec2::new(72.0, BUTTON_H));
        let plain = Rect::from_min_size(
            Pos2::new(paste.right() + 12.0, buttons_top),
            Vec2::new(148.0, BUTTON_H),
        );
        let paste_resp = ui.interact(paste, Id::new("paste-btn"), Sense::click());
        let plain_resp = ui.interact(plain, Id::new("plain-btn"), Sense::click());

        painter.rect_filled(
            paste,
            Rounding::same(radius::CHIP),
            if paste_resp.hovered() {
                pal.accent_hover
            } else {
                pal.accent
            },
        );
        draw_tracked(
            painter,
            paste.center(),
            "Paste",
            tokens::ui(type_size::BUTTON),
            pal.on_accent,
            0.0,
            Align2::CENTER_CENTER,
        );

        if plain_resp.hovered() {
            painter.rect_filled(plain, Rounding::same(radius::CHIP), pal.row_hover);
        }
        painter.rect_stroke(
            plain,
            Rounding::same(radius::CHIP),
            Stroke::new(1.0_f32, pal.border_strong),
        );
        draw_tracked(
            painter,
            plain.center(),
            "Paste as plain text",
            tokens::ui(type_size::BUTTON),
            pal.text_primary,
            0.0,
            Align2::CENTER_CENTER,
        );

        if paste_resp.clicked() {
            return Some(Action::Paste {
                id: clip.id,
                plain: false,
            });
        }
        if plain_resp.clicked() {
            return Some(Action::Paste {
                id: clip.id,
                plain: true,
            });
        }
        None
    }

    /// The selected image decoded into a GPU texture, at most one at a time.
    ///
    /// The previous selection's texture is dropped here, so switching rows frees
    /// the old image's pixels instead of accumulating them. Falls back to the
    /// row thumbnail when the payload was dropped for size.
    fn preview_texture(&self, ctx: &egui::Context, clip: &ClipItem) -> Option<egui::TextureId> {
        let image = clip.image.as_ref()?;
        if !image.has_payload() {
            return self.thumbs.borrow_mut().get(ctx, clip);
        }
        let mut slot = self.preview_tex.borrow_mut();
        if let Some((id, handle)) = slot.as_ref() {
            if *id == clip.id {
                return Some(handle.id());
            }
        }
        let (w, h, rgba) = image.decode_rgba()?;
        let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let handle = ctx.load_texture(
            format!("pastebox-preview-{}", clip.id),
            color,
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        *slot = Some((clip.id, handle));
        Some(id)
    }

    fn paint_footer(&self, painter: &egui::Painter, pal: &Palette, panel: Rect) {
        let footer = Rect::from_min_size(
            Pos2::new(panel.left(), panel.bottom() - FOOTER_H),
            Vec2::new(panel.width(), FOOTER_H),
        );
        painter.rect_filled(
            footer,
            Rounding {
                nw: 0.0,
                ne: 0.0,
                sw: radius::PANEL,
                se: radius::PANEL,
            },
            pal.chrome,
        );
        painter.rect_filled(
            Rect::from_min_size(footer.min, Vec2::new(footer.width(), 1.0)),
            0.0,
            pal.hairline,
        );

        let cy = footer.center().y;
        let mut x = footer.left() + GUTTER;
        let hints: [(Cap, &str); 4] = [
            (Cap::Return, "Paste"),
            (Cap::ShiftReturn, "Plain text"),
            (Cap::Text("Ctrl P"), "Pin"),
            (Cap::Text("Del"), "Delete"),
        ];
        for (cap, label) in hints {
            let (w, _) = paint_keycap(painter, pal, Pos2::new(x, cy), cap);
            x += w + 8.0;
            let rect = draw_tracked(
                painter,
                Pos2::new(x, cy),
                label,
                tokens::ui(type_size::ROW_META),
                pal.text_tertiary,
                0.0,
                Align2::LEFT_CENTER,
            );
            x = rect.right() + 20.0;
        }

        let right = footer.right() - GUTTER;
        if let Some(toast) = &self.toast {
            let color = if toast.danger { pal.danger } else { pal.accent };
            draw_tracked(
                painter,
                Pos2::new(right, cy),
                &toast.text,
                tokens::ui(type_size::ROW_META),
                color,
                0.0,
                Align2::RIGHT_CENTER,
            );
        } else {
            let label = format::disk_label(self.source.bytes_on_disk());
            let rect = draw_tracked(
                painter,
                Pos2::new(right, cy),
                &label,
                tokens::ui(type_size::ROW_META),
                pal.text_tertiary,
                0.0,
                Align2::RIGHT_CENTER,
            );
            painter.circle_filled(Pos2::new(rect.left() - 8.0 - 3.0, cy), 3.0, pal.kind_file);
        }
    }
}

/// Row thumbnails resident on the GPU, keyed by clip id.
///
/// The cache holds only what the viewport is showing: [`ThumbCache::retain`] is
/// called once per frame with the visible ids, and any other texture handle is
/// dropped — which frees the GL texture and its CPU-side upload immediately.
#[derive(Default)]
struct ThumbCache {
    textures: HashMap<u64, egui::TextureHandle>,
}

impl ThumbCache {
    /// The texture for `clip`'s thumbnail, uploading it on first sight.
    fn get(&mut self, ctx: &egui::Context, clip: &ClipItem) -> Option<egui::TextureId> {
        let image = clip.image.as_ref()?;
        if let Some(handle) = self.textures.get(&clip.id) {
            return Some(handle.id());
        }
        let thumb = image.thumb();
        if thumb.width() == 0 || thumb.height() == 0 {
            return None;
        }
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [thumb.width() as usize, thumb.height() as usize],
            thumb.rgba(),
        );
        let handle = ctx.load_texture(
            format!("pastebox-thumb-{}", clip.id),
            color,
            egui::TextureOptions::LINEAR,
        );
        let id = handle.id();
        self.textures.insert(clip.id, handle);
        Some(id)
    }

    /// Drop every texture whose row is no longer on screen.
    fn retain(&mut self, onscreen: &HashSet<u64>) {
        self.textures.retain(|id, _| onscreen.contains(id));
    }
}

/// Pixel dimensions of an image row, falling back to the parsed title so the
/// aspect ratio is still sane when the payload was dropped.
fn image_dims(clip: &ClipItem) -> (u32, u32) {
    clip.image
        .as_ref()
        .map(ClipImage::dims)
        .or_else(|| format::image_dims(&clip.preview))
        .filter(|(w, h)| *w > 0 && *h > 0)
        .unwrap_or((1, 1))
}

/// Draw `texture` into `area`, cropping to fill it (thumbnails).
fn paint_image_cover(
    painter: &egui::Painter,
    texture: egui::TextureId,
    area: Rect,
    dims: (u32, u32),
) {
    let (w, h) = (dims.0 as f32, dims.1 as f32);
    let scale = (area.width() / w).max(area.height() / h);
    let size = Vec2::new(w * scale, h * scale);
    let full = Rect::from_center_size(area.center(), size);
    // UV window that maps `area` onto the centre of the image.
    let u0 = ((full.width() - area.width()) / 2.0) / full.width();
    let v0 = ((full.height() - area.height()) / 2.0) / full.height();
    let u1 = u0 + area.width() / full.width();
    let v1 = v0 + area.height() / full.height();
    painter.image(
        texture,
        area,
        Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u1, v1)),
        Color32::WHITE,
    );
}

/// Draw `texture` inside `area`, letterboxed (preview pane).
fn paint_image_fit(
    painter: &egui::Painter,
    texture: egui::TextureId,
    area: Rect,
    dims: (u32, u32),
) {
    let (w, h) = (dims.0 as f32, dims.1 as f32);
    let scale = (area.width() / w).min(area.height() / h);
    let size = Vec2::new(w * scale, h * scale);
    painter.image(
        texture,
        Rect::from_center_size(area.center(), size),
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// One row: plate, accent bar, glyph, title, meta, age.
#[allow(clippy::too_many_arguments)]
fn paint_row(
    painter: &egui::Painter,
    pal: &Palette,
    row: Rect,
    clip: &ClipItem,
    meta: &ClipMeta,
    selected: bool,
    hovered: bool,
    now_ms: u64,
    thumbs: &RefCell<ThumbCache>,
    ctx: &egui::Context,
) {
    if selected {
        let plate = Rect::from_min_max(row.min, Pos2::new(row.right() - 8.0, row.bottom()));
        painter.rect_filled(
            plate,
            Rounding {
                nw: 0.0,
                ne: radius::ROW,
                sw: 0.0,
                se: radius::ROW,
            },
            pal.row_selected,
        );
        painter.rect_filled(
            Rect::from_min_size(row.min, Vec2::new(2.0, row.height())),
            0.0,
            pal.accent,
        );
    } else if hovered {
        let plate = Rect::from_min_max(row.min, Pos2::new(row.right() - 8.0, row.bottom()));
        painter.rect_filled(
            plate,
            Rounding {
                nw: 0.0,
                ne: radius::ROW,
                sw: 0.0,
                se: radius::ROW,
            },
            pal.row_hover,
        );
    }

    // The age label is measured first so the text column can reserve exactly
    // the room it needs; the title and meta line are then clipped to what is
    // left (PRD UI-07: never cut a title, always end it with `…`).
    let meta_font = tokens::ui(type_size::ROW_META);
    let age = format::relative_time(now_ms, clip.created_ms);
    let age_w = painter
        .layout_no_wrap(age.clone(), meta_font.clone(), pal.text_tertiary)
        .size()
        .x;
    let columns = layout::row_columns(row, age_w);

    paint_kind_glyph(painter, pal, clip, columns.icon, thumbs, ctx);

    let title_font = tokens::ui(type_size::ROW_TITLE);
    let title = truncate_to_width(&clip.preview, columns.text.width(), |s| {
        painter
            .layout_no_wrap(s.to_string(), title_font.clone(), pal.text_primary)
            .size()
            .x
    });
    let title_color = if meta.too_large {
        pal.text_tertiary
    } else {
        pal.text_primary
    };
    let weight = if selected { 0.5 } else { 0.2 };
    let clipped = painter.with_clip_rect(columns.text);
    draw_text(
        &clipped,
        Pos2::new(columns.text.left(), row.top() + 9.0),
        &title,
        title_font,
        title_color,
        weight,
    );

    let meta_text = format::meta_line(&meta.source_app, clip, meta.dims);
    let meta_budget = columns.text.width() - if clip.pinned { 20.0 } else { 0.0 };
    let meta_text = truncate_to_width(&meta_text, meta_budget, |s| {
        painter
            .layout_no_wrap(s.to_string(), meta_font.clone(), pal.text_tertiary)
            .size()
            .x
    });
    let meta_rect = draw_text(
        &clipped,
        Pos2::new(columns.text.left(), row.top() + 31.0),
        &meta_text,
        meta_font.clone(),
        pal.text_tertiary,
        0.0,
    );

    if clip.pinned {
        let pin = Rect::from_min_size(
            Pos2::new(meta_rect.right() + 6.0, meta_rect.center().y - 6.0),
            Vec2::new(12.0, 12.0),
        );
        icons::paint(&clipped, Icon::PinFilled, pin, pal.accent);
    }

    draw_tracked(
        painter,
        Pos2::new(columns.age.right(), row.top() + 19.0),
        &age,
        meta_font,
        pal.text_tertiary,
        0.0,
        Align2::RIGHT_TOP,
    );

    if !selected {
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(row.left() + GUTTER, row.bottom() - 1.0),
                Vec2::new(row.width() - GUTTER * 2.0, 1.0),
            ),
            0.0,
            pal.row_divider,
        );
    }
}

fn paint_kind_glyph(
    painter: &egui::Painter,
    pal: &Palette,
    clip: &ClipItem,
    rect: Rect,
    thumbs: &RefCell<ThumbCache>,
    ctx: &egui::Context,
) {
    if clip.kind == ClipKind::Image {
        let thumb = Rect::from_min_size(
            Pos2::new(rect.left() - 7.0, rect.top() - 7.0),
            Vec2::new(32.0, 32.0),
        );
        painter.rect_filled(thumb, Rounding::same(radius::CHIP), pal.thumb);
        // The row shows the picture itself when we have a thumbnail: it is the
        // cheapest way to tell two screenshots apart.
        if let Some(texture) = thumbs.borrow_mut().get(ctx, clip) {
            let inner = thumb.shrink(2.0);
            paint_image_cover(painter, texture, inner, image_dims(clip));
        } else {
            let inner = Rect::from_center_size(thumb.center(), Vec2::splat(18.0));
            icons::paint(painter, Icon::Image, inner, pal.kind_image);
        }
        painter.rect_stroke(
            thumb,
            Rounding::same(radius::CHIP),
            Stroke::new(1.0_f32, pal.hairline),
        );
        return;
    }

    if let Some(hex) = clip.text.as_deref().and_then(parse_hex_color) {
        let swatch = Rect::from_min_size(rect.min, Vec2::new(ICON, ICON));
        painter.rect_filled(swatch, Rounding::same(4.0), hex);
        painter.rect_stroke(
            swatch,
            Rounding::same(4.0),
            Stroke::new(1.0_f32, tokens::rgba(16, 24, 40, 0.12)),
        );
        return;
    }

    let code =
        clip.kind == ClipKind::Text && clip.text.as_deref().is_some_and(format::is_code_like);
    let (icon, tint) = match clip.kind {
        ClipKind::Link => (Icon::Link, pal.kind_link),
        ClipKind::Files => (Icon::File, pal.kind_file),
        ClipKind::Image => (Icon::Image, pal.kind_image),
        ClipKind::Text => {
            if code {
                (Icon::Code, pal.kind_code)
            } else {
                (Icon::Text, pal.kind_text)
            }
        }
    };
    icons::paint(painter, icon, rect, tint);
}

fn paint_preview_body(
    painter: &egui::Painter,
    pal: &Palette,
    clip: &ClipItem,
    meta: &ClipMeta,
    body: Rect,
    preview_texture: Option<egui::TextureId>,
) {
    match clip.kind {
        ClipKind::Image => {
            let plate_h = (body.width() * 0.56).min((body.height() - 32.0).max(0.0));
            let plate = Rect::from_min_size(body.min, Vec2::new(body.width(), plate_h));
            painter.rect_filled(plate, Rounding::same(radius::CHIP), pal.thumb);
            painter.rect_stroke(
                plate,
                Rounding::same(radius::CHIP),
                Stroke::new(1.0_f32, pal.hairline),
            );
            match preview_texture {
                Some(texture) => {
                    paint_image_fit(painter, texture, plate.shrink(8.0), image_dims(clip));
                }
                // No pixels to show: a centred glyph plus the decoded
                // dimensions reads as a deliberate placeholder, not a missing
                // image.
                None => {
                    icons::paint(
                        painter,
                        Icon::Image,
                        Rect::from_center_size(
                            plate.center() - Vec2::new(0.0, 10.0),
                            Vec2::splat(28.0),
                        ),
                        pal.text_tertiary,
                    );
                }
            }
            let caption = match meta
                .dims
                .or_else(|| clip.image.as_ref().map(ClipImage::dims))
            {
                Some((w, h)) => format!("{w} × {h}"),
                None => format::detail(clip, meta.dims),
            };
            draw_tracked(
                painter,
                Pos2::new(plate.center().x, plate.center().y + 14.0),
                &caption,
                tokens::ui(type_size::ROW_META),
                pal.text_tertiary,
                0.0,
                Align2::CENTER_TOP,
            );
        }
        ClipKind::Files => {
            let mut y = body.top();
            for file in clip.files.iter().take(8) {
                let name = file.rsplit(['\\', '/']).next().unwrap_or(file);
                icons::paint(
                    painter,
                    Icon::File,
                    Rect::from_min_size(Pos2::new(body.left(), y + 3.0), Vec2::splat(14.0)),
                    pal.kind_file,
                );
                draw_text(
                    painter,
                    Pos2::new(body.left() + 22.0, y),
                    name,
                    tokens::ui(type_size::PREVIEW_BODY),
                    pal.text_primary,
                    0.0,
                );
                y += 24.0;
            }
        }
        ClipKind::Text | ClipKind::Link => {
            let text = clip.text.as_deref().unwrap_or(clip.preview.as_str());
            let mono = format::is_code_like(text);
            let font = if mono {
                tokens::mono(type_size::PREVIEW_MONO)
            } else {
                tokens::ui(type_size::PREVIEW_BODY)
            };
            let line_height = if mono { 18.0 } else { 21.0 };
            let galley = painter.layout_job(layout_job(
                text,
                font,
                pal.text_primary,
                line_height,
                0.0,
                body.width(),
            ));
            let clipped = painter.with_clip_rect(body);
            let overflow = galley.size().y > body.height();
            clipped.galley(body.min, galley, pal.text_primary);
            if overflow {
                let fade_h = 24.0;
                let fade = Rect::from_min_max(
                    Pos2::new(body.left(), body.bottom() - fade_h),
                    body.right_bottom(),
                );
                fade_bottom(&clipped, fade, pal.panel);
            }
        }
    }
}

/// What sits under the body copy of a state block.
enum StateFooter {
    /// `Ctrl` `C` keycaps.
    Keycaps(&'static [&'static str]),
    /// A single accent-coloured link; the bool is the click result.
    Link(&'static str, &'static str),
    None,
}

/// What an empty state says.
struct StateSpec<'a> {
    icon: Icon,
    title: &'a str,
    body: &'a str,
    footer: StateFooter,
}

/// One centred state composition — icon, headline, body, footer — laid out on
/// the content width and centred both ways inside `area`.
fn paint_state(
    ui: &mut Ui,
    painter: &egui::Painter,
    pal: &Palette,
    area: Rect,
    spec: StateSpec<'_>,
) -> bool {
    let StateSpec {
        icon,
        title,
        body,
        footer,
    } = spec;
    let width = 360.0_f32.min(area.width() - 64.0).max(200.0);
    let title_galley = painter.layout_job(layout_job(
        title,
        tokens::ui(type_size::EMPTY_TITLE),
        pal.text_primary,
        24.0,
        0.0,
        width,
    ));
    let body_galley = painter.layout_job(layout_job(
        body,
        tokens::ui(type_size::EMPTY_BODY),
        pal.text_secondary,
        20.0,
        0.0,
        width,
    ));

    let footer_h = match footer {
        StateFooter::None => 0.0,
        _ => 16.0 + 20.0,
    };
    let height = 56.0 + title_galley.size().y + 8.0 + body_galley.size().y + footer_h;

    let cx = area.center().x;
    let mut y = area.center().y - height / 2.0;

    icons::paint(
        painter,
        icon,
        Rect::from_center_size(Pos2::new(cx, y + 20.0), Vec2::splat(40.0)),
        pal.text_tertiary,
    );
    y += 56.0;

    painter.galley(
        Pos2::new(cx - title_galley.size().x / 2.0, y),
        title_galley.clone(),
        pal.text_primary,
    );
    y += title_galley.size().y + 8.0;

    painter.galley(
        Pos2::new(cx - body_galley.size().x / 2.0, y),
        body_galley.clone(),
        pal.text_secondary,
    );
    y += body_galley.size().y + 16.0;

    match footer {
        StateFooter::None => false,
        StateFooter::Keycaps(labels) => {
            let cap_w = 34.0;
            let gap = 4.0;
            let total = cap_w * labels.len() as f32 + gap * (labels.len().saturating_sub(1)) as f32;
            let mut x = cx - total / 2.0;
            for label in labels {
                let cap = Rect::from_min_size(Pos2::new(x, y), Vec2::new(cap_w, 20.0));
                paint_keycap_body(painter, pal, cap);
                draw_tracked(
                    painter,
                    cap.center(),
                    label,
                    tokens::ui(type_size::KBD),
                    pal.text_secondary,
                    0.0,
                    Align2::CENTER_CENTER,
                );
                x += cap_w + gap;
            }
            false
        }
        StateFooter::Link(label, id) => paint_state_link(ui, painter, pal, cx, y, label, id),
    }
}

fn paint_state_link(
    ui: &mut Ui,
    painter: &egui::Painter,
    pal: &Palette,
    cx: f32,
    y: f32,
    label: &str,
    id: &str,
) -> bool {
    let w = painter
        .layout_no_wrap(
            label.to_string(),
            tokens::ui(type_size::EMPTY_BODY),
            pal.accent,
        )
        .size()
        .x;
    let rect = Rect::from_min_size(Pos2::new(cx - w / 2.0, y), Vec2::new(w, 20.0));
    let resp = ui.interact(rect, Id::new(id), Sense::click());
    if resp.hovered() {
        painter.rect_filled(
            rect.expand2(Vec2::new(6.0, 2.0)),
            Rounding::same(4.0),
            pal.row_hover,
        );
    }
    draw_tracked(
        painter,
        rect.left_center(),
        label,
        tokens::ui(type_size::EMPTY_BODY),
        pal.accent,
        0.0,
        Align2::LEFT_CENTER,
    );
    resp.clicked()
}

/// History is genuinely empty: nothing has ever been copied.
fn paint_empty(ui: &mut Ui, painter: &egui::Painter, pal: &Palette, area: Rect) {
    paint_state(
        ui,
        painter,
        pal,
        area,
        StateSpec {
            icon: Icon::Text,
            title: "Nothing copied yet",
            body: "Copy anything and it appears here. Nothing is saved to disk.",
            footer: StateFooter::Keycaps(&["Ctrl", "C"]),
        },
    );
}

/// The history has clips, but the active kind filter matched none of them.
fn paint_filter_empty(
    ui: &mut Ui,
    painter: &egui::Painter,
    pal: &Palette,
    area: Rect,
    filter: Filter,
    total: usize,
) -> bool {
    let icon = match filter {
        Filter::Images => Icon::Image,
        Filter::Files => Icon::File,
        Filter::Links => Icon::Link,
        Filter::Pinned => Icon::Pin,
        _ => Icon::Text,
    };
    let title = filter.empty_title().to_string();
    let body = filter.empty_body(total);
    let footer = if filter == Filter::All {
        StateFooter::None
    } else {
        StateFooter::Link("Show all clips", "show-all-clips")
    };
    paint_state(
        ui,
        painter,
        pal,
        area,
        StateSpec {
            icon,
            title: &title,
            body: &body,
            footer,
        },
    )
}

/// A query with no hits, optionally narrowed by a kind filter.
fn paint_no_results(
    ui: &mut Ui,
    painter: &egui::Painter,
    pal: &Palette,
    area: Rect,
    query: &str,
    filter: Filter,
) -> bool {
    let title = if filter == Filter::All {
        format!("No clips match “{query}”")
    } else {
        format!("No {} match “{query}”", filter.noun())
    };
    let body = "Titles, URLs and file names are searched. Images match by caption.";
    paint_state(
        ui,
        painter,
        pal,
        area,
        StateSpec {
            icon: Icon::Search,
            title: &title,
            body,
            footer: StateFooter::Link("Clear search", "clear-search"),
        },
    )
}

/// A hairline scroll indicator that only appears while the pointer is in the
/// list, so the hero frame stays free of scrollbars.
fn paint_scroll_hint(
    painter: &egui::Painter,
    pal: &Palette,
    list: Rect,
    top_row: usize,
    count: usize,
    visible: usize,
) {
    if count <= visible {
        return;
    }
    let hovered = painter
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|p| list.contains(p));
    if !hovered {
        return;
    }
    let track_h = list.height() - 16.0;
    let thumb_h = (track_h * visible as f32 / count as f32).max(32.0);
    let max_top = (count - visible) as f32;
    let t = if max_top > 0.0 {
        top_row as f32 / max_top
    } else {
        0.0
    };
    let y = list.top() + 8.0 + (track_h - thumb_h) * t;
    painter.rect_filled(
        Rect::from_min_size(Pos2::new(list.right() - 6.0, y), Vec2::new(3.0, thumb_h)),
        Rounding::same(2.0),
        pal.border_strong,
    );
}

fn paint_shadow(painter: &egui::Painter, panel: Rect, pal: &Palette) {
    let base = pal.shadow_rgb;
    // 26 stacked rounded rects approximate the 64 px blur without an offscreen
    // pass; alpha accumulates to the spec's 0.24 at the panel edge.
    for i in 0..26 {
        let t = i as f32 / 25.0;
        let spread = 40.0 * (1.0 - t) * (1.0 - t);
        let alpha = 3.0 * (1.0 - 0.4 * t);
        let color = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha as u8);
        let rect = panel
            .expand(spread)
            .translate(Vec2::new(0.0, 12.0 * (1.0 - t)));
        painter.rect_filled(rect, Rounding::same(radius::PANEL + spread), color);
    }
    for i in 0..6 {
        let spread = 8.0 * (1.0 - i as f32 / 6.0);
        let color = Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 2);
        painter.rect_filled(
            panel.expand(spread).translate(Vec2::new(0.0, 2.0)),
            Rounding::same(radius::PANEL + spread),
            color,
        );
    }
}

/// A text layout job with an explicit line height, so the preview can be set at
/// 13/21 like a document instead of the font's default leading.
fn layout_job(
    text: &str,
    font: FontId,
    color: Color32,
    line_height: f32,
    tracking: f32,
    wrap: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: tracking,
            line_height: Some(line_height),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap;
    job
}

/// Single-line text, optionally thickened by a sub-pixel double draw (egui has
/// no weight axis, and the selected title must read heavier).
fn draw_text(
    painter: &egui::Painter,
    pos: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    weight: f32,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_string(), font, color);
    let rect = Rect::from_min_size(pos, galley.size());
    painter.galley(pos, galley.clone(), color);
    if weight > 0.0 {
        painter.galley(pos + Vec2::new(weight, 0.0), galley, color);
    }
    rect
}

/// Text with manual letter tracking — the uppercase eyebrow and the keycaps.
fn draw_tracked(
    painter: &egui::Painter,
    anchor: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    tracking: f32,
    align: Align2,
) -> Rect {
    if tracking.abs() < 0.01 {
        let galley = painter.layout_no_wrap(text.to_string(), font, color);
        let rect = align.anchor_size(anchor, galley.size());
        painter.galley(rect.min, galley, color);
        return rect;
    }
    let mut widths = Vec::with_capacity(text.chars().count());
    let mut total = 0.0;
    let mut height: f32 = 0.0;
    for ch in text.chars() {
        let g = painter.layout_no_wrap(ch.to_string(), font.clone(), color);
        let w = g.size().x;
        height = height.max(g.size().y);
        widths.push((ch, w));
        total += w + tracking;
    }
    let total = (total - tracking).max(0.0);
    let rect = align.anchor_size(anchor, Vec2::new(total, height));
    let mut x = rect.left();
    for (ch, w) in widths {
        painter.text(
            Pos2::new(x, rect.top()),
            Align2::LEFT_TOP,
            ch,
            font.clone(),
            color,
        );
        x += w + tracking;
    }
    rect
}

fn fade_bottom(painter: &egui::Painter, rect: Rect, color: Color32) {
    let clear = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), clear);
    mesh.colored_vertex(rect.right_top(), clear);
    mesh.colored_vertex(rect.right_bottom(), color);
    mesh.colored_vertex(rect.left_bottom(), color);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

#[derive(Clone, Copy)]
enum Cap {
    Text(&'static str),
    Return,
    ShiftReturn,
}

fn paint_keycap_body(painter: &egui::Painter, pal: &Palette, rect: Rect) {
    painter.rect_filled(rect, Rounding::same(radius::KEYCAP), pal.keycap_fill);
    painter.rect_stroke(
        rect,
        Rounding::same(radius::KEYCAP),
        Stroke::new(1.0_f32, pal.border_strong),
    );
}

/// Draw a keycap with its content; returns its width and the rect.
fn paint_keycap(painter: &egui::Painter, pal: &Palette, at: Pos2, cap: Cap) -> (f32, Rect) {
    let font = tokens::ui(type_size::KBD);
    let (width, height) = match cap {
        Cap::Text(text) => {
            let w = painter
                .layout_no_wrap(text.to_string(), font.clone(), pal.text_secondary)
                .size()
                .x;
            (w + 12.0, KEYCAP_H)
        }
        Cap::Return => (22.0, KEYCAP_H),
        Cap::ShiftReturn => (34.0, KEYCAP_H),
    };
    let rect = Rect::from_min_size(
        Pos2::new(at.x, at.y - height / 2.0),
        Vec2::new(width, height),
    );
    paint_keycap_body(painter, pal, rect);
    match cap {
        Cap::Text(text) => {
            draw_tracked(
                painter,
                rect.center(),
                text,
                font,
                pal.text_secondary,
                0.0,
                Align2::CENTER_CENTER,
            );
        }
        Cap::Return => {
            icons::paint(
                painter,
                Icon::Return,
                Rect::from_center_size(rect.center(), Vec2::splat(12.0)),
                pal.text_secondary,
            );
        }
        Cap::ShiftReturn => {
            icons::paint(
                painter,
                Icon::Shift,
                Rect::from_center_size(rect.center() - Vec2::new(6.0, 0.0), Vec2::splat(12.0)),
                pal.text_secondary,
            );
            icons::paint(
                painter,
                Icon::Return,
                Rect::from_center_size(rect.center() + Vec2::new(6.0, 0.0), Vec2::splat(12.0)),
                pal.text_secondary,
            );
        }
    }
    (width, rect)
}

/// `#4F46E5` → a swatch colour, for colour clips (still `Text` in core).
fn parse_hex_color(text: &str) -> Option<Color32> {
    let t = text.trim();
    let hex = t.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_swatches_parse() {
        assert_eq!(
            parse_hex_color("#4F46E5"),
            Some(Color32::from_rgb(0x4F, 0x46, 0xE5))
        );
        assert_eq!(parse_hex_color("4F46E5"), None);
        assert_eq!(parse_hex_color("#4F46E"), None);
        assert_eq!(parse_hex_color("#GGGGGG"), None);
    }

    #[test]
    fn filters_accept_only_their_kind() {
        assert!(Filter::All.accepts(ClipKind::Image, false));
        assert!(Filter::Images.accepts(ClipKind::Image, false));
        assert!(!Filter::Images.accepts(ClipKind::Text, false));
        assert!(Filter::Pinned.accepts(ClipKind::Text, true));
        assert!(!Filter::Pinned.accepts(ClipKind::Text, false));
    }

    #[test]
    fn chip_order_matches_the_keyboard() {
        assert_eq!(Filter::CHIPS[0].1, "All");
        assert_eq!(Filter::from_index(2), Filter::Images);
        assert_eq!(Filter::from_index(0), Filter::All);
    }

    /// Feed one key press through a real `egui::Context`. `frame` is the
    /// frame-level modifier state the *backend* reports, which is what the
    /// real Wine session fails to populate; the event carries its own.
    fn press_frame(
        app: &mut PasteboxApp,
        key: egui::Key,
        frame: egui::Modifiers,
        event: egui::Modifiers,
    ) {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            time: Some(1.0),
            modifiers: frame,
            events: vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: true,
                repeat: false,
                modifiers: event,
            }],
            ..Default::default()
        };
        let _ = ctx.run(raw, |c| app.handle_keys(c));
    }

    fn press(app: &mut PasteboxApp, key: egui::Key, mods: egui::Modifiers) {
        press_frame(app, key, mods, mods);
    }

    fn first_row_id(app: &PasteboxApp) -> u64 {
        app.selected_id()
            .expect("demo fixture has a selectable row")
    }

    /// Wine's winit backend sends the key with `ctrl: true` but never updates
    /// the frame-level modifier state, so `InputState::modifiers` is all-false.
    /// Captured from a live run; this is the release-blocking shape.
    #[test]
    fn ctrl_p_pins_when_the_backend_reports_no_frame_modifiers() {
        let mut app = PasteboxApp::bare(Box::new(crate::DemoSource::hero()), 0);
        app.visible_rows = 6;
        let id = first_row_id(&app);
        let pinned_now = app.selected_clip().is_some_and(|c| c.pinned);
        press_frame(
            &mut app,
            egui::Key::P,
            egui::Modifiers::NONE,
            egui::Modifiers::CTRL,
        );
        match app.take_action() {
            Some(Action::Pin { id: got, pinned }) => {
                assert_eq!(got, id, "pin must target the selected row");
                assert_eq!(pinned, !pinned_now, "pin must toggle");
            }
            other => panic!("Ctrl+P produced {other:?}, expected Action::Pin"),
        }
    }

    #[test]
    fn ctrl_p_pins_the_selected_row() {
        let mut app = PasteboxApp::bare(Box::new(crate::DemoSource::hero()), 0);
        app.visible_rows = 6;
        let id = first_row_id(&app);
        let pinned_now = app.selected_clip().is_some_and(|c| c.pinned);
        press(&mut app, egui::Key::P, egui::Modifiers::CTRL);
        match app.take_action() {
            Some(Action::Pin { id: got, pinned }) => {
                assert_eq!(got, id, "pin must target the selected row");
                assert_eq!(pinned, !pinned_now, "pin must toggle");
            }
            other => panic!("Ctrl+P produced {other:?}, expected Action::Pin"),
        }
    }

    #[test]
    fn ctrl_p_works_with_a_search_field_focused() {
        // The search box holds keyboard focus for the palette's whole life, so
        // the shortcut must survive the text edit's own event handling.
        let mut app = PasteboxApp::bare(Box::new(crate::DemoSource::hero()), 0);
        app.visible_rows = 6;
        app.focus_search = true;
        let id = first_row_id(&app);
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            time: Some(1.0),
            modifiers: egui::Modifiers::CTRL,
            events: vec![egui::Event::Key {
                key: egui::Key::P,
                physical_key: Some(egui::Key::P),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::CTRL,
            }],
            ..Default::default()
        };
        // Run a full frame, so the header's TextEdit also consumes events.
        let _ = ctx.run(raw, |c| {
            app.handle_keys(c);
            let mut q = app.query.clone();
            let edit = egui::TextEdit::singleline(&mut q).id(Id::new("pastebox.search"));
            egui::CentralPanel::default().show(c, |ui| {
                ui.put(
                    Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 24.0)),
                    edit,
                );
            });
        });
        assert!(
            matches!(app.take_action(), Some(Action::Pin { id: got, .. }) if got == id),
            "Ctrl+P was swallowed by the focused text edit"
        );
    }

    #[test]
    fn ctrl_number_selects_a_chip() {
        let mut app = PasteboxApp::bare(Box::new(crate::DemoSource::hero()), 0);
        app.visible_rows = 6;
        press_frame(
            &mut app,
            egui::Key::Num3,
            egui::Modifiers::NONE,
            egui::Modifiers::CTRL,
        );
        assert_eq!(app.filter, Filter::Images);
    }

    #[test]
    fn a_filter_with_no_hits_never_claims_the_history_is_empty() {
        assert_eq!(Filter::Images.empty_title(), "No image clips");
        assert_eq!(Filter::Files.empty_title(), "No file clips");
        assert_eq!(Filter::Links.empty_title(), "No link clips");
        assert_eq!(Filter::Pinned.empty_title(), "Nothing pinned");
        for filter in [Filter::Text, Filter::Images, Filter::Files, Filter::Links] {
            let body = filter.empty_body(3);
            assert!(body.contains("3 clips"), "{}: {body}", filter.label());
            assert!(
                !body.contains("Nothing copied yet"),
                "{}: {body}",
                filter.label()
            );
        }
        assert_eq!(
            Filter::Images.empty_body(1),
            "Your only clip isn't an image. Copy a picture and it appears here."
        );
        assert!(Filter::Pinned.empty_body(3).contains("Ctrl P"));
    }

    #[test]
    fn an_empty_history_and_a_filtered_empty_read_differently() {
        let mut app = PasteboxApp::bare(Box::new(crate::DemoSource::hero()), 0);
        assert!(!app.source.clips().is_empty(), "fixture has clips");
        // Nothing is pinned in the fixture, so Pinned is a filter miss on a
        // non-empty history — the case that used to say "Nothing copied yet".
        app.filter = Filter::Pinned;
        app.refresh();
        assert!(app.matches.is_empty());
        assert_eq!(app.filter.empty_title(), "Nothing pinned");
    }
}
