//! The dashboard: one card per account, one row per quota window. Everything is painted by hand
//! so spacing, motion and color stay exact.

use crate::anim::{Tweens, ease_in_out, ease_out_cubic};
use crate::i18n::Lang;
use crate::icons::{self, Icons};
use crate::model::{Account, Reading, Status, Window};
use crate::prefs::ThemeMode;
use crate::theme::{Palette, SEMIBOLD};
use chrono::{DateTime, Local, Utc};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2, epaint::PathStroke, pos2, vec2,
};
use std::collections::HashMap;
use std::f32::consts::TAU;

pub struct Frame<'a> {
    pub accounts: &'a [Account],
    pub busy: bool,
    pub last_refresh: Option<DateTime<Utc>>,
    pub refresh_every: u64,
    pub now: DateTime<Utc>,
    pub lang: Lang,
    pub theme: ThemeMode,
    pub icons: &'a Icons,
}

/// What the user asked for this frame.
#[derive(Default)]
pub struct Actions {
    pub refresh: bool,
    pub cycle_theme: bool,
    pub toggle_lang: bool,
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
const BTN_R: f32 = 18.0;

impl Dashboard {
    pub fn new() -> Self {
        Self {
            tweens: Tweens::default(),
            start: None,
            spin: 0.0,
        }
    }

    pub fn show(&mut self, ui: &mut Ui, pal: &Palette, f: Frame<'_>) -> Actions {
        let now = ui.input(|i| i.time);
        let start = *self.start.get_or_insert(now);
        let since = (now - start) as f32;
        self.tweens.active = false;
        let mut act = Actions::default();
        let lang = f.lang;

        let full = ui.max_rect();
        let outer = (full.width() * 0.045).clamp(16.0, 56.0);
        let content_w = (full.width() - outer * 2.0).min(1280.0);
        let left = full.center().x - content_w / 2.0;
        let top = full.top() + outer * 0.45;

        // ---------- header ----------
        let head_alpha = ease_out_cubic(since / 0.5);
        let p = ui.painter().clone();
        p.text(
            pos2(left, top + (1.0 - head_alpha) * 6.0),
            Align2::LEFT_TOP,
            lang.t("Cotas", "Quotas"),
            FontId::new(24.0, FontFamily::Name(SEMIBOLD.into())),
            pal.text.gamma_multiply(head_alpha),
        );
        let (summary, summary_color) = summary(f.accounts, f.now, pal, lang);
        let dot_c = pos2(left + 4.0, top + 46.0);
        p.circle_filled(dot_c, 3.5, summary_color.gamma_multiply(head_alpha));
        p.text(
            pos2(left + 14.0, top + 46.0),
            Align2::LEFT_CENTER,
            summary,
            FontId::proportional(13.5),
            pal.muted.gamma_multiply(head_alpha),
        );

        // Header buttons, right to left: refresh, theme, language.
        let cy = top + 20.0;
        let step = BTN_R * 2.0 + 10.0;
        let refresh_c = pos2(left + content_w - BTN_R, cy);
        let theme_c = refresh_c - vec2(step, 0.0);
        let lang_c = theme_c - vec2(step, 0.0);

        let (resp, hover) = self.round_button(ui, &p, pal, refresh_c, "refresh", head_alpha);
        let glyph = lerp_color(pal.muted, pal.text, hover).gamma_multiply(head_alpha);
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
        refresh_icon(&p, refresh_c, 7.0, self.spin, glyph);
        act.refresh = resp.clicked();
        resp.on_hover_ui(|ui| {
            ui.label(egui::RichText::new(lang.t("Atualizar agora", "Refresh now")).strong());
            if let Some(at) = f.last_refresh {
                let ago = (f.now - at).num_seconds().max(0);
                let next = (f.refresh_every as i64 - ago).max(0);
                ui.label(match lang {
                    Lang::Pt => format!(
                        "Atualizado há {} · próxima em {}",
                        short_secs(ago),
                        short_secs(next)
                    ),
                    Lang::En => format!(
                        "Updated {} ago · next in {}",
                        short_secs(ago),
                        short_secs(next)
                    ),
                });
            }
            ui.label(egui::RichText::new("F5").weak());
        });

        let (resp, hover) = self.round_button(ui, &p, pal, theme_c, "theme", head_alpha);
        let glyph = lerp_color(pal.muted, pal.text, hover).gamma_multiply(head_alpha);
        theme_icon(&p, theme_c, f.theme, glyph, pal.card);
        act.cycle_theme = resp.clicked();
        resp.on_hover_ui(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{}: {}",
                    lang.t("Tema", "Theme"),
                    f.theme.name(lang)
                ))
                .strong(),
            );
            ui.label(format!(
                "{} {}",
                lang.t("Clique para", "Click for"),
                f.theme.next().name(lang).to_lowercase()
            ));
        });

        let (resp, hover) = self.round_button(ui, &p, pal, lang_c, "lang", head_alpha);
        let glyph = lerp_color(pal.muted, pal.text, hover).gamma_multiply(head_alpha);
        p.text(
            lang_c + vec2(0.0, 0.5),
            Align2::CENTER_CENTER,
            lang.code(),
            FontId::new(11.5, FontFamily::Name(SEMIBOLD.into())),
            glyph,
        );
        act.toggle_lang = resp.clicked();
        resp.on_hover_ui(|ui| {
            ui.label(
                egui::RichText::new(lang.t("Idioma: Português (BR)", "Language: English (US)"))
                    .strong(),
            );
            ui.label(lang.t("Clique para English (US)", "Click for Português (BR)"));
        });

        // ---------- grid ----------
        let grid_top = top + 78.0;
        let avail_h = full.bottom() - grid_top - outer * 0.6;

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
        let max_cols = if content_w >= 980.0 {
            3
        } else if content_w >= 620.0 {
            2
        } else {
            1
        };
        let cols = max_cols.min(cards.len().max(1));

        // Pick the largest density that fits.
        let mut k = 1.0;
        let mut m = Metrics::at(k);
        let mut rows;
        let mut total_h;
        loop {
            (rows, total_h) = grid_rows(&cards, cols, &m);
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
                let mut idx = 0;
                for (r, &(y, h)) in rows.iter().enumerate() {
                    let row_cards = &cards[r * cols..((r + 1) * cols).min(cards.len())];
                    // Center an incomplete last row.
                    let shift = (cols - row_cards.len()) as f32 * (card_w + m.gap) / 2.0;
                    for (c, acc) in row_cards.iter().enumerate() {
                        let rect = Rect::from_min_size(
                            pos2(left + shift + c as f32 * (card_w + m.gap), area.top() + y),
                            vec2(card_w, h),
                        );
                        let show_account =
                            per_provider.get(&acc.provider).copied().unwrap_or(0) > 1;
                        self.card(ui, pal, &f, acc, rect, &m, idx, since, now, show_account);
                        idx += 1;
                    }
                }
            });

        if self.tweens.active || f.busy {
            ui.ctx().request_repaint();
        } else {
            // Countdowns tick once a second.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1000));
        }
        act
    }

    /// A round header button; returns its response and hover amount.
    fn round_button(
        &mut self,
        ui: &Ui,
        p: &Painter,
        pal: &Palette,
        c: Pos2,
        id: &str,
        alpha: f32,
    ) -> (Response, f32) {
        let rect = Rect::from_center_size(c, vec2(BTN_R * 2.0, BTN_R * 2.0));
        let resp = ui.interact(rect, ui.scope_id().with(("btn", id)), Sense::click());
        let hover = ui.ctx().animate_bool_with_time(
            ui.scope_id().with(("btn-h", id)),
            resp.hovered(),
            0.15,
        );
        let press = if resp.is_pointer_button_down_on() {
            0.6
        } else {
            0.0
        };
        p.circle_filled(c, BTN_R, pal.card.gamma_multiply(alpha));
        p.circle_stroke(
            c,
            BTN_R - 0.5,
            Stroke::new(
                1.0,
                lerp_color(pal.border, pal.border_hover, (hover + press).min(1.0))
                    .gamma_multiply(alpha),
            ),
        );
        (resp, hover)
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
        let lang = f.lang;
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
        let icon = Rect::from_min_size(pos2(inner.left(), hy - 11.0), vec2(22.0, 22.0));
        icons::paint(
            ui,
            f.icons.provider(acc.provider),
            icon,
            6.0,
            if unavailable { a * 0.5 } else { a },
        );
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
                    lang.t("Sem dados", "No data"),
                    FontId::proportional(14.0),
                    pal.muted.gamma_multiply(a),
                );
                p.text(
                    pos2(inner.left(), body_top + 24.0),
                    Align2::LEFT_TOP,
                    capitalize(&lang.data(reason)),
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
                        lerp_color(pal.track, pal.empty, 0.45)
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
                        "{}: {} · {}%",
                        lang.t("Mais apertada", "Tightest"),
                        lang.data(&b.label),
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
                        let ctx = RowCtx {
                            pal,
                            lang,
                            now_utc: f.now,
                            now,
                            painter: &p,
                            alpha: a,
                            delay,
                        };
                        self.row(ui, &ctx, &format!("bar:{}:{gi}:{wi}", acc.key), w, row);
                        y += m.row;
                        row_i += 1;
                    }
                }
            }
        }
    }

    fn row(&mut self, ui: &mut Ui, c: &RowCtx<'_>, id: &str, w: &Window, row: Rect) {
        let (pal, p, a, lang) = (c.pal, c.painter, c.alpha, c.lang);
        let st = w.status(c.now_utc);
        let color = pal.status(st);
        let v = self.tweens.get(id, w.remaining, 0.0, c.now, 0.9, c.delay);
        let resp = ui.interact(row, ui.scope_id().with(id), Sense::hover());
        let hover =
            ui.ctx()
                .animate_bool_with_time(ui.scope_id().with((id, "h")), resp.hovered(), 0.15);
        let label = lang.data(&w.label);

        let line_y = row.top() + 9.0;
        p.text(
            pos2(row.left(), line_y),
            Align2::LEFT_CENTER,
            &label,
            FontId::proportional(13.0),
            lerp_color(pal.muted, pal.text, hover * 0.6).gamma_multiply(a),
        );
        // Right side: countdown (muted), then the percentage.
        let pct_color = if st == Status::Ok { pal.text } else { color };
        let pct_g = p.layout_no_wrap(
            format!("{}%", pct(v)),
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
            let left = (at - c.now_utc).num_seconds();
            p.text(
                pos2(row.right() - pct_w - 10.0, line_y + 0.5),
                Align2::RIGHT_CENTER,
                countdown(left, lang),
                FontId::proportional(12.0),
                pal.muted.gamma_multiply(a * 0.8),
            );
        }

        // Bar.
        let bar = Rect::from_min_size(pos2(row.left(), row.top() + 22.0), vec2(row.width(), BAR_H));
        let track = if st == Status::Empty {
            lerp_color(pal.track, pal.empty, 0.3)
        } else {
            pal.track
        };
        p.rect_filled(bar, 3.0, track.gamma_multiply(a));
        if v > 0.0005 {
            let fill = Rect::from_min_size(bar.min, vec2((bar.width() * v).max(BAR_H), BAR_H));
            p.rect_filled(fill, 3.0, color.gamma_multiply(a));
        }
        // Pace marker: where the fill would be at an even pace.
        if let Some(t) = w.time_left_fraction(c.now_utc) {
            let tv = self
                .tweens
                .get(&format!("{id}:pace"), t, t, c.now, 0.6, c.delay + 0.4);
            let appear = ease_in_out(
                ((c.now as f32) - (self.start.unwrap_or(0.0) as f32) - c.delay - 0.5) / 0.4,
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
            ui.set_max_width(280.0);
            ui.label(
                egui::RichText::new(format!(
                    "{label} · {}% {}",
                    pct(w.remaining),
                    lang.t("restante", "left")
                ))
                .strong(),
            );
            if let Some(at) = w.resets_at {
                ui.label(format!(
                    "{} {}",
                    lang.t("Renova", "Resets"),
                    local_when(at, lang)
                ));
            }
            if let Some(line) = pace_text(w, c.now_utc, lang) {
                ui.label(egui::RichText::new(line).weak());
            }
        });
    }
}

struct RowCtx<'a> {
    pal: &'a Palette,
    lang: Lang,
    now_utc: DateTime<Utc>,
    now: f64,
    painter: &'a Painter,
    alpha: f32,
    delay: f32,
}

/// Row-major grid: every card in a row shares the row's height.
/// Returns (y, height) per row and the total height.
fn grid_rows(cards: &[&Account], cols: usize, m: &Metrics) -> (Vec<(f32, f32)>, f32) {
    let mut out = Vec::new();
    let mut y = 0.0;
    for chunk in cards.chunks(cols) {
        let h = chunk.iter().map(|a| card_height(a, m)).fold(0.0, f32::max);
        out.push((y, h));
        y += h + m.gap;
    }
    let total = (y - m.gap).max(0.0);
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

fn summary(
    accounts: &[Account],
    now: DateTime<Utc>,
    pal: &Palette,
    lang: Lang,
) -> (String, Color32) {
    let mut warn = 0;
    let mut empty = 0;
    let mut ok = 0;
    for a in accounts {
        for w in a.windows() {
            match w.status(now) {
                Status::Ok => ok += 1,
                Status::Warning | Status::Critical => warn += 1,
                Status::Empty => empty += 1,
            }
        }
    }
    if ok + warn + empty == 0 {
        return (
            lang.t("Lendo as cotas…", "Reading quotas…").into(),
            pal.faint,
        );
    }
    let attention = |n: usize| match lang {
        Lang::Pt => format!("{n} {} atenção", plural(n, "pede", "pedem")),
        Lang::En => format!("{n} {} attention", plural(n, "needs", "need")),
    };
    let depleted = |n: usize| match lang {
        Lang::Pt => format!("{n} {}", plural(n, "esgotada", "esgotadas")),
        Lang::En => format!("{n} depleted"),
    };
    match (empty, warn) {
        (0, 0) => (lang.t("Tudo dentro do ritmo", "All on pace").into(), pal.ok),
        (0, w) => (attention(w), pal.warn),
        (e, 0) => (depleted(e), pal.empty),
        (e, w) => (format!("{} · {}", depleted(e), attention(w)), pal.warn),
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

fn countdown(secs: i64, lang: Lang) -> String {
    if secs <= 0 {
        return lang.t("agora", "now").into();
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

fn local_when(at: DateTime<Utc>, lang: Lang) -> String {
    use chrono::{Datelike, Timelike};
    let l = at.with_timezone(&Local);
    let today = Local::now().date_naive();
    let day = if l.date_naive() == today {
        lang.t("hoje", "today").to_string()
    } else if l.date_naive() == today.succ_opt().unwrap_or(today) {
        lang.t("amanhã", "tomorrow").to_string()
    } else {
        let wd = l.weekday().num_days_from_monday() as usize;
        let mo = l.month0() as usize;
        match lang {
            Lang::Pt => {
                const WD: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
                const MO: [&str; 12] = [
                    "jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov",
                    "dez",
                ];
                format!("{}, {} {}", WD[wd], l.day(), MO[mo])
            }
            Lang::En => {
                const WD: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
                const MO: [&str; 12] = [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ];
                format!("{}, {} {}", WD[wd], MO[mo], l.day())
            }
        }
    };
    format!(
        "{day} {} {:02}:{:02}",
        lang.t("às", "at"),
        l.hour(),
        l.minute()
    )
}

/// Plain-language pace: will this window run out before it resets?
fn pace_text(w: &Window, now: DateTime<Utc>, lang: Lang) -> Option<String> {
    let t_left = w.time_left_fraction(now)?;
    let len = w.length?.as_secs_f32();
    let elapsed = (1.0 - t_left) * len;
    let used = 1.0 - w.remaining;
    if w.remaining <= 0.005 {
        return Some(
            lang.t("Esgotada até renovar", "Depleted until it resets")
                .into(),
        );
    }
    if elapsed < len * 0.02 || used < 0.01 {
        return Some(
            lang.t(
                "Ainda sem consumo relevante nesta janela",
                "No meaningful use in this window yet",
            )
            .into(),
        );
    }
    let rate = used / elapsed; // fraction per second
    let to_empty = w.remaining / rate;
    let until_reset = t_left * len;
    if to_empty < until_reset {
        let (a, b) = (
            countdown(to_empty as i64, lang),
            countdown((until_reset - to_empty) as i64, lang),
        );
        Some(match lang {
            Lang::Pt => format!("Neste ritmo, acaba em {a} — {b} antes de renovar"),
            Lang::En => format!("At this pace, runs out in {a} — {b} before it resets"),
        })
    } else {
        let leftover = pct((w.remaining - rate * until_reset).max(0.0));
        Some(match lang {
            Lang::Pt => format!("Neste ritmo, sobram ~{leftover}% na renovação"),
            Lang::En => format!("At this pace, ~{leftover}% will be left at reset"),
        })
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

/// Sun (light), moon (dark) or monitor (follow the system).
fn theme_icon(p: &Painter, c: Pos2, mode: ThemeMode, color: Color32, bg: Color32) {
    let s = Stroke::new(1.5, color);
    match mode {
        ThemeMode::Light => {
            p.circle_stroke(c, 3.4, s);
            for i in 0..8 {
                let d = Vec2::angled(i as f32 / 8.0 * TAU);
                p.line_segment([c + d * 5.6, c + d * 7.6], s);
            }
        }
        ThemeMode::Dark => {
            p.circle_filled(c, 6.5, color);
            p.circle_filled(c + vec2(3.2, -2.6), 5.4, bg);
        }
        ThemeMode::System => {
            let screen = Rect::from_center_size(c - vec2(0.0, 1.5), vec2(14.0, 9.5));
            p.rect_stroke(screen, 1.5, s, StrokeKind::Middle);
            p.line_segment(
                [pos2(c.x, screen.bottom()), pos2(c.x, screen.bottom() + 3.0)],
                s,
            );
            p.line_segment(
                [
                    pos2(c.x - 3.5, screen.bottom() + 3.2),
                    pos2(c.x + 3.5, screen.bottom() + 3.2),
                ],
                s,
            );
        }
    }
}
