//! Window chrome drawn by the app: title bar (drag, double-click to maximize, window buttons),
//! resize edges and a hairline border. The native frame is turned off in `main`.

use crate::i18n::Lang;
use crate::icons::Icons;
use crate::theme::{Palette, SEMIBOLD};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontFamily, FontId, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, ViewportCommand, pos2, vec2, viewport::ResizeDirection,
};

pub const BAR_H: f32 = 38.0;
const BTN_W: f32 = 46.0;
const EDGE: f32 = 5.0;

fn maximized(ui: &Ui) -> bool {
    ui.input(|i| i.viewport().maximized.unwrap_or(false))
}

/// Paint the title bar at the top of `full` and return the area below it.
pub fn title_bar(ui: &mut Ui, pal: &Palette, icons: &mut Icons, lang: Lang, full: Rect) -> Rect {
    let bar = Rect::from_min_size(full.min, vec2(full.width(), BAR_H));
    let p = ui.painter().clone();
    let max = maximized(ui);

    // Drag area: everything left of the window buttons.
    let drag = Rect::from_min_max(bar.min, pos2(bar.right() - BTN_W * 3.0, bar.bottom()));
    let resp = ui.interact(
        drag,
        ui.scope_id().with("titlebar"),
        Sense::click_and_drag(),
    );
    if resp.double_clicked() {
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
    } else if resp.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }

    let focused = ui.input(|i| i.viewport().focused.unwrap_or(true));
    let ink = if focused { pal.text } else { pal.muted };
    let icon = Rect::from_center_size(pos2(bar.left() + 20.0, bar.center().y), vec2(16.0, 16.0));
    icons.paint_logo(ui, icon, ink);
    p.text(
        pos2(icon.right() + 8.0, bar.center().y),
        Align2::LEFT_CENTER,
        "AgentUsage",
        FontId::new(12.5, FontFamily::Name(SEMIBOLD.into())),
        ink.gamma_multiply(0.85),
    );

    // Window buttons, right to left: close, maximize/restore, minimize.
    let mut x = bar.right();
    for kind in [Btn::Close, Btn::Max, Btn::Min] {
        let r = Rect::from_min_max(pos2(x - BTN_W, bar.top()), pos2(x, bar.bottom()));
        x -= BTN_W;
        let resp = ui.interact(
            r,
            ui.scope_id().with(("winbtn", kind as u8)),
            Sense::click(),
        );
        let h = ui.ctx().animate_bool_with_time(
            ui.scope_id().with(("winbtn-h", kind as u8)),
            resp.hovered(),
            0.12,
        );
        let (fill, glyph) = match kind {
            Btn::Close => (pal.close_hover, lerp(ink, Color32::WHITE, h)),
            _ => (pal.track, ink),
        };
        if h > 0.0 {
            p.rect_filled(r, CornerRadius::ZERO, fill.gamma_multiply(h));
        }
        let c = r.center();
        let s = Stroke::new(1.0, glyph);
        match kind {
            Btn::Close => {
                let d = 5.0;
                p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], s);
                p.line_segment([c + vec2(-d, d), c + vec2(d, -d)], s);
            }
            Btn::Max if max => {
                let back = Rect::from_center_size(c + vec2(1.5, -1.5), vec2(8.0, 8.0));
                let front = Rect::from_center_size(c + vec2(-1.0, 1.0), vec2(8.0, 8.0));
                p.line_segment([back.left_top() + vec2(0.0, 0.0), back.right_top()], s);
                p.line_segment([back.right_top(), back.right_bottom()], s);
                p.rect_stroke(front, 1.0, s, StrokeKind::Middle);
            }
            Btn::Max => {
                p.rect_stroke(
                    Rect::from_center_size(c, vec2(10.0, 10.0)),
                    1.0,
                    s,
                    StrokeKind::Middle,
                );
            }
            Btn::Min => {
                p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], s);
            }
        }
        let tip = match kind {
            Btn::Close => lang.t("Fechar", "Close"),
            Btn::Max if max => lang.t("Restaurar", "Restore"),
            Btn::Max => lang.t("Maximizar", "Maximize"),
            Btn::Min => lang.t("Minimizar", "Minimize"),
        };
        let clicked = resp.clicked();
        resp.on_hover_text(tip);
        if clicked {
            let cmd = match kind {
                Btn::Close => ViewportCommand::Close,
                Btn::Max => ViewportCommand::Maximized(!max),
                Btn::Min => ViewportCommand::Minimized(true),
            };
            ui.ctx().send_viewport_cmd(cmd);
        }
    }

    Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max)
}

#[derive(Clone, Copy)]
enum Btn {
    Close,
    Max,
    Min,
}

/// Hairline border and invisible resize edges. Call last so the edges sit on top.
pub fn frame_edges(ui: &mut Ui, pal: &Palette, full: Rect) {
    if maximized(ui) {
        return;
    }
    ui.painter().rect_stroke(
        full,
        CornerRadius::ZERO,
        Stroke::new(1.0, pal.border_hover),
        StrokeKind::Inside,
    );
    let (l, r, t, b) = (full.left(), full.right(), full.top(), full.bottom());
    let c = EDGE * 2.0;
    let zones: [(Rect, ResizeDirection, CursorIcon); 8] = [
        (
            rect(l, t, l + c, t + c),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNwSe,
        ),
        (
            rect(r - c, t, r, t + c),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNeSw,
        ),
        (
            rect(l, b - c, l + c, b),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeNeSw,
        ),
        (
            rect(r - c, b - c, r, b),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeNwSe,
        ),
        (
            rect(l + c, t, r - c, t + EDGE * 0.6),
            ResizeDirection::North,
            CursorIcon::ResizeVertical,
        ),
        (
            rect(l + c, b - EDGE, r - c, b),
            ResizeDirection::South,
            CursorIcon::ResizeVertical,
        ),
        (
            rect(l, t + c, l + EDGE, b - c),
            ResizeDirection::West,
            CursorIcon::ResizeHorizontal,
        ),
        (
            rect(r - EDGE, t + c, r, b - c),
            ResizeDirection::East,
            CursorIcon::ResizeHorizontal,
        ),
    ];
    for (i, (zone, dir, cursor)) in zones.into_iter().enumerate() {
        let resp = ui.interact(zone, ui.scope_id().with(("resize", i)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(cursor);
        }
        if resp.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

fn rect(l: f32, t: f32, r: f32, b: f32) -> Rect {
    Rect::from_min_max(Pos2::new(l, t), Pos2::new(r, b))
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}
