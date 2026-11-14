#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod anim;
mod cache;
mod chrome;
mod config;
mod demo;
mod hidden;
mod i18n;
mod icons;
mod model;
mod poller;
mod prefs;
mod providers;
mod share;
mod theme;
#[cfg(windows)]
mod tray;
mod ui;
mod update;

use chrono::Utc;
use eframe::egui::{self, Rect, pos2};
use prefs::{Prefs, ThemeMode};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Args {
    demo: bool,
    theme: Option<ThemeMode>,
    lang: Option<i18n::Lang>,
    screenshot: Option<PathBuf>,
    size: [f32; 2],
}

fn parse_args() -> Args {
    let mut args = Args {
        demo: false,
        theme: None,
        lang: None,
        screenshot: None,
        size: [1180.0, 780.0],
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--demo" => args.demo = true,
            "--light" => args.theme = Some(ThemeMode::Light),
            "--dark" => args.theme = Some(ThemeMode::Dark),
            "--en" => args.lang = Some(i18n::Lang::En),
            "--pt" => args.lang = Some(i18n::Lang::Pt),
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
    icons: icons::Icons,
    prefs: Prefs,
    screenshot: Option<PathBuf>,
    shot_taken: Arc<Mutex<bool>>,
    demo_since: f64,
    first_frame: Option<f64>,
    updates: update::Checker,
    /// The tray menu; None without a tray icon.
    #[cfg(windows)]
    tray: Option<tray::TrayMenu>,
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
            let poller = poller::Poller::start(
                sources,
                Duration::from_secs(refresh_every),
                cc.egui_ctx.clone(),
            );
            share::start(cfg.share_port, poller.shared.clone());
            Data::Live(poller)
        };
        let mut prefs = Prefs::load();
        if let Some(t) = args.theme {
            prefs.theme = t;
        }
        if let Some(l) = args.lang {
            prefs.lang = l;
        }
        // Screenshots show every card, whatever this machine hides.
        if args.screenshot.is_some() {
            prefs.hidden.clear();
        }
        #[cfg(windows)]
        let tray = if args.screenshot.is_none() {
            tray::install(cc, prefs.lang)
        } else {
            None
        };
        Self {
            data,
            dash: ui::Dashboard::new(),
            icons: icons::Icons::load(&cc.egui_ctx),
            prefs,
            screenshot: args.screenshot,
            shot_taken: Arc::new(Mutex::new(false)),
            demo_since: 0.0,
            first_frame: None,
            updates: update::Checker::new(),
            #[cfg(windows)]
            tray,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dark = match self.prefs.theme {
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
            ThemeMode::System => {
                ctx.system_theme().unwrap_or(egui::Theme::Light) == egui::Theme::Dark
            }
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

        // Closing hides the window in the tray; the tray menu ends the app.
        #[cfg(windows)]
        if self.tray.is_some() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            tray::hide();
        }

        let key_refresh = ctx.input(|i| {
            i.key_pressed(egui::Key::F5) || (i.modifiers.command && i.key_pressed(egui::Key::R))
        });
        let now = Utc::now();
        let time = ctx.input(|i| i.time);

        let full = ui.max_rect();
        theme::paint_background(ui.painter(), full, &pal);
        let (bar, buttons_right) =
            chrome::title_bar(ui, &pal, &mut self.icons, self.prefs.lang, full);
        let body = Rect::from_min_max(pos2(full.left(), bar.bottom()), full.max);
        let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));

        let (accounts, busy) = match &self.data {
            Data::Demo(a) => (a.clone(), time - self.demo_since < 0.9),
            Data::Live(p) => {
                let s = p.shared.lock().unwrap();
                (s.accounts.clone(), s.in_flight > 0)
            }
        };
        let f = ui::Frame {
            accounts: &accounts,
            busy,
            now,
            lang: self.prefs.lang,
            theme: self.prefs.theme,
            update: self.updates.state(),
            hidden: &self.prefs.hidden,
        };
        let act = self.dash.title_buttons(ui, &pal, &f, bar, buttons_right);
        self.dash.show(&mut body_ui, &pal, f);
        chrome::frame_edges(ui, &pal, full);
        if act.check_update {
            let c = ctx.clone();
            self.updates.check(move |_| c.request_repaint());
        }

        if act.refresh || key_refresh {
            if key_refresh {
                self.dash.note_refresh(time);
            }
            match &self.data {
                Data::Demo(_) => self.demo_since = time,
                Data::Live(p) => p.refresh(),
            }
        }
        if let Some(key) = &act.toggle_card {
            self.prefs.toggle_card(key);
            self.prefs.save();
            ctx.request_repaint();
        }
        if act.cycle_theme || act.toggle_lang {
            if act.cycle_theme {
                self.prefs.theme = self.prefs.theme.next();
            }
            if act.toggle_lang {
                self.prefs.lang = self.prefs.lang.toggled();
                #[cfg(windows)]
                if let Some(t) = &self.tray {
                    t.relabel(self.prefs.lang);
                }
            }
            self.prefs.save();
            ctx.request_repaint();
        }

        if let Some(path) = self.screenshot.clone()
            && time - *self.first_frame.get_or_insert(time) > 3.2
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

fn window_icon() -> egui::IconData {
    let img = image::load_from_memory(icons::APP_ICON)
        .map(|i| i.resize(128, 128, image::imageops::FilterType::Lanczos3))
        .map(|i| i.to_rgba8());
    match img {
        Ok(rgba) => egui::IconData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: rgba.into_raw(),
        },
        Err(_) => egui::IconData::default(),
    }
}

/// `--background`: no window, only the poller and the phone snapshot. A few MB instead of a
/// GPU-backed window, meant for starting with Windows.
fn run_background() {
    let cfg = config::Config::load();
    if cfg.share_port == 0 {
        return;
    }
    let poller = poller::Poller::start(
        providers::discover(&cfg),
        Duration::from_secs(cfg.refresh_secs.max(15)),
        egui::Context::default(),
    );
    share::start(cfg.share_port, poller.shared.clone());
    loop {
        std::thread::park();
    }
}

fn main() -> eframe::Result {
    // agy updates itself from a detached background process. Started by our quota check, that
    // updater dies with our job object mid-download, and its `agy.exe --version` probe opens a
    // visible console window. The Admin's own agy sessions still keep agy up to date.
    // SAFETY: set before any other thread exists.
    unsafe { std::env::set_var("AGY_CLI_DISABLE_AUTO_UPDATE", "1") };
    if std::env::args().any(|a| a == "--background") {
        run_background();
        return Ok(());
    }
    let mut args = parse_args();
    // Screenshots end up in docs and chats: they always use the fictional accounts.
    if args.screenshot.is_some() {
        args.demo = true;
    }
    // Opening the app again (e.g. from its shortcut) brings the open window forward.
    #[cfg(windows)]
    if args.screenshot.is_none() && tray::focus_running() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AgentUsage")
            .with_app_id("agent-usage")
            .with_inner_size(args.size)
            .with_min_inner_size([400.0, 460.0])
            .with_decorations(false)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "AgentUsage",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc, args)))),
    )
}
