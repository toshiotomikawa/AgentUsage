//! The dashboard: one card per account, one row per quota window. Everything is painted by hand
//! so spacing, motion and color stay exact.

use crate::anim::{Tweens, ease_in_out, ease_out_cubic};
use crate::i18n::Lang;
use crate::icons;
use crate::model::{Account, Reading, Status, Window};
use crate::prefs::ThemeMode;
use crate::theme::{Palette, SEMIBOLD};
use crate::update::State as UpdateState;
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
    pub now: DateTime<Utc>,
    pub lang: Lang,
    pub theme: ThemeMode,
    pub update: UpdateState,
}

/// What the user asked for this frame.
#[derive(Default)]
pub struct Actions {
    pub refresh: bool,
    pub cycle_theme: bool,
    pub toggle_lang: bool,
    pub check_update: bool,
}

pub struct Dashboard {
    tweens: Tweens,
    start: Option<f64>,
    /// Refresh icon angle; it turns only after the user asks for a refresh.
    spin: f32,
    /// Start of the refresh icon's current turn.
    spin_from: Option<f64>,
    /// When the last asked-for refresh finished (the check mark shows for a moment).
    done_at: Option<f64>,
    menu_open: bool,
    /// When the menu last opened or closed, for its fade.
    menu_at: f64,
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
            pad: 24.0 * k.max(0.8),
            head: CIRCLE,
            row: 50.0 * k,
            group: 26.0 * k,
            gap: 16.0 * k.max(0.75),
        }
    }
}

const BAR_H: f32 = 6.0;
/// Densest layout: rows stay readable (label line plus bar) down to this scale.
const MIN_K: f32 = 0.6;
/// Every circle (title bar buttons, provider icon discs, status rings) is this wide.
const CIRCLE: f32 = 28.0;
const BTN_R: f32 = CIRCLE / 2.0;
/// Concentric status rings: stroke width and radii (outer hours, middle week, inner month).
/// The outer edge of the outer ring is exactly `CIRCLE` wide.
const RING_W: f32 = 2.0;
const RING_R: [f32; 3] = [
    BTN_R - RING_W / 2.0,
    BTN_R - RING_W / 2.0 - 4.5,
    BTN_R - RING_W / 2.0 - 9.0,
];
/// Card corner radius, and the space between a card's header and its rows (times `gap`).
const CARD_R: u8 = 10;
const BODY_GAP: f32 = 1.0;
/// Narrowest card that still reads well, used before falling back to scrolling.
const MIN_CARD_W: f32 = 220.0;
/// One eased turn of the refresh icon, and how long the check mark stays after it.
const TURN: f64 = 0.6;
const DONE: f64 = 1.1;
const MENU_FADE: f64 = 0.18;

impl Dashboard {
    pub fn new() -> Self {
        Self {
            tweens: Tweens::default(),
            start: None,
            spin: 0.0,
            spin_from: None,
            done_at: None,
            menu_open: false,
            menu_at: f64::NEG_INFINITY,
        }
    }

    /// The user asked for a refresh (button or keyboard): one eased turn, more while busy.
    pub fn note_refresh(&mut self, now: f64) {
        if self.spin_from.is_none() {
            self.spin_from = Some(now);
        }
        self.done_at = None;
    }

    fn advance_spin(&mut self, now: f64, busy: bool) {
        let Some(mut t0) = self.spin_from else { return };
        if now - t0 >= TURN {
            if busy {
                // Still working: start another turn.
                t0 += TURN * ((now - t0) / TURN).floor();
                self.spin_from = Some(t0);
            } else {
                self.spin_from = None;
                self.spin = 0.0;
                self.done_at = Some(now);
                return;
            }
        }
        self.spin = TAU * ease_in_out(((now - t0) / TURN) as f32);
    }

    fn animating(&self, now: f64) -> bool {
        self.spin_from.is_some()
            || self.done_at.is_some_and(|t| now - t < DONE)
            || now - self.menu_at < MENU_FADE
    }

    /// Refresh and menu buttons in the title bar, ending at `right` (left of the window
    /// buttons), and the menu itself.
    pub fn title_buttons(
        &mut self,
        ui: &mut Ui,
        pal: &Palette,
        f: &Frame<'_>,
        bar: Rect,
        right: f32,
    ) -> Actions {
        let mut act = Actions::default();
        let now = ui.input(|i| i.time);
        let lang = f.lang;
        let p = ui.painter().clone();
        let cy = bar.center().y;
        let menu_c = pos2(right - 10.0 - BTN_R, cy);
        let refresh_c = menu_c - vec2(CIRCLE + 8.0, 0.0);

        // Refresh: one eased turn, then a brief check mark once the data is in.
        let (resp, hover) = self.round_button(ui, &p, pal, refresh_c, "refresh", false);
        if resp.clicked() {
            self.note_refresh(now);
            act.refresh = true;
        }
        self.advance_spin(now, f.busy);
        let glyph = lerp_color(pal.muted, pal.text, hover);
        let since_done = self.done_at.map(|t| (now - t) as f32);
        let check = since_done.map_or(0.0, |d| {
            ease_out_cubic(d / 0.15) * (1.0 - ease_in_out((d - 0.8) / 0.3))
        });
        if check < 1.0 {
            refresh_icon(
                &p,
                refresh_c,
                6.5,
                self.spin,
                glyph.gamma_multiply(1.0 - check),
            );
        }
        if let Some(d) = since_done
            && check > 0.0
        {
            check_icon(
                &p,
                refresh_c,
                ease_out_cubic(d / 0.25),
                pal.ok.gamma_multiply(check),
            );
        }
        resp.on_hover_text(lang.t("Recarregar \u{b7} F5", "Reload \u{b7} F5"));

        // Menu button.
        let (resp, hover) = self.round_button(ui, &p, pal, menu_c, "menu", self.menu_open);
        let glyph = lerp_color(pal.muted, pal.text, hover);
        for dy in [-4.0, 0.0, 4.0] {
            p.line_segment(
                [menu_c + vec2(-5.5, dy), menu_c + vec2(5.5, dy)],
                Stroke::new(1.5, glyph),
            );
        }
        let menu_clicked = resp.clicked();
        resp.on_hover_text(lang.t("Menu", "Menu"));
        if menu_clicked {
            self.menu_open = !self.menu_open;
            self.menu_at = now;
        }

        // The menu, right-aligned under its button.
        const ITEM_H: f32 = 34.0;
        const MENU_W: f32 = 200.0;
        const MENU_PAD: f32 = 6.0;
        let menu_rect = Rect::from_min_size(
            pos2(menu_c.x + BTN_R - MENU_W, bar.bottom() + 4.0),
            vec2(MENU_W, ITEM_H * 4.0 + MENU_PAD * 2.0),
        );
        let fade = ((now - self.menu_at) / MENU_FADE).min(1.0) as f32;
        let shown = if self.menu_open {
            ease_out_cubic(fade)
        } else {
            1.0 - ease_out_cubic(fade)
        };
        if shown > 0.0 {
            let area = egui::Area::new(ui.scope_id().with("menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(menu_rect.min)
                .interactable(self.menu_open)
                .show(ui.ctx(), |ui| {
                    let rect = menu_rect.translate(vec2(0.0, (1.0 - shown) * -4.0));
                    let _ = ui.allocate_exact_size(menu_rect.size(), Sense::hover());
                    let p = ui.painter().clone();
                    let shadow = egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 18,
                        spread: 0,
                        color: Color32::from_black_alpha(
                            ((if pal.dark { 90.0 } else { 26.0 }) * shown) as u8,
                        ),
                    };
                    p.add(shadow.as_shape(rect, CornerRadius::same(10)));
                    p.rect_filled(rect, CornerRadius::same(10), pal.card.gamma_multiply(shown));
                    p.rect_stroke(
                        rect,
                        CornerRadius::same(10),
                        Stroke::new(1.0, pal.border.gamma_multiply(shown)),
                        StrokeKind::Inside,
                    );
                    let mut chosen = None;
                    let items = [Item::Lang, Item::Theme, Item::GitHub, Item::Update];
                    for (i, item) in items.into_iter().enumerate() {
                        let r = Rect::from_min_size(
                            pos2(
                                rect.left() + MENU_PAD,
                                rect.top() + MENU_PAD + i as f32 * ITEM_H,
                            ),
                            vec2(MENU_W - MENU_PAD * 2.0, ITEM_H),
                        );
                        let resp = ui.interact(r, ui.scope_id().with(("item", i)), Sense::click());
                        let h = ui.ctx().animate_bool_with_time(
                            ui.scope_id().with(("item-h", i)),
                            resp.hovered(),
                            0.15,
                        );
                        if h > 0.0 {
                            p.rect_filled(
                                r,
                                CornerRadius::same(7),
                                pal.track.gamma_multiply(h * shown),
                            );
                        }
                        let ink =
                            lerp_color(pal.muted, pal.text, 0.55 + 0.45 * h).gamma_multiply(shown);
                        let ic = pos2(r.left() + 18.0, r.center().y);
                        match item {
                            Item::Lang => {
                                p.text(
                                    ic + vec2(0.0, 0.5),
                                    Align2::CENTER_CENTER,
                                    lang.code(),
                                    FontId::new(10.5, FontFamily::Name(SEMIBOLD.into())),
                                    ink,
                                );
                            }
                            Item::Theme => theme_icon(&p, ic, f.theme, ink, pal.card),
                            Item::GitHub => icons::paint_github(
                                ui,
                                Rect::from_center_size(ic, vec2(15.0, 15.0)),
                                ink,
                            ),
                            Item::Update => update_icon(&p, ic, ink),
                        }
                        let word = match item {
                            Item::Lang => lang.t("IDIOMA", "LANGUAGE"),
                            Item::Theme => lang.t("TEMA", "THEME"),
                            Item::GitHub => "GITHUB",
                            Item::Update => lang.t("ATUALIZAR", "UPDATE"),
                        };
                        p.text(
                            pos2(r.left() + 40.0, r.center().y + 0.5),
                            Align2::LEFT_CENTER,
                            word,
                            FontId::new(12.0, FontFamily::Name(SEMIBOLD.into())),
                            ink,
                        );
                        // A quiet note on the right: the current theme, or the update check.
                        let note = match (item, &f.update) {
                            (Item::Theme, _) => Some((f.theme.name(lang).to_string(), pal.faint)),
                            (Item::Update, UpdateState::Checking) => {
                                Some(("\u{2026}".to_string(), pal.faint))
                            }
                            (Item::Update, UpdateState::UpToDate) => {
                                Some((lang.t("em dia", "up to date").to_string(), pal.ok))
                            }
                            (Item::Update, UpdateState::Available(v, _)) => {
                                Some((format!("v{v}"), pal.warn))
                            }
                            (Item::Update, UpdateState::Failed) => {
                                Some((lang.t("sem conex\u{e3}o", "offline").to_string(), pal.faint))
                            }
                            _ => None,
                        };
                        if let Some((text, color)) = note {
                            p.text(
                                pos2(r.right() - 10.0, r.center().y + 0.5),
                                Align2::RIGHT_CENTER,
                                text,
                                FontId::proportional(11.5),
                                color.gamma_multiply(shown),
                            );
                        }
                        if resp.clicked() {
                            chosen = Some(item);
                        }
                    }
                    chosen
                });
            match area.inner {
                Some(Item::Lang) => act.toggle_lang = true,
                Some(Item::Theme) => act.cycle_theme = true,
                Some(Item::GitHub) => {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(icons::REPO_URL));
                    self.close_menu(now);
                }
                Some(Item::Update) => match &f.update {
                    UpdateState::Available(_, url) => {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        self.close_menu(now);
                    }
                    _ => act.check_update = true,
                },
                None => {}
            }
        }
        // Close on Escape or a click anywhere else.
        if self.menu_open && !menu_clicked {
            let outside = ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .press_origin()
                        .is_some_and(|o| !menu_rect.contains(o) && !resp_rect(menu_c).contains(o))
            });
            if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.close_menu(now);
            }
        }
        if self.animating(now) {
            ui.ctx().request_repaint();
        }
        act
    }

    fn close_menu(&mut self, now: f64) {
        if self.menu_open {
            self.menu_open = false;
            self.menu_at = now;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, pal: &Palette, f: Frame<'_>) {
        let now = ui.input(|i| i.time);
        let start = *self.start.get_or_insert(now);
        let since = (now - start) as f32;
        self.tweens.active = false;

        let full = ui.max_rect();
        let merged = crate::model::merge_claude(f.accounts);
        let mut cards: Vec<&Account> = merged.iter().collect();
        cards.sort_by(|a, b| {
            a.title()
                .to_lowercase()
                .cmp(&b.title().to_lowercase())
                .then(a.account.cmp(&b.account))
                .then(a.key.cmp(&b.key))
        });
        // Everything must fit without scrolling: when even the densest layout does not, scale
        // the whole window down (largest zoom that fits, never above the system's).
        let zoom = ui.ctx().zoom_factor();
        let want = (0..=10)
            .map(|i| 1.0 - i as f32 * 0.05)
            .find(|&z| {
                let r = Rect::from_min_size(full.min, full.size() * zoom / z);
                let g = Grid::plan(&cards, r);
                g.total_h <= g.avail_h
            })
            .unwrap_or(0.5);
        if (want - zoom).abs() > 0.01 {
            ui.ctx().set_zoom_factor(want);
            ui.ctx().request_repaint();
        }
        let Grid {
            outer,
            content_w,
            left,
            grid_top,
            cols,
            m,
            rows,
            total_h,
            ..
        } = Grid::plan(&cards, full);

        // ---------- grid ----------
        let mut per_provider: HashMap<_, usize> = HashMap::new();
        for a in &cards {
            *per_provider.entry(a.provider).or_default() += 1;
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
                    // An incomplete last row stays aligned to the left, on the same columns.
                    for (c, acc) in row_cards.iter().enumerate() {
                        let rect = Rect::from_min_size(
                            pos2(left + c as f32 * (card_w + m.gap), area.top() + y),
                            vec2(card_w, h),
                        );
                        let show_account =
                            per_provider.get(&acc.provider).copied().unwrap_or(0) > 1;
                        self.card(ui, pal, &f, acc, rect, &m, idx, since, now, show_account);
                        idx += 1;
                    }
                }
            });

        if self.tweens.active || self.animating(now) {
            ui.ctx().request_repaint();
        } else {
            // Countdowns tick once a second.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1000));
        }
    }

    /// A round title bar button; returns its response and hover amount.
    fn round_button(
        &mut self,
        ui: &Ui,
        p: &Painter,
        pal: &Palette,
        c: Pos2,
        id: &str,
        active: bool,
    ) -> (Response, f32) {
        let resp = ui.interact(
            resp_rect(c),
            ui.scope_id().with(("btn", id)),
            Sense::click(),
        );
        let hover = ui.ctx().animate_bool_with_time(
            ui.scope_id().with(("btn-h", id)),
            resp.hovered() || active,
            0.15,
        );
        let press = if resp.is_pointer_button_down_on() {
            0.6
        } else {
            0.0
        };
        p.circle_filled(c, BTN_R, lerp_color(pal.card, pal.card_hover, hover));
        p.circle_stroke(
            c,
            BTN_R - 0.5,
            Stroke::new(
                1.0,
                lerp_color(pal.border, pal.border_hover, (hover + press).min(1.0)),
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
        let appear = ease_out_cubic((since - idx as f32 * 0.03) / 0.25);
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
                CornerRadius::same(CARD_R + 1),
                Color32::from_black_alpha((10.0 * hover * a) as u8),
            );
        }
        p.rect_filled(
            rect,
            CornerRadius::same(CARD_R),
            lerp_color(pal.card, pal.card_hover, hover).gamma_multiply(appear),
        );
        p.rect_stroke(
            rect,
            CornerRadius::same(CARD_R),
            Stroke::new(
                1.0,
                lerp_color(pal.border, pal.border_hover, hover).gamma_multiply(appear),
            ),
            StrokeKind::Inside,
        );

        // Header: icon on a disc, title, optional account, status rings. The disc and the
        // rings are the same size as the title bar buttons.
        let inner = rect.shrink(m.pad);
        let hy = inner.top() + m.head / 2.0;
        let disc = pos2(inner.left() + BTN_R, hy);
        p.circle_filled(disc, BTN_R, pal.track.gamma_multiply(a));
        icons::paint_provider(
            ui,
            acc.provider,
            Rect::from_center_size(disc, Vec2::splat(15.0)),
            pal.text,
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
            pos2(inner.left() + CIRCLE + 10.0, hy - title.size().y / 2.0),
            title,
            pal.text,
        );
        if show_account && let Some(name) = &acc.account {
            p.text(
                pos2(inner.left() + CIRCLE + 10.0 + title_w + 7.0, hy + 0.5),
                Align2::LEFT_CENTER,
                format!("·  {name}"),
                FontId::proportional(13.0),
                pal.muted.gamma_multiply(a),
            );
        }

        let body_top = inner.top() + m.head + m.gap * BODY_GAP;
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
                // Concentric rings, outer to inner: hours, week, month (the tightest window
                // of each). Same place and size on every card; a ring shows only if it applies.
                let rc = pos2(inner.right() - BTN_R, hy);
                let mut tip = Vec::new();
                for (i, w) in acc.by_period().into_iter().enumerate() {
                    let Some(w) = w else { continue };
                    let st = w.status(f.now);
                    let v = self.tweens.get(
                        &format!("ring:{}:{i}", acc.key),
                        w.remaining,
                        0.0,
                        now,
                        0.3,
                        0.05 + idx as f32 * 0.03 + i as f32 * 0.04,
                    );
                    let track = if st == Status::Empty {
                        lerp_color(pal.track, pal.empty, 0.45)
                    } else {
                        pal.track
                    };
                    p.circle_stroke(rc, RING_R[i], Stroke::new(RING_W, track.gamma_multiply(a)));
                    arc(
                        &p,
                        rc,
                        RING_R[i],
                        v,
                        Stroke::new(RING_W, pal.status(st).gamma_multiply(a)),
                    );
                    tip.push(format!("{} · {}%", lang.data(&w.label), pct(w.remaining)));
                }
                if !tip.is_empty() {
                    let ring_rect = Rect::from_center_size(rc, Vec2::splat(CIRCLE + 4.0));
                    ui.interact(
                        ring_rect,
                        ui.scope_id().with(("ring", &acc.key)),
                        Sense::hover(),
                    )
                    .on_hover_text(tip.join(
                        "
",
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
                        let delay = 0.05 + idx as f32 * 0.03 + row_i as f32 * 0.02;
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
        let v = self.tweens.get(id, w.remaining, 0.0, c.now, 0.3, c.delay);
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
        let pct_g = p.layout_no_wrap(
            format!("{}%", pct(v)),
            FontId::new(13.5, FontFamily::Name(SEMIBOLD.into())),
            pal.text.gamma_multiply(a),
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
            // Drawn straight from the clock: it moves too slowly to need a tween, and a tween
            // restarted every second would keep the window repainting.
            let tv = t;
            let appear = ease_in_out(
                ((c.now as f32) - (self.start.unwrap_or(0.0) as f32) - c.delay - 0.2) / 0.2,
            );
            if appear < 1.0 {
                self.tweens.active = true;
            }
            let x = bar.left() + bar.width() * tv;
            let tick = Rect::from_center_size(pos2(x, bar.center().y), vec2(12.0, 20.0));
            let tick_resp = ui.interact(tick, ui.scope_id().with((id, "pace")), Sense::hover());
            let tick_h = ui.ctx().animate_bool_with_time(
                ui.scope_id().with((id, "pace-h")),
                tick_resp.hovered(),
                0.15,
            );
            let h = 6.0 + 2.0 * hover.max(tick_h);
            p.line_segment(
                [pos2(x, bar.center().y - h), pos2(x, bar.center().y + h)],
                Stroke::new(
                    1.6 + 0.6 * tick_h,
                    pal.pace.gamma_multiply((0.55 + 0.35 * tick_h) * a * appear),
                ),
            );
            tick_resp.on_hover_ui(|ui| {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, color);
                    ui.label(status_name(st, lang));
                });
            });
        }

        if w.resets_at.is_some() || pace_text(w, c.now_utc, lang).is_some() {
            resp.on_hover_ui(|ui| {
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
}

#[derive(Clone, Copy)]
enum Item {
    Lang,
    Theme,
    GitHub,
    Update,
}

fn resp_rect(c: Pos2) -> Rect {
    Rect::from_center_size(c, Vec2::splat(CIRCLE))
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
/// Where everything goes for a window area: the densest layout that fits, or the smallest.
struct Grid {
    outer: f32,
    content_w: f32,
    left: f32,
    grid_top: f32,
    avail_h: f32,
    cols: usize,
    m: Metrics,
    rows: Vec<(f32, f32)>,
    total_h: f32,
}

impl Grid {
    fn plan(cards: &[&Account], full: Rect) -> Grid {
        let outer = (full.width() * 0.045).clamp(16.0, 56.0);
        let content_w = (full.width() - outer * 2.0).min(1280.0);
        let left = full.center().x - content_w / 2.0;
        // The same margin above and below the grid.
        let compact = full.height() < 640.0;
        let margin = outer * if compact { 0.35 } else { 0.6 };
        let grid_top = full.top() + margin;
        let avail_h = full.bottom() - grid_top - margin;

        let max_cols = if content_w >= 980.0 {
            3
        } else if content_w >= 620.0 {
            2
        } else {
            1
        };
        // Pick the largest density that fits; when even the smallest does not, add a column
        // (as long as cards stay readable).
        let fit = |cols: usize| {
            let mut k = 1.0;
            loop {
                let m = Metrics::at(k);
                let (rows, total_h) = grid_rows(cards, cols, &m);
                if total_h <= avail_h || k <= MIN_K {
                    return (cols, m, rows, total_h);
                }
                k -= 0.05;
            }
        };
        let n = cards.len().max(1);
        let mut best = fit(max_cols.min(n));
        let mut cols = best.0;
        while best.3 > avail_h && cols < 3.min(n) {
            cols += 1;
            let gap = Metrics::at(MIN_K).gap;
            if (content_w - gap * (cols - 1) as f32) / (cols as f32) < MIN_CARD_W {
                break;
            }
            let next = fit(cols);
            if next.3 < best.3 {
                best = next;
            }
        }
        let (cols, m, rows, total_h) = best;
        Grid {
            outer,
            content_w,
            left,
            grid_top,
            avail_h,
            cols,
            m,
            rows,
            total_h,
        }
    }
}

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

/// Height of a card whose content ends exactly `pad` above its bottom edge, like the top.
fn card_height(a: &Account, m: &Metrics) -> f32 {
    let body = match &a.reading {
        // The last placeholder bar ends 30 px into its row.
        Reading::Loading => m.row + 30.0,
        Reading::Unavailable(_) => 40.0,
        Reading::Ok(groups) => {
            let rows: usize = groups.iter().map(|g| g.windows.len()).sum();
            let heads = if groups.len() > 1 {
                groups.len() as f32 * m.group
            } else {
                0.0
            };
            // The last row's bar ends 28 px into it.
            rows.saturating_sub(1) as f32 * m.row + if rows > 0 { 28.0 } else { 0.0 } + heads
        }
    };
    m.pad * 2.0 + m.head + m.gap * BODY_GAP + body
}

fn status_name(st: Status, lang: Lang) -> &'static str {
    match st {
        Status::Ok => lang.t("No ritmo", "On pace"),
        Status::Warning => lang.t("Atenção", "Attention"),
        Status::Critical => lang.t("Crítica", "Critical"),
        Status::Empty => lang.t("Esgotada", "Depleted"),
    }
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
    format!("{day} {:02}:{:02}", l.hour(), l.minute())
}

/// Plain-language pace: will this window run out before it resets?
fn pace_text(w: &Window, now: DateTime<Utc>, lang: Lang) -> Option<String> {
    let t_left = w.time_left_fraction(now)?;
    let len = w.length?.as_secs_f32();
    let elapsed = (1.0 - t_left) * len;
    let used = 1.0 - w.remaining;
    if w.remaining <= 0.005 {
        return None;
    }
    if elapsed < len * 0.02 || used < 0.01 {
        return None;
    }
    let rate = used / elapsed; // fraction per second
    let to_empty = w.remaining / rate;
    let until_reset = t_left * len;
    Some(if to_empty < until_reset {
        let a = countdown(to_empty as i64, lang);
        match lang {
            Lang::Pt => format!("Acaba em {a}"),
            Lang::En => format!("Runs out in {a}"),
        }
    } else {
        let leftover = pct((w.remaining - rate * until_reset).max(0.0));
        match lang {
            Lang::Pt => format!("Sobra ~{leftover}%"),
            Lang::En => format!("~{leftover}% left at reset"),
        }
    })
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

/// A check mark drawn from its short stroke to its long one as `t` goes 0 to 1.
fn check_icon(p: &Painter, c: Pos2, t: f32, color: Color32) {
    let a = c + vec2(-5.0, 0.2);
    let b = c + vec2(-1.6, 3.6);
    let e = c + vec2(5.2, -3.6);
    let (l1, l2) = (a.distance(b), b.distance(e));
    let d = t.clamp(0.0, 1.0) * (l1 + l2);
    let mut pts = vec![a];
    if d <= l1 {
        pts.push(a + (b - a) * (d / l1).max(0.01));
    } else {
        pts.push(b);
        pts.push(b + (e - b) * ((d - l1) / l2));
    }
    p.add(Shape::line(pts, PathStroke::new(1.8, color)));
}

/// An arrow coming down onto a tray: download the new version.
fn update_icon(p: &Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.5, color);
    p.line_segment([c + vec2(0.0, -6.0), c + vec2(0.0, 2.5)], s);
    p.line_segment([c + vec2(-3.5, -1.0), c + vec2(0.0, 2.5)], s);
    p.line_segment([c + vec2(3.5, -1.0), c + vec2(0.0, 2.5)], s);
    p.line_segment([c + vec2(-6.0, 6.0), c + vec2(6.0, 6.0)], s);
}
