//! The icon set: thirteen 24 × 24 outlines drawn with the painter, so the
//! product carries no image assets and no emoji (PRD §6.7 / UI-12).
//!
//! Every glyph is authored on a 24 grid with a 1.5 px stroke and is rendered at
//! exactly 18 px in rows and 16 px in chrome; the stroke width scales with the
//! box so a 16 px gear is not heavier than an 18 px one.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};

/// Which glyph to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Search,
    Text,
    Image,
    File,
    Link,
    Code,
    Pin,
    PinFilled,
    Trash,
    Gear,
    Close,
    Warning,
    Return,
    Shift,
}

/// Draw `icon` inside `rect` (a square), stroked in `color`.
pub fn paint(painter: &Painter, icon: Icon, rect: Rect, color: Color32) {
    let s = rect.width() / 24.0;
    let stroke = Stroke::new((1.55 * s).max(0.9), color);
    let p = |x: f32, y: f32| -> Pos2 { Pos2::new(rect.left() + x * s, rect.top() + y * s) };
    let line = |pts: &[(f32, f32)]| {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(Shape::line(points, stroke));
    };
    let closed = |pts: &[(f32, f32)]| {
        let points: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.add(Shape::closed_line(points, stroke));
    };
    let dot = |x: f32, y: f32, r: f32| {
        painter.circle_filled(p(x, y), r * s, color);
    };

    match icon {
        Icon::Search => {
            painter.circle_stroke(p(10.5, 10.5), 6.4 * s, stroke);
            line(&[(15.2, 15.2), (20.0, 20.0)]);
        }
        Icon::Text => {
            line(&[(4.0, 7.0), (20.0, 7.0)]);
            line(&[(4.0, 12.0), (20.0, 12.0)]);
            line(&[(4.0, 17.0), (13.5, 17.0)]);
        }
        Icon::Image => {
            painter.rect_stroke(
                Rect::from_min_max(p(3.5, 4.5), p(20.5, 19.5)),
                3.0 * s,
                stroke,
            );
            painter.circle_stroke(p(9.0, 9.6), 1.7 * s, stroke);
            line(&[
                (4.6, 17.6),
                (9.6, 12.6),
                (13.0, 16.0),
                (16.2, 13.0),
                (20.0, 16.8),
            ]);
        }
        Icon::File => {
            closed(&[
                (6.0, 3.5),
                (14.0, 3.5),
                (19.0, 8.5),
                (19.0, 20.5),
                (6.0, 20.5),
            ]);
            line(&[(14.0, 3.5), (14.0, 8.5), (19.0, 8.5)]);
        }
        Icon::Link => {
            // Two chain links, set on the diagonal.
            let angle = -std::f32::consts::FRAC_PI_4;
            capsule(painter, p(9.2, 14.8), 5.6 * s, 3.1 * s, angle, stroke);
            capsule(painter, p(14.8, 9.2), 5.6 * s, 3.1 * s, angle, stroke);
        }
        Icon::Code => {
            line(&[(9.8, 6.5), (4.6, 12.0), (9.8, 17.5)]);
            line(&[(14.2, 6.5), (19.4, 12.0), (14.2, 17.5)]);
        }
        Icon::Pin | Icon::PinFilled => {
            let head = [
                (9.2, 4.0),
                (14.8, 4.0),
                (14.8, 8.2),
                (17.4, 11.2),
                (17.4, 13.2),
                (6.6, 13.2),
                (6.6, 11.2),
                (9.2, 8.2),
            ];
            if icon == Icon::PinFilled {
                let points: Vec<Pos2> = head.iter().map(|&(x, y)| p(x, y)).collect();
                painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
            } else {
                closed(&head);
            }
            line(&[(12.0, 13.2), (12.0, 20.0)]);
        }
        Icon::Trash => {
            line(&[(3.6, 7.0), (20.4, 7.0)]);
            line(&[(9.0, 7.0), (9.0, 3.8), (15.0, 3.8), (15.0, 7.0)]);
            line(&[(6.2, 7.0), (7.4, 20.2), (16.6, 20.2), (17.8, 7.0)]);
        }
        Icon::Gear => {
            painter.circle_stroke(p(12.0, 12.0), 3.2 * s, stroke);
            for k in 0..8 {
                let a = std::f32::consts::TAU * (k as f32) / 8.0;
                let (sin, cos) = a.sin_cos();
                line(&[
                    (12.0 + cos * 6.3, 12.0 + sin * 6.3),
                    (12.0 + cos * 9.2, 12.0 + sin * 9.2),
                ]);
            }
        }
        Icon::Close => {
            line(&[(6.4, 6.4), (17.6, 17.6)]);
            line(&[(17.6, 6.4), (6.4, 17.6)]);
        }
        Icon::Warning => {
            closed(&[(12.0, 3.6), (21.0, 20.0), (3.0, 20.0)]);
            line(&[(12.0, 9.4), (12.0, 14.4)]);
            dot(12.0, 17.2, 0.95);
        }
        Icon::Return => {
            line(&[(18.0, 6.5), (18.0, 13.0), (7.0, 13.0)]);
            line(&[(10.6, 9.4), (7.0, 13.0), (10.6, 16.6)]);
        }
        Icon::Shift => {
            line(&[(12.0, 18.5), (12.0, 6.5)]);
            line(&[(6.4, 11.8), (12.0, 6.2), (17.6, 11.8)]);
        }
    }
}

/// A rotated rounded rectangle, outlined — one half of the link glyph.
fn capsule(
    painter: &Painter,
    center: Pos2,
    half_len: f32,
    half_w: f32,
    angle: f32,
    stroke: Stroke,
) {
    let (sin, cos) = angle.sin_cos();
    let r = half_w;
    let straight = (half_len - half_w).max(0.0);
    let mut points = Vec::with_capacity(26);
    // Two straight edges plus two half-circles, walked clockwise.
    let arc = |from: f32, to: f32, cx: f32| {
        let mut out = Vec::new();
        let steps = 8;
        for i in 0..=steps {
            let t = from + (to - from) * (i as f32 / steps as f32);
            out.push((cx + r * t.cos(), r * t.sin()));
        }
        out
    };
    points.extend(arc(
        -std::f32::consts::FRAC_PI_2,
        std::f32::consts::FRAC_PI_2,
        straight,
    ));
    points.extend(arc(
        std::f32::consts::FRAC_PI_2,
        3.0 * std::f32::consts::FRAC_PI_2,
        -straight,
    ));
    let world: Vec<Pos2> = points
        .into_iter()
        .map(|(x, y)| center + Vec2::new(x * cos - y * sin, x * sin + y * cos))
        .collect();
    painter.add(Shape::closed_line(world, stroke));
}
