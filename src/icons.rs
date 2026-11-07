//! Small vector marks for each provider, drawn with the painter so they stay crisp at any scale.

use crate::model::Provider;
use eframe::egui::{Color32, Painter, Pos2, Shape, Stroke, Vec2, epaint::PathStroke, pos2, vec2};
use std::f32::consts::{PI, TAU};

pub fn brand(provider: Provider, ink: Color32) -> Color32 {
    match provider {
        Provider::Claude => Color32::from_rgb(0xD9, 0x77, 0x57),
        Provider::Antigravity => Color32::from_rgb(0x4C, 0x8B, 0xF5),
        _ => ink,
    }
}

/// Paint the mark centered at `c`, fitting a square of side `s`.
pub fn paint(p: &Painter, provider: Provider, c: Pos2, s: f32, color: Color32) {
    let r = s * 0.5;
    let w = (s * 0.09).max(1.4);
    match provider {
        Provider::Claude => {
            // Twelve-ray burst with alternating lengths.
            for i in 0..12 {
                let a = i as f32 / 12.0 * TAU + 0.13;
                let len = if i % 2 == 0 { r * 0.95 } else { r * 0.68 };
                let d = Vec2::angled(a);
                p.line_segment(
                    [c + d * r * 0.18, c + d * len],
                    Stroke::new(w * 1.15, color),
                );
            }
        }
        Provider::Codex => {
            // Three interlocking rounded loops, a nod to a knot.
            for i in 0..3 {
                let a = i as f32 / 3.0 * TAU - PI / 2.0;
                let off = Vec2::angled(a) * r * 0.30;
                p.circle_stroke(c + off, r * 0.52, Stroke::new(w, color));
            }
        }
        Provider::Cursor => {
            // Isometric cube.
            let pts: Vec<Pos2> = (0..6)
                .map(|i| {
                    let a = i as f32 / 6.0 * TAU - PI / 2.0;
                    c + Vec2::angled(a) * r * 0.92
                })
                .collect();
            p.add(Shape::closed_line(pts.clone(), Stroke::new(w, color)));
            p.line_segment([c, pts[0]], Stroke::new(w, color));
            p.line_segment([c, pts[2]], Stroke::new(w, color));
            p.line_segment([c, pts[4]], Stroke::new(w, color));
            // Shade the top face lightly.
            let top = vec![pts[0], pts[1], c, pts[5]];
            p.add(Shape::convex_polygon(
                top,
                color.gamma_multiply(0.22),
                Stroke::NONE,
            ));
        }
        Provider::OpenCode => {
            // Outer square with an inner block: a terminal frame.
            let outer = eframe::egui::Rect::from_center_size(c, vec2(s * 0.80, s * 0.92));
            p.rect_stroke(
                outer,
                s * 0.08,
                Stroke::new(w, color),
                eframe::egui::StrokeKind::Middle,
            );
            let inner = eframe::egui::Rect::from_min_max(
                pos2(outer.left() + s * 0.2, outer.center().y - s * 0.02),
                pos2(outer.right() - s * 0.2, outer.bottom() - s * 0.2),
            );
            p.rect_filled(inner, s * 0.03, color.gamma_multiply(0.85));
        }
        Provider::Antigravity => {
            // A soaring arch: two strokes meeting at the apex.
            let apex = c + vec2(0.0, -r * 0.85);
            let left = c + vec2(-r * 0.9, r * 0.85);
            let right = c + vec2(r * 0.9, r * 0.85);
            let mid_l = c + vec2(-r * 0.32, r * 0.05);
            let mid_r = c + vec2(r * 0.32, r * 0.05);
            let stroke = PathStroke::new(w * 1.5, color);
            p.add(Shape::line(vec![left, mid_l, apex, mid_r, right], stroke));
        }
    }
}
