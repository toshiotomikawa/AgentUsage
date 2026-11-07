//! The dashboard: one card per account, one row per quota window. Everything is painted by hand
//! so spacing, motion and color stay exact.

use crate::anim::{Tweens, ease_in_out, ease_out_cubic};
use crate::icons;
use crate::model::{Account, Reading, Status, Window};
use crate::theme::{Palette, SEMIBOLD};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Sense, Shape,
    Stroke, StrokeKind, Ui, Vec2, epaint::PathStroke, pos2, vec2,
};
use std::collections::HashMap;
use std::f32::consts::TAU;

pub struct Frame<'a> {
    pub accounts: &'a [Account],
    pub busy: bool,
    pub last_refresh: Option<DateTime<Utc>>,
    pub refresh_every: u64,
    pub now: DateTime<Utc>,
}

pub struct Dashboard {
    tweens: Tweens,
    start: Option<f64>,
    spin: f32,
}

/// Layout metrics, scaled by density so everything fits without scrolling.
#[derive(Clone, Copy)]
struct Metrics {
    pad: f32,
    head: f32,
    row: f32,
    group: f32,
    gap: f32,
}

impl Metrics {
    fn at(k: f32) -> Self {
        Self {
            pad: 20.0 * k.max(0.8),
            head: 30.0,
            row: 50.0 * k,
            group: 26.0 * k,
            gap: 16.0 * k.max(0.75),
        }
    }
}

const BAR_H: f32 = 6.0;

impl Dashboard {
    pub fn new() -> Self {
        Self {
            tweens: Tweens::default(),
            start: None,
            spin: 0.0,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, pal: &Palette, f: Frame<'_>) -> bool {
        let now = ui.input(|i| i.time);
        let start = *self.start.get_or_insert(now);
        let since = (now - start) as f32;
        self.tweens.active = false;
        let mut refresh = false;

        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, pal.bg);

        let outer = (full.width() * 0.045).clamp(16.0, 56.0);
        let content_w = (full.width() - outer * 2.0).min(1280.0);
        let left = full.center().x - content_w / 2.0;
        let top = full.top() + outer * 0.8;

        // ---------- header ----------
        let head_alpha = ease_out_cubic(since / 0.5);
        let p = ui.painter().clone();
        let title_pos = pos2(left, top + (1.0 - head_alpha) * 6.0);
        p.text(
            title_pos,
            Align2::LEFT_TOP,
            "Cotas",
            FontId::new(24.0, FontFamily::Name(SEMIBOLD.into())),
            pal.text.gamma_multiply(head_alpha),
        );
        let (summary, summary_color) = summary(f.accounts, f.now, pal);
        let dot_c = pos2(left + 4.0, top + 46.0);
        p.circle_filled(dot_c, 3.5, summary_color.gamma_multiply(head_alpha));
        p.text(
            pos2(left + 14.0, top + 46.0),
            Align2::LEFT_CENTER,
            summary,
            FontId::proportional(13.5),
            pal.muted.gamma_multiply(head_alpha),
        );

        // Refresh button.
        let btn =
            Rect::from_center_size(pos2(left + content_w - 18.0, top + 20.0), vec2(36.0, 36.0));
        let resp = ui.interact(btn, ui.scope_id().with("refresh"), Sense::click());
        let hover =
            ui.ctx()
                .animate_bool_with_time(ui.scope_id().with("refresh-h"), resp.hovered(), 0.15);
        p.circle_filled(btn.center(), 18.0, pal.card.gamma_multiply(head_alpha));
        p.circle_stroke(
            btn.center(),
            17.5,
            Stroke::new(
                1.0,
                lerp_color(pal.border, pal.border_hover, hover).gamma_multiply(head_alpha),
            ),
        );
        let dt = ui.input(|i| i.stable_dt).min(0.05);
        if f.busy {
            self.spin += dt * TAU * 1.1;
            self.tweens.active = true;
        } else {
            // Settle to the nearest full turn.
            let target = (self.spin / TAU).ceil() * TAU;
            if (target - self.spin).abs() > 0.01 {
                self.spin += ((target - self.spin) * 10.0 * dt)
                    .max(0.02)
                    .min(target - self.spin);
                self.tweens.active = true;
            } else {
                self.spin = 0.0;
            }
        }
        refresh_icon(
            &p,
            btn.center(),
            7.0,
            self.spin,
            lerp_color(pal.muted, pal.text, hover).gamma_multiply(head_alpha),
        );
        if resp.clicked() {
            refresh = true;
        }
        resp.on_hover_ui(|ui| {
            ui.label(egui::RichText::new("Atualizar agora").strong());
            if let Some(at) = f.last_refresh {
                let ago = (f.now - at).num_seconds().max(0);
                let next = (f.refresh_every as i64 - ago).max(0);
                ui.label(format!(
                    "Atualizado há {} · próxima em {}",
                    short_secs(ago),
                    short_secs(next)
                ));
            }
            ui.label(egui::RichText::new("F5").weak());
        });

        // ---------- grid ----------
        let grid_top = top + 78.0;
        let avail_h = full.bottom() - grid_top - outer * 0.6;
        let cols = if content_w >= 980.0 {
            3
        } else if content_w >= 620.0 {
            2
        } else {
            1
        };

        let mut cards: Vec<&Account> = f.accounts.iter().collect();
        cards.sort_by(|a, b| {
            a.title()
                .to_lowercase()
                .cmp(&b.title().to_lowercase())
                .then(a.account.cmp(&b.account))
                .then(a.key.cmp(&b.key))
        });
        let mut per_provider: HashMap<_, usize> = HashMap::new();
        for a in &cards {
            *per_provider.entry(a.provider).or_default() += 1;
        }

        // Pick the largest density that fits.
        let mut k = 1.0;
        let mut m = Metrics::at(k);
        let mut placed;
        let mut total_h;
        loop {
            (placed, total_h) = masonry(&cards, cols, &m);
            if total_h <= avail_h || k <= 0.8 {
                break;
            }
            k -= 0.05;
            m = Metrics::at(k);
        }
        let card_w = (content_w - m.gap * (cols - 1) as f32) / cols as f32;

        let grid_rect = Rect::from_min_max(pos2(full.left(), grid_top), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid_rect));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                let (area, _) = ui
                    .allocate_exact_size(vec2(full.width(), total_h + outer * 0.6), Sense::hover());
                for (idx, (acc, &(col, y, h))) in cards.iter().zip(placed.iter()).enumerate() {
                    let rect = Rect::from_min_size(
                        pos2(left + col as f32 * (card_w + m.gap), area.top() + y),
                        vec2(card_w, h),
                    );
                    let show_account = per_provider.get(&acc.provider).copied().unwrap_or(0) > 1;
                    self.card(ui, pal, &f, acc, rect, &m, idx, since, now, show_account);
                }
            });

        if self.tweens.active || f.busy {
            ui.ctx().request_repaint();
        } else {
            // Countdowns tick once a second.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1000));
        }
        refresh
    }

    #[allow(clippy::too_many_arguments)]
    fn card(
        &mut self,
        ui: &mut Ui,
        pal: &Palette,
        f: &Frame<'_>,
        acc: &Account,
        rect: Rect,
        m: &Metrics,
        idx: usize,
        since: f32,
        now: f64,
        show_account: bool,
    ) {
        let appear = ease_out_cubic((since - 0.08 - idx as f32 * 0.06) / 0.55);
        if appear < 1.0 {
            self.tweens.active = true;
        }
        if appear <= 0.0 {
            return;
        }
        let rect = rect.translate(vec2(0.0, (1.0 - appear) * 14.0));
        let resp = ui.interact(rect, ui.scope_id().with(("card", &acc.key)), Sense::hover());
        let hover = ui.ctx().animate_bool_with_time(
            ui.scope_id().with(("card-h", &acc.key)),
            resp.hovered(),
            0.18,
        );
        let unavailable = matches!(acc.reading, Reading::Unavailable(_));
        let a = appear * if unavailable { 0.72 } else { 1.0 };
        let p = ui.painter_at(rect.expand(8.0));

        // Soft shadow on hover.
        if hover > 0.0 && !pal.dark {
            p.rect_filled(
                rect.translate(vec2(0.0, 2.0)).expand(1.0),
                CornerRadius::same(15),
                Color32::from_black_alpha((10.0 * hover * a) as u8),
            );
        }
        p.rect_filled(
            rect,
            CornerRadius::same(14),
            lerp_color(pal.card, pal.card_hover, hover).gamma_multiply(appear),
        );
        p.rect_stroke(
            rect,
            CornerRadius::same(14),
            Stroke::new(
                1.0,
                lerp_color(pal.border, pal.border_hover, hover).gamma_multiply(appear),
            ),
            StrokeKind::Inside,
        );

        // Header: icon, title, optional account, status ring.
        let inner = rect.shrink(m.pad);
        let hy = inner.top() + m.head / 2.0;
        let icon_c = pos2(inner.left() + 11.0, hy);
        let icon_color = if unavailable {
            pal.faint
        } else {
            icons::brand(acc.provider, pal.text)
        };
        icons::paint(&p, acc.provider, icon_c, 20.0, icon_color.gamma_multiply(a));
        let title_font = FontId::new(15.5, FontFamily::Name(SEMIBOLD.into()));
        let title = p.layout_no_wrap(
            acc.title().to_string(),
            title_font,
            pal.text.gamma_multiply(a),
        );
        let title_w = title.size().x;
        p.galley(
            pos2(inner.left() + 32.0, hy - title.size().y / 2.0),
            title,
            pal.text,
        );
        if show_account && let Some(name) = &acc.account {
            p.text(
                pos2(inner.left() + 32.0 + title_w + 8.0, hy + 0.5),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(13.0),
                pal.muted.gamma_multiply(a),
            );
        }

        let body_top = inner.top() + m.head + m.gap * 0.6;
        match &acc.reading {
            Reading::Loading => {
                // Shimmering placeholders.
                let phase = ((now as f32 * 1.4) % 1.0).fract();
                self.tweens.active = true;
                for i in 0..2 {
                    let y = body_top + i as f32 * m.row;
                    let line = Rect::from_min_size(
                        pos2(inner.left(), y + 2.0),
                        vec2(inner.width() * 0.3, 10.0),
                    );
                    p.rect_filled(line, 5.0, pal.track.gamma_multiply(a));
                    let bar = Rect::from_min_size(
                        pos2(inner.left(), y + 24.0),
                        vec2(inner.width(), BAR_H),
                    );
                    p.rect_filled(bar, 3.0, pal.track.gamma_multiply(a));
                    let sx = bar.left() + bar.width() * (phase * 1.4 - 0.2);
                    let glow = Rect::from_min_max(
                        pos2(sx, bar.top()),
                        pos2(sx + bar.width() * 0.2, bar.bottom()),
                    )
                    .intersect(bar);
                    if glow.width() > 0.0 {
                        p.rect_filled(glow, 3.0, pal.faint.gamma_multiply(0.35 * a));
                    }
                }
            }
            Reading::Unavailable(reason) => {
                p.text(
                    pos2(inner.left(), body_top + 4.0),
                    Align2::LEFT_TOP,
                    "Sem dados",
                    FontId::proportional(14.0),
                    pal.muted.gamma_multiply(a),
                );
                p.text(
                    pos2(inner.left(), body_top + 24.0),
                    Align2::LEFT_TOP,
                    capitalize(reason),
                    FontId::proportional(12.5),
                    pal.faint.gamma_multiply(appear),
                );
            }
            Reading::Ok(groups) => {
                // Status ring: the tightest window.
                if let Some(b) = acc.binding() {
                    let st = b.status(f.now);
                    let v = self.tweens.get(
                        &format!("ring:{}", acc.key),
                        b.remaining,
                        0.0,
                        now,
                        0.9,
                        0.15 + idx as f32 * 0.06,
                    );
                    let rc = pos2(inner.right() - 9.0, hy);
                    let ring_track = if st == Status::Empty {
                        lerp_color(pal.track, pal.crit, 0.35)
                    } else {
                        pal.track
                    };
                    p.circle_stroke(rc, 8.0, Stroke::new(2.4, ring_track.gamma_multiply(a)));
                    arc(
                        &p,
                        rc,
                        8.0,
                        v,
                        Stroke::new(2.4, pal.status(st).gamma_multiply(a)),
                    );
                    let ring_rect = Rect::from_center_size(rc, vec2(24.0, 24.0));
                    ui.interact(
                        ring_rect,
                        ui.scope_id().with(("ring", &acc.key)),
                        Sense::hover(),
                    )
                    .on_hover_text(format!(
                        "Mais apertada: {} · {}%",
                        b.label,
                        pct(b.remaining)
                    ));
                }
                let mut y = body_top;
                let many = groups.len() > 1;
                let mut row_i = 0;
                for (gi, g) in groups.iter().enumerate() {
                    if many {
                        p.text(
                            pos2(inner.left(), y + m.group * 0.45),
                            Align2::LEFT_CENTER,
                            g.name.to_uppercase(),
                            FontId::new(11.0, FontFamily::Name(SEMIBOLD.into())),
                            pal.muted.gamma_multiply(a * 0.75),
                        );
                        y += m.group;
                    }
                    for (wi, w) in g.windows.iter().enumerate() {
                        let row =
                            Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), m.row));
                        let delay = 0.2 + idx as f32 * 0.06 + row_i as f32 * 0.05;
                        self.row(ui, pal, f.now, now, &p, &acc.key, gi, wi, w, row, a, delay);
                        y += m.row;
                        row_i += 1;
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        ui: &mut Ui,
        pal: &Palette,
        now_utc: DateTime<Utc>,
        now: f64,
        p: &Painter,
        key: &str,
        gi: usize,
        wi: usize,
        w: &Window,
        row: Rect,
        a: f32,
        delay: f32,
    ) {
        let st = w.status(now_utc);
        let color = pal.status(st);
        let id = format!("bar:{key}:{gi}:{wi}");
        let v = self.tweens.get(&id, w.remaining, 0.0, now, 0.9, delay);
        let resp = ui.interact(row, ui.scope_id().with(&id), Sense::hover());
        let hover =
            ui.ctx()
                .animate_bool_with_time(ui.scope_id().with((&id, "h")), resp.hovered(), 0.15);

        let line_y = row.top() + 9.0;
        p.text(
            pos2(row.left(), line_y),
            Align2::LEFT_CENTER,
            &w.label,
            FontId::proportional(13.0),
            lerp_color(pal.muted, pal.text, hover * 0.6).gamma_multiply(a),
        );
        // Right side: countdown (muted), then the percentage.
        let pct_text = format!("{}%", pct(v));
        let pct_color = if st == Status::Ok { pal.text } else { color };
        let pct_g = p.layout_no_wrap(
            pct_text,
            FontId::new(14.0, FontFamily::Name(SEMIBOLD.into())),
            pct_color.gamma_multiply(a),
        );
        let pct_w = pct_g.size().x;
        p.galley(
            pos2(row.right() - pct_w, line_y - pct_g.size().y / 2.0),
            pct_g,
            pal.text,
        );
        if let Some(at) = w.resets_at {
            let left = (at - now_utc).num_seconds();
            p.text(
                pos2(row.right() - pct_w - 10.0, line_y + 0.5),
                Align2::RIGHT_CENTER,
                countdown(left),
                FontId::proportional(12.0),
                pal.muted.gamma_multiply(a * 0.8),
            );
        }

        // Bar.
        let bar = Rect::from_min_size(pos2(row.left(), row.top() + 22.0), vec2(row.width(), BAR_H));
        let track = if st == Status::Empty {
            lerp_color(pal.track, pal.crit, 0.18)
        } else {
            pal.track
        };
        p.rect_filled(bar, 3.0, track.gamma_multiply(a));
        if v > 0.0005 {
            let fill = Rect::from_min_size(bar.min, vec2((bar.width() * v).max(BAR_H), BAR_H));
            p.rect_filled(fill, 3.0, color.gamma_multiply(a));
        }
        // Pace marker: where the fill would be at an even pace.
        if let Some(t) = w.time_left_fraction(now_utc) {
            let tv = self
                .tweens
                .get(&format!("{id}:pace"), t, t, now, 0.6, delay + 0.4);
            let appear = ease_in_out(
                ((now as f32) - (self.start.unwrap_or(0.0) as f32) - delay - 0.5) / 0.4,
            );
            if appear < 1.0 {
                self.tweens.active = true;
            }
            let x = bar.left() + bar.width() * tv;
            let h = 6.0 + 2.0 * hover;
            p.line_segment(
                [pos2(x, bar.center().y - h), pos2(x, bar.center().y + h)],
                Stroke::new(1.6, pal.pace.gamma_multiply(0.55 * a * appear)),
            );
        }

        resp.on_hover_ui(|ui| {
            ui.set_max_width(260.0);
            ui.label(
                egui::RichText::new(format!("{} · {}% restante", w.label, pct(w.remaining)))
                    .strong(),
            );
            if let Some(at) = w.resets_at {
                ui.label(format!("Renova {}", local_when(at)));
            }
            if let Some(line) = pace_text(w, now_utc) {
                ui.label(egui::RichText::new(line).weak());
            }
        });
    }
}

/// Place cards (already in reading order) into the shortest column.
/// Returns (column, y, height) per card and the total height.
fn masonry(cards: &[&Account], cols: usize, m: &Metrics) -> (Vec<(usize, f32, f32)>, f32) {
    let mut bottoms = vec![0.0f32; cols];
    let mut out: Vec<(usize, f32, f32)> = Vec::with_capacity(cards.len());
    for a in cards {
        let h = card_height(a, m);
        let col = (0..cols)
            .min_by(|&x, &y| bottoms[x].total_cmp(&bottoms[y]).then(x.cmp(&y)))
            .unwrap_or(0);
        let y = if bottoms[col] > 0.0 {
            bottoms[col] + m.gap
        } else {
            0.0
        };
        out.push((col, y, h));
        bottoms[col] = y + h;
    }
    let total = bottoms.iter().copied().fold(0.0, f32::max);
    (out, total)
}

fn card_height(a: &Account, m: &Metrics) -> f32 {
    let body = match &a.reading {
        Reading::Loading => 2.0 * m.row,
        Reading::Unavailable(_) => 44.0,
        Reading::Ok(groups) => {
            let rows: usize = groups.iter().map(|g| g.windows.len()).sum();
            let heads = if groups.len() > 1 {
                groups.len() as f32 * m.group
            } else {
                0.0
            };
            rows as f32 * m.row + heads
        }
    };
    m.pad * 2.0 + m.head + m.gap * 0.6 + body - 10.0
}

fn summary(accounts: &[Account], now: DateTime<Utc>, pal: &Palette) -> (String, Color32) {
    let mut warn = 0;
    let mut crit = 0;
    let mut ok = 0;
    for a in accounts {
        for w in a.windows() {
            match w.status(now) {
                Status::Ok => ok += 1,
                Status::Warning => warn += 1,
                Status::Critical | Status::Empty => crit += 1,
            }
        }
    }
    if ok + warn + crit == 0 {
        return ("Lendo as cotas…".into(), pal.faint);
    }
    match (crit, warn) {
        (0, 0) => ("Tudo dentro do ritmo".into(), pal.ok),
        (0, w) => (
            format!("{w} {} atenção", plural(w, "pede", "pedem")),
            pal.warn,
        ),
        (c, 0) => (
            format!("{c} {} no limite", plural(c, "cota", "cotas")),
            pal.crit,
        ),
        (c, w) => (
            format!("{c} no limite · {w} {} atenção", plural(w, "pede", "pedem")),
            pal.crit,
        ),
    }
}

fn plural<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

fn pct(v: f32) -> i32 {
    (v * 100.0).round() as i32
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn countdown(secs: i64) -> String {
    if secs <= 0 {
        return "agora".into();
    }
    let m = secs / 60;
    let h = m / 60;
    let d = h / 24;
    if d >= 1 {
        format!("{d}d {}h", h % 24)
    } else if h >= 1 {
        format!("{h}h {}m", m % 60)
    } else if m >= 1 {
        format!("{m} min")
    } else {
        format!("{secs} s")
    }
}

fn short_secs(s: i64) -> String {
    if s < 60 {
        format!("{s} s")
    } else {
        format!("{} min", s / 60)
    }
}

fn local_when(at: DateTime<Utc>) -> String {
    use chrono::{Datelike, Timelike};
    let l = at.with_timezone(&Local);
    let today = Local::now().date_naive();
    let day = if l.date_naive() == today {
        "hoje".to_string()
    } else if l.date_naive() == today.succ_opt().unwrap_or(today) {
        "amanhã".to_string()
    } else {
        const WD: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
        const MO: [&str; 12] = [
            "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez",
        ];
        format!(
            "{}, {} {}",
            WD[l.weekday().num_days_from_monday() as usize],
            l.day(),
            MO[l.month0() as usize]
        )
    };
    format!("{day} às {:02}:{:02}", l.hour(), l.minute())
}

/// Plain-language pace: will this window run out before it resets?
fn pace_text(w: &Window, now: DateTime<Utc>) -> Option<String> {
    let t_left = w.time_left_fraction(now)?;
    let len = w.length?.as_secs_f32();
    let elapsed = (1.0 - t_left) * len;
    let used = 1.0 - w.remaining;
    if w.remaining <= 0.005 {
        return Some("Esgotada até renovar".into());
    }
    if elapsed < len * 0.02 || used < 0.01 {
        return Some("Ainda sem consumo relevante nesta janela".into());
    }
    let rate = used / elapsed; // fraction per second
    let to_empty = w.remaining / rate;
    let until_reset = t_left * len;
    if to_empty < until_reset {
        Some(format!(
            "Neste ritmo, acaba em {} — {} antes de renovar",
            countdown(to_empty as i64),
            countdown((until_reset - to_empty) as i64)
        ))
    } else {
        let leftover = (w.remaining - rate * until_reset).max(0.0);
        Some(format!(
            "Neste ritmo, sobram ~{}% na renovação",
            pct(leftover)
        ))
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        l(a.r(), b.r()),
        l(a.g(), b.g()),
        l(a.b(), b.b()),
        l(a.a(), b.a()),
    )
}

fn arc(p: &Painter, c: Pos2, r: f32, frac: f32, stroke: Stroke) {
    if frac <= 0.001 {
        return;
    }
    let n = (48.0 * frac).ceil().max(2.0) as usize;
    let start = -TAU / 4.0;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = start + TAU * frac * i as f32 / n as f32;
            c + Vec2::angled(a) * r
        })
        .collect();
    p.add(Shape::line(
        pts,
        PathStroke::new(stroke.width, stroke.color),
    ));
}

fn refresh_icon(p: &Painter, c: Pos2, r: f32, rot: f32, color: Color32) {
    let start = rot - TAU * 0.05;
    let sweep = TAU * 0.78;
    let n = 32;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| c + Vec2::angled(start + sweep * i as f32 / n as f32) * r)
        .collect();
    let end = *pts.last().unwrap();
    p.add(Shape::line(pts, PathStroke::new(1.6, color)));
    // Arrowhead tangent to the arc's end.
    let a = start + sweep;
    let tangent = Vec2::angled(a + TAU / 4.0);
    let radial = Vec2::angled(a);
    let tip = end + tangent * 2.2;
    p.add(Shape::convex_polygon(
        vec![
            tip,
            end - tangent * 2.2 + radial * 3.0,
            end - tangent * 2.2 - radial * 3.0,
        ],
        color,
        Stroke::NONE,
    ));
}
