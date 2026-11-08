//! The demo backdrop: a blurred document under a scrim, so the panel reads as
//! one object floating over the desktop you were just working in
//! (`screenshot-spec.md` §1, layers 0–2).
//!
//! The real app paints nothing here — its window is transparent and the desktop
//! supplies this context. Only the demo composition draws it.

use egui::{Color32, Painter, Pos2, Rect, Shape, Vec2};

use crate::tokens::Palette;

/// Document window geometry, absolute in canvas coordinates.
const DOC: Rect = Rect {
    min: Pos2 { x: 220.0, y: 120.0 },
    max: Pos2 {
        x: 1060.0,
        y: 680.0,
    },
};
const TITLE_STRIP: f32 = 36.0;

/// Paint layers 0–2 over the whole canvas.
pub fn paint(painter: &Painter, canvas: Rect, pal: &Palette) {
    vertical_gradient(painter, canvas, pal.backdrop_top, pal.backdrop_bottom);
    paint_document(painter, pal);
    painter.rect_filled(canvas, 0.0, pal.scrim);
}

fn vertical_gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// The document you were working in: a title strip with three dots and three
/// columns of ragged grey bars, softened so it is legible only as texture.
fn paint_document(painter: &Painter, pal: &Palette) {
    let p = painter.with_clip_rect(DOC);
    p.rect_filled(DOC, 10.0, pal.doc_fill);

    let strip = Rect::from_min_size(DOC.min, Vec2::new(DOC.width(), TITLE_STRIP));
    p.rect_filled(
        strip,
        egui::Rounding {
            nw: 10.0,
            ne: 10.0,
            sw: 0.0,
            se: 0.0,
        },
        pal.doc_chrome,
    );
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(DOC.left(), DOC.top() + TITLE_STRIP - 1.0),
            Pos2::new(DOC.right(), DOC.top() + TITLE_STRIP),
        ),
        0.0,
        pal.doc_bar,
    );
    for i in 0..3 {
        let x = DOC.left() + 24.0 + i as f32 * 18.0;
        p.circle_filled(Pos2::new(x, DOC.top() + 18.0), 4.0, pal.doc_dot);
    }

    const WIDTHS: [f32; 4] = [320.0, 288.0, 300.0, 240.0];
    for col in 0..3 {
        let x = DOC.left() + 40.0 + col as f32 * 296.0;
        for row in 0..14 {
            let w = WIDTHS[(row + col) % WIDTHS.len()];
            let y = DOC.top() + 66.0 + row as f32 * 26.0;
            let bar = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, 8.0));
            soft_bar(&p, bar, pal.doc_bar);
        }
    }
}

/// A bar drawn three times with growing softness: a stand-in for the σ = 12 px
/// blur in the spec that costs three shapes instead of an offscreen pass.
fn soft_bar(painter: &Painter, rect: Rect, color: Color32) {
    let faint = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90);
    let softer = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 170);
    painter.rect_filled(rect.expand(3.0), 4.0, faint);
    painter.rect_filled(rect.expand(1.5), 4.0, softer);
    painter.rect_filled(rect, 4.0, color);
}
