#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod providers;
mod sys;

use eframe::egui;
use providers::{Provider, GROUPS, QUICK};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Instant;

const ANIM: f32 = 0.22; // seconds for UI transitions

const DOMAIN: &str = "www.google.com";
const TIMEOUT_MS: u64 = 500;

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Simple,
    Advanced,
}

enum Msg {
    Status(String, bool), // text, ok?
    TestResult(usize, Option<u32>),
    TestDone,
}

struct App {
    mode: Mode,
    ipv6: bool,
    adapters: Vec<String>,
    adapter_idx: usize,
    group_idx: usize,
    server_idx: usize,
    custom1: String,
    custom2: String,
    use_custom: bool,
    status: String,
    status_ok: bool,
    busy: bool,
    results: Vec<Option<u32>>,
    testing: bool,
    best: Option<usize>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    shot_path: Option<String>,
    frames: u32,
    prev_mode: Mode,
    mode_since: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, shot_path: Option<String>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        // Smooth transitions for hover/selection highlights and toggles.
        style.animation_time = ANIM;
        cc.egui_ctx.set_style(style);

        let (tx, rx) = std::sync::mpsc::channel();
        let adapters = sys::adapters();
        App {
            mode: Mode::Simple,
            ipv6: false,
            adapters,
            adapter_idx: 0,
            group_idx: 0,
            server_idx: 0,
            custom1: String::new(),
            custom2: String::new(),
            use_custom: false,
            status: "Ready. Pick a DNS and press Apply.".into(),
            status_ok: true,
            busy: false,
            results: Vec::new(),
            testing: false,
            best: None,
            tx,
            rx,
            shot_path,
            frames: 0,
            prev_mode: Mode::Simple,
            mode_since: Instant::now() - std::time::Duration::from_secs(1),
        }
    }

    fn current_list(&self) -> &'static [Provider] {
        let g = &GROUPS[self.group_idx];
        if self.ipv6 {
            g.ipv6
        } else {
            g.ipv4
        }
    }

    fn adapter_name(&self) -> String {
        self.adapters
            .get(self.adapter_idx)
            .cloned()
            .unwrap_or_default()
    }

    fn chosen_dns(&self) -> (String, String) {
        if self.use_custom {
            (self.custom1.trim().to_string(), self.custom2.trim().to_string())
        } else {
            let list = self.current_list();
            match list.get(self.server_idx) {
                Some(p) => (p.dns1.to_string(), p.dns2.to_string()),
                None => (String::new(), String::new()),
            }
        }
    }

    fn apply(&mut self, auto_flush: bool) {
        let (d1, d2) = self.chosen_dns();
        if d1.is_empty() {
            self.set_status("Enter a DNS address first.", false);
            return;
        }
        let adapter = self.adapter_name();
        if adapter.is_empty() {
            self.set_status("No network adapter found.", false);
            return;
        }
        let ipv6 = self.ipv6;
        let tx = self.tx.clone();
        self.busy = true;
        self.set_status("Applying DNS…", true);
        thread::spawn(move || {
            let r = sys::apply_dns(&adapter, ipv6, &d1, &d2);
            match r {
                Ok(()) => {
                    if auto_flush {
                        let _ = sys::flush_dns();
                    }
                    let _ = tx.send(Msg::Status(
                        format!("DNS successfully changed to {} on \"{}\".", d1, adapter),
                        true,
                    ));
                }
                Err(e) => {
                    let _ = tx.send(Msg::Status(short_err(&e), false));
                }
            }
        });
    }

    fn restore(&mut self) {
        let adapter = self.adapter_name();
        if adapter.is_empty() {
            self.set_status("No network adapter found.", false);
            return;
        }
        let tx = self.tx.clone();
        self.busy = true;
        self.set_status("Restoring automatic DNS…", true);
        thread::spawn(move || {
            let r = sys::restore_dhcp(&adapter);
            let _ = tx.send(match r {
                Ok(()) => Msg::Status(format!("Automatic (DHCP) DNS restored on \"{}\".", adapter), true),
                Err(e) => Msg::Status(short_err(&e), false),
            });
        });
    }

    fn flush(&mut self) {
        let tx = self.tx.clone();
        self.busy = true;
        self.set_status("Flushing DNS cache…", true);
        thread::spawn(move || {
            let r = sys::flush_dns();
            let _ = tx.send(match r {
                Ok(()) => Msg::Status("Successfully flushed the DNS resolver cache.".into(), true),
                Err(e) => Msg::Status(short_err(&e), false),
            });
        });
    }

    fn start_test(&mut self) {
        let list = self.current_list();
        self.results = vec![None; list.len()];
        self.best = None;
        self.testing = true;
        self.set_status("Testing DNS speed…", true);
        let servers: Vec<String> = list.iter().map(|p| p.dns1.to_string()).collect();
        let tx = self.tx.clone();
        thread::spawn(move || {
            for (i, s) in servers.iter().enumerate() {
                let ms = sys::resolve_ms(s, DOMAIN, TIMEOUT_MS);
                let _ = tx.send(Msg::TestResult(i, ms));
            }
            let _ = tx.send(Msg::TestDone);
        });
    }

    fn set_status(&mut self, s: &str, ok: bool) {
        self.status = s.to_string();
        self.status_ok = ok;
    }

    fn pump(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Status(s, ok) => {
                    self.status = s;
                    self.status_ok = ok;
                    self.busy = false;
                }
                Msg::TestResult(i, ms) => {
                    if i < self.results.len() {
                        self.results[i] = ms;
                    }
                }
                Msg::TestDone => {
                    self.testing = false;
                    let mut best: Option<(usize, u32)> = None;
                    for (i, r) in self.results.iter().enumerate() {
                        if let Some(v) = r {
                            if best.map_or(true, |(_, b)| *v < b) {
                                best = Some((i, *v));
                            }
                        }
                    }
                    if let Some((i, v)) = best {
                        self.best = Some(i);
                        self.server_idx = i;
                        self.use_custom = false;
                        self.set_status(
                            &format!(
                                "Fastest: {} ({} ms). Selected — press Apply to use it.",
                                self.current_list()[i].name,
                                v
                            ),
                            true,
                        );
                    } else {
                        self.set_status("No DNS server answered. Check your connection.", false);
                    }
                }
            }
        }
    }
}

fn short_err(e: &str) -> String {
    let low = e.to_lowercase();
    if low.contains("elev") || low.contains("requer") || low.contains("740") || low.contains("denied") || low.contains("acesso") {
        "Permission denied. Run this app as Administrator to change DNS.".into()
    } else {
        let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(e);
        format!("Could not change DNS: {}", first.trim())
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();

        // Detect a Simple/Advanced switch to drive a fade + slide transition.
        if self.mode != self.prev_mode {
            self.prev_mode = self.mode;
            self.mode_since = Instant::now();
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("DNS Switcher");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut self.mode, Mode::Advanced, "Advanced");
                    ui.selectable_value(&mut self.mode, Mode::Simple, "Simple");
                    ui.label("Mode:");
                });
            });
            ui.label(
                egui::RichText::new("Change your DNS in one click — faster browsing, fewer blocks.")
                    .weak(),
            );
            ui.add_space(6.0);
        });

        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.add_space(4.0);
            let col = if self.status_ok {
                egui::Color32::from_rgb(60, 140, 60)
            } else {
                egui::Color32::from_rgb(170, 60, 60)
            };
            ui.horizontal(|ui| {
                if self.busy || self.testing {
                    ui.spinner();
                }
                ui.colored_label(col, &self.status);
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Network:");
                egui::ComboBox::from_id_source("adapter")
                    .selected_text(self.adapter_name())
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (i, a) in self.adapters.iter().enumerate() {
                            ui.selectable_value(&mut self.adapter_idx, i, a);
                        }
                    });
                if ui.button("Refresh").on_hover_text("Refresh adapters").clicked() {
                    self.adapters = sys::adapters();
                    if self.adapter_idx >= self.adapters.len() {
                        self.adapter_idx = 0;
                    }
                }
            });
            ui.separator();

            // Fade and slide the panel contents in on a mode change.
            let t = (self.mode_since.elapsed().as_secs_f32() / ANIM).clamp(0.0, 1.0);
            let ease = t * t * (3.0 - 2.0 * t); // smoothstep
            if ease < 1.0 {
                ui.add_space((1.0 - ease) * 10.0);
                ui.ctx().request_repaint();
            }
            egui::Frame::none().show(ui, |ui| {
                ui.multiply_opacity(0.25 + 0.75 * ease);
                match self.mode {
                    Mode::Simple => self.simple_ui(ui),
                    Mode::Advanced => self.advanced_ui(ui),
                }
            });
        });

        if self.shot_path.is_some() {
            self.frames += 1;
            if self.frames == 90 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            }
            let img = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = img {
                save_screenshot(&image, self.shot_path.as_ref().unwrap());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
        if self.testing || self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

impl App {
    fn simple_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Pick a DNS:").strong());
        ui.add_space(4.0);
        let mut pending: Option<(String, String)> = None;
        egui::ScrollArea::vertical()
            .max_height(236.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for q in QUICK {
                    let selected = self.use_custom && self.custom1 == q.dns1;
                    if dns_row(ui, selected, q.name, q.dns1, q.dns2) {
                        pending = Some((q.dns1.to_string(), q.dns2.to_string()));
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some((d1, d2)) = pending {
            self.use_custom = true;
            self.custom1 = d1;
            self.custom2 = d2;
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let apply = egui::Button::new(egui::RichText::new("Apply DNS").size(16.0));
            if ui
                .add_enabled(!self.busy, apply.min_size(egui::vec2(150.0, 40.0)))
                .clicked()
            {
                self.apply(true);
            }
            if ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Automatic").min_size(egui::vec2(130.0, 40.0)),
                )
                .on_hover_text("Restore your provider's default DNS")
                .clicked()
            {
                self.restore();
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.testing, egui::Button::new("Find fastest"))
                .clicked()
            {
                self.group_idx = 0;
                self.ipv6 = false;
                self.start_test();
            }
            if ui.add_enabled(!self.busy, egui::Button::new("Flush cache")).clicked() {
                self.flush();
            }
        });
    }

    fn advanced_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Group:");
            for (i, g) in GROUPS.iter().enumerate() {
                if ui.selectable_label(self.group_idx == i, g.title).clicked() {
                    self.group_idx = i;
                    self.server_idx = 0;
                    self.results.clear();
                }
            }
            ui.separator();
            if ui.checkbox(&mut self.ipv6, "IPv6").changed() {
                self.server_idx = 0;
                self.results.clear();
            }
        });
        ui.add_space(4.0);

        ui.checkbox(&mut self.use_custom, "Use a custom DNS server");
        if self.use_custom {
            ui.horizontal(|ui| {
                ui.label("DNS 1:");
                ui.add(egui::TextEdit::singleline(&mut self.custom1).desired_width(150.0));
                ui.label("DNS 2:");
                ui.add(egui::TextEdit::singleline(&mut self.custom2).desired_width(150.0));
            });
        }
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(220.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // reserve room for the scrollbar so the Speed column isn't clipped
                ui.set_max_width(ui.available_width() - 14.0);
                egui::Grid::new("servers")
                    .striped(true)
                    .num_columns(4)
                    .min_col_width(0.0)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        ui.add_sized([22.0, 16.0], egui::Label::new(""));
                        ui.strong("DNS server");
                        ui.strong("Primary / Secondary");
                        ui.strong("Speed");
                        ui.end_row();
                        let best = self.best;
                        let testing = self.testing;
                        let list = self.current_list();
                        let mut pick: Option<usize> = None;
                        for (i, p) in list.iter().enumerate() {
                            let sel = !self.use_custom && self.server_idx == i;
                            if ui.add_sized([22.0, 18.0], egui::RadioButton::new(sel, "")).clicked() {
                                pick = Some(i);
                            }
                            let mut name = egui::RichText::new(p.name);
                            if best == Some(i) {
                                name = name.color(egui::Color32::from_rgb(60, 140, 60)).strong();
                            }
                            ui.label(name);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} / {}",
                                    p.dns1,
                                    if p.dns2.is_empty() { "—" } else { p.dns2 }
                                ))
                                .monospace()
                                .size(12.0),
                            );
                            match self.results.get(i).copied().flatten() {
                                Some(ms) => {
                                    ui.label(egui::RichText::new(format!("{} ms", ms)).monospace());
                                }
                                None => {
                                    if testing {
                                        ui.weak("…");
                                    } else {
                                        ui.weak("--");
                                    }
                                }
                            };
                            ui.end_row();
                        }
                        if let Some(i) = pick {
                            self.server_idx = i;
                            self.use_custom = false;
                        }
                    });
            });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new(egui::RichText::new("Apply DNS").size(15.0)).min_size(egui::vec2(130.0, 36.0)))
                .clicked()
            {
                self.apply(true);
            }
            if ui.add_enabled(!self.testing, egui::Button::new("Fastest DNS").min_size(egui::vec2(120.0, 36.0))).clicked() {
                self.start_test();
            }
            if ui.add_enabled(!self.busy, egui::Button::new("Flush DNS").min_size(egui::vec2(110.0, 36.0))).clicked() {
                self.flush();
            }
            if ui.add_enabled(!self.busy, egui::Button::new("Default").min_size(egui::vec2(100.0, 36.0))).on_hover_text("Restore automatic (DHCP) DNS").clicked() {
                self.restore();
            }
        });
    }
}

/// A left-aligned, full-width selectable DNS row (two lines), clipped-safe.
fn dns_row(ui: &mut egui::Ui, selected: bool, name: &str, dns1: &str, dns2: &str) -> bool {
    use egui::{Align2, FontId, Pos2, Rounding, Sense, Stroke, Vec2};
    let w = ui.available_width() - 14.0; // keep clear of the scrollbar
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 46.0), Sense::click());
    let visuals = ui.visuals();
    let hovered = resp.hovered();
    let bg = if selected {
        visuals.selection.bg_fill
    } else if hovered {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.faint_bg_color
    };
    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(8.0), bg);
    if selected {
        p.rect_stroke(rect, Rounding::same(8.0), Stroke::new(1.5, visuals.selection.stroke.color));
    } else {
        p.rect_stroke(rect, Rounding::same(8.0), Stroke::new(1.0, visuals.widgets.inactive.bg_stroke.color));
    }
    let text_col = visuals.text_color();
    let weak = visuals.weak_text_color();
    let x = rect.left() + 12.0;
    p.text(Pos2::new(x, rect.top() + 13.0), Align2::LEFT_CENTER, name, FontId::proportional(14.5), text_col);
    let ips = format!("{}  •  {}", dns1, if dns2.is_empty() { "—" } else { dns2 });
    p.text(Pos2::new(x, rect.bottom() - 13.0), Align2::LEFT_CENTER, ips, FontId::monospace(12.5), weak);
    resp.clicked()
}

fn save_screenshot(image: &egui::ColorImage, path: &str) {
    let [w, h] = image.size;
    let mut buf = Vec::with_capacity(w * h * 4);
    for px in &image.pixels {
        buf.extend_from_slice(&[px.r(), px.g(), px.b(), px.a()]);
    }
    if let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, buf) {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = img.save(path);
    }
}

/// Title bar / taskbar icon, decoded from the PNG baked into the binary.
fn window_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut shot_path = None;
    for i in 0..args.len() {
        if args[i] == "--screenshot" {
            shot_path = args.get(i + 1).cloned();
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([520.0, 560.0])
        .with_min_inner_size([460.0, 480.0])
        .with_title("DNS Switcher");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }

    if shot_path.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(windows)]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let shot = shot_path.clone();
    eframe::run_native(
        "DNS Switcher",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, shot.clone())))),
    )
}
