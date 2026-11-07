#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod config;
mod demo;
mod icons;
mod model;
mod poller;
mod providers;
mod theme;
mod ui;

use chrono::Utc;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Args {
    demo: bool,
    theme: Option<egui::Theme>,
    screenshot: Option<PathBuf>,
    size: [f32; 2],
}

fn parse_args() -> Args {
    let mut args = Args {
        demo: false,
        theme: None,
        screenshot: None,
        size: [1180.0, 760.0],
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => args.demo = true,
            "--light" => args.theme = Some(egui::Theme::Light),
            "--dark" => args.theme = Some(egui::Theme::Dark),
            "--screenshot" => args.screenshot = it.next().map(PathBuf::from),
            "--size" => {
                if let Some((w, h)) = it.next().as_deref().and_then(|s| s.split_once('x'))
                    && let (Ok(w), Ok(h)) = (w.parse(), h.parse())
                {
                    args.size = [w, h];
                }
            }
            _ => {}
        }
    }
    args
}

enum Data {
    Demo(Vec<model::Account>),
    Live(poller::Poller),
}

struct App {
    data: Data,
    dash: ui::Dashboard,
    refresh_every: u64,
    theme: Option<egui::Theme>,
    screenshot: Option<PathBuf>,
    shot_taken: Arc<Mutex<bool>>,
    demo_since: f64,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, args: Args) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let cfg = config::Config::load();
        let refresh_every = cfg.refresh_secs.max(15);
        let data = if args.demo {
            Data::Demo(demo::accounts())
        } else {
            let sources = providers::discover(&cfg);
            Data::Live(poller::Poller::start(
                sources,
                Duration::from_secs(refresh_every),
                cc.egui_ctx.clone(),
            ))
        };
        Self {
            data,
            dash: ui::Dashboard::new(),
            refresh_every,
            theme: args.theme,
            screenshot: args.screenshot,
            shot_taken: Arc::new(Mutex::new(false)),
            demo_since: 0.0,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dark = match self.theme {
            Some(t) => t == egui::Theme::Dark,
            None => ctx.system_theme().unwrap_or(egui::Theme::Light) == egui::Theme::Dark,
        };
        let pal = if dark {
            theme::Palette::dark()
        } else {
            theme::Palette::light()
        };
        ctx.set_theme(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });

        let key_refresh = ctx.input(|i| {
            i.key_pressed(egui::Key::F5) || (i.modifiers.command && i.key_pressed(egui::Key::R))
        });
        let now = Utc::now();
        let time = ctx.input(|i| i.time);

        let clicked = match &mut self.data {
            Data::Demo(accounts) => {
                let busy = time - self.demo_since < 0.9;
                let f = ui::Frame {
                    accounts,
                    busy,
                    last_refresh: Some(now - chrono::Duration::seconds(12)),
                    refresh_every: self.refresh_every,
                    now,
                };
                let r = self.dash.show(ui, &pal, f);
                if r || key_refresh {
                    self.demo_since = time;
                }
                false
            }
            Data::Live(p) => {
                let (accounts, busy, last) = {
                    let s = p.shared.lock().unwrap();
                    (s.accounts.clone(), s.in_flight > 0, s.last_refresh)
                };
                let f = ui::Frame {
                    accounts: &accounts,
                    busy,
                    last_refresh: last,
                    refresh_every: self.refresh_every,
                    now,
                };
                self.dash.show(ui, &pal, f) || key_refresh
            }
        };
        if clicked && let Data::Live(p) = &self.data {
            p.refresh();
        }

        if let Some(path) = self.screenshot.clone()
            && time > 3.2
            && !*self.shot_taken.lock().unwrap()
        {
            *self.shot_taken.lock().unwrap() = true;
            let c2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let mut buf = Vec::with_capacity(w * h * 4);
                for px in &img.pixels {
                    buf.extend_from_slice(&px.to_srgba_unmultiplied());
                }
                if let Some(rgba) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                    let _ = rgba.save(&path);
                }
                c2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if self.screenshot.is_some() {
            ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    let args = parse_args();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Agent Usage")
            .with_app_id("agent-usage")
            .with_inner_size(args.size)
            .with_min_inner_size([380.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Agent Usage",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc, args)))),
    )
}
