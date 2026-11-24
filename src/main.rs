#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod service;
mod netcfg;
mod sys;
mod tray;
mod updater;

use eframe::egui;
use dns_switcher::ipc::Request;
use dns_switcher::probe;
use dns_switcher::providers::{Provider, GROUPS, QUICK};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Instant;

const ANIM: f32 = 0.22; // seconds for UI transitions
/// How many resolvers are measured at the same time.
const PARALLEL_PROBES: usize = 6;
const GOOD: egui::Color32 = egui::Color32::from_rgb(60, 150, 70);
const BAD: egui::Color32 = egui::Color32::from_rgb(190, 70, 70);


#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Simple,
    Advanced,
}

enum Msg {
    Status(String, bool), // text, ok?
    Current(String, String, Vec<String>), // adapter, DNS in use, provider (DHCP) DNS
    TestResult(u64, usize, probe::Probe), // test generation, index, result
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
    /// DNS servers the router/provider hands out by DHCP for this adapter.
    provider: Vec<String>,
    /// The provider's DNS is the selected choice (applying it = Automatic).
    use_provider: bool,
    /// The last test included the provider's DNS as its last entry.
    tested_provider: bool,
    status: String,
    status_ok: bool,
    busy: bool,
    /// One slot per server in the list that was tested (QUICK in simple mode).
    results: Vec<Option<probe::Probe>>,
    tested_simple: bool,
    test_got: usize,
    test_gen: u64,
    testing: bool,
    /// DNS servers in use on the selected adapter, as reported by Windows.
    current: String,
    /// Adapter the tray menu acts on.
    tray_adapter: std::sync::Arc<std::sync::Mutex<String>>,
    /// Set when the window should hide to the tray instead of closing.
    tray_ready: bool,
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
            provider: Vec::new(),
            use_provider: false,
            tested_provider: false,
            status: "Ready. Pick a DNS and press Apply.".into(),
            status_ok: true,
            busy: false,
            results: Vec::new(),
            tested_simple: false,
            test_got: 0,
            test_gen: 0,
            testing: false,
            current: String::new(),
            tray_adapter: Default::default(),
            tray_ready: false,
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

    /// Reads the adapter's DNS off the UI thread; the answer arrives as Msg::Current.
    fn refresh_current(&mut self) {
        let adapter = self.adapter_name();
        if adapter.is_empty() {
            return;
        }
        let tx = self.tx.clone();
        thread::spawn(move || {
            let dns = sys::current_dns(&adapter);
            let provider = sys::provider_dns(&adapter).iter().map(|ip| ip.to_string()).collect();
            let _ = tx.send(Msg::Current(adapter, dns, provider));
        });
    }

    fn sync_tray_adapter(&self) {
        if let Ok(mut a) = self.tray_adapter.lock() {
            *a = self.adapter_name();
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
        if self.use_provider {
            return self.restore();
        }
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
        let req = match Request::apply(&adapter, self.ipv6, &d1, &d2) {
            Ok(r) => r,
            Err(e) => return self.set_status(&e, false),
        };
        let tx = self.tx.clone();
        self.busy = true;
        self.set_status("Applying DNS…", true);
        thread::spawn(move || {
            let r = service::perform(&req);
            if r.is_ok() && auto_flush {
                let _ = service::perform(&Request::Flush);
            }
            let _ = tx.send(status_msg(r));
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
            let _ = tx.send(status_msg(service::perform(&Request::Restore { adapter })));
        });
    }

    fn flush(&mut self) {
        let tx = self.tx.clone();
        self.busy = true;
        self.set_status("Flushing DNS cache…", true);
        thread::spawn(move || {
            let _ = tx.send(status_msg(service::perform(&Request::Flush)));
        });
    }

    /// Probes every server of the visible list in parallel (QUICK in simple mode).
    fn start_test(&mut self, simple: bool) {
        let list: &[Provider] = if simple { QUICK } else { self.current_list() };
        let mut servers: Vec<String> = list.iter().map(|p| p.dns1.to_string()).collect();
        // Your provider's DNS (IPv4, from DHCP) is the baseline everything is
        // compared with: it's often the closest resolver and the best for CDNs.
        let with_provider = !self.ipv6 || simple;
        self.clear_results();
        self.tested_provider = false;
        if with_provider {
            if let Some(p) = self.provider.first() {
                servers.push(p.clone());
                self.tested_provider = true;
            }
        }
        self.results = vec![None; servers.len()];
        self.tested_simple = simple;
        self.test_got = 0;
        self.best = None;
        self.testing = true;
        self.set_status(&format!("Testing DNS servers… 0/{}", servers.len()), true);
        // A small worker pool: probing every server at once floods home routers
        // and shows up as false packet loss.
        let queue: Vec<(usize, String)> = servers.into_iter().enumerate().collect();
        let queue = std::sync::Arc::new(std::sync::Mutex::new(queue));
        for _ in 0..PARALLEL_PROBES.min(self.results.len()) {
            let (tx, queue, gen) = (self.tx.clone(), queue.clone(), self.test_gen);
            thread::spawn(move || {
                while let Some((i, server)) = queue.lock().ok().and_then(|mut q| q.pop()) {
                    if tx.send(Msg::TestResult(gen, i, probe::probe(&server))).is_err() {
                        break;
                    }
                }
            });
        }
    }

    /// Drops current results; replies from an earlier test are ignored from now on.
    fn clear_results(&mut self) {
        self.results.clear();
        self.best = None;
        self.testing = false;
        self.test_gen += 1;
    }

    fn tested_list(&self) -> &'static [Provider] {
        if self.tested_simple {
            QUICK
        } else {
            self.current_list()
        }
    }

    fn finish_test(&mut self) {
        self.testing = false;
        let Some(i) = probe::best(&self.results) else {
            self.set_status("No DNS server answered. Check your connection.", false);
            return;
        };
        self.best = Some(i);
        let r = self.results[i].as_ref().expect("best has a result");
        let speed = |r: &probe::Probe| match r.cdn {
            Some(c) => format!("{} ms lookup, {c} ms to sites", r.median().unwrap_or(0)),
            None => format!("{} ms lookup", r.median().unwrap_or(0)),
        };
        if self.tested_provider && i == self.results.len() - 1 {
            let msg = format!(
                "Best: your provider's DNS ({}), {}. Keep it: press Apply to use automatic DNS.",
                self.provider.join(", "),
                speed(r)
            );
            self.use_provider = true;
            self.use_custom = false;
            return self.set_status(&msg, true);
        }
        let msg_speed = speed(r);
        self.use_provider = false;
        let winner = &self.tested_list()[i];
        if self.tested_simple {
            self.use_custom = true;
            self.custom1 = winner.dns1.to_string();
            self.custom2 = winner.dns2.to_string();
        } else {
            self.server_idx = i;
            self.use_custom = false;
        }
        let r = self.results[i].as_ref().expect("best has a result");
        let mut msg = format!("Best: {}, {}, {}% loss", winner.name, msg_speed, r.loss_pct());
        if let Some(site) = &r.site {
            msg.push_str(&format!(", answered by {site}"));
        }
        msg.push_str(". Press Apply to use it.");
        self.set_status(&msg, true);
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
                    self.refresh_current();
                }
                Msg::Current(adapter, dns, provider) => {
                    if adapter == self.adapter_name() {
                        self.current = dns;
                        self.provider = provider;
                    }
                }
                Msg::TestResult(gen, i, r) => {
                    if gen != self.test_gen {
                        continue;
                    }
                    if let Some(slot) = self.results.get_mut(i) {
                        *slot = Some(r);
                        self.test_got += 1;
                        if self.test_got == self.results.len() {
                            self.finish_test();
                        } else {
                            self.status = format!("Testing DNS servers… {}/{}", self.test_got, self.results.len());
                        }
                    }
                }
            }
        }
    }
}

fn status_msg(r: Result<String, String>) -> Msg {
    match r {
        Ok(m) => Msg::Status(m, true),
        Err(e) => {
            let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(&e).trim().to_string();
            Msg::Status(first, false)
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        if self.tray_ready && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            tray::hide_window();
            tray::set_pending("Still running here. Right-click for quick switching.".into());
        }

        // Detect a Simple/Advanced switch to drive a fade + slide transition.
        if self.mode != self.prev_mode {
            self.prev_mode = self.mode;
            self.clear_results();
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
                GOOD
            } else {
                BAD
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
                let before = self.adapter_idx;
                egui::ComboBox::from_id_source("adapter")
                    .selected_text(self.adapter_name())
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (i, a) in self.adapters.iter().enumerate() {
                            ui.selectable_value(&mut self.adapter_idx, i, a);
                        }
                    });
                if self.adapter_idx != before {
                    self.sync_tray_adapter();
                    self.current.clear();
                    self.refresh_current();
                }
                if ui.button("Refresh").on_hover_text("Refresh adapters").clicked() {
                    self.adapters = sys::adapters();
                    if self.adapter_idx >= self.adapters.len() {
                        self.adapter_idx = 0;
                    }
                    self.refresh_current();
                }
            });
            if !self.current.is_empty() {
                ui.label(egui::RichText::new(format!("DNS in use: {}", self.current)).weak());
            }
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
            if let (Some(image), Some(path)) = (img, self.shot_path.as_ref()) {
                save_screenshot(&image, path);
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
            // leave room for the two button rows below
            .max_height((ui.available_height() - 120.0).max(110.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let shown = self.tested_simple;
                if let Some(p1) = self.provider.first().cloned() {
                    let i = QUICK.len();
                    let metric = if shown && self.tested_provider { row_metric(self.results.get(i), self.best == Some(i), self.testing) } else { None };
                    let p2 = self.provider.get(1).cloned().unwrap_or_default();
                    if dns_row(ui, self.use_provider, "Your provider — automatic", &p1, &p2, metric.as_deref()) {
                        self.use_provider = true;
                        self.use_custom = false;
                    }
                    ui.add_space(6.0);
                }
                for (i, q) in QUICK.iter().enumerate() {
                    let selected = !self.use_provider && self.use_custom && self.custom1 == q.dns1;
                    let metric = if shown { row_metric(self.results.get(i), self.best == Some(i), self.testing) } else { None };
                    if dns_row(ui, selected, q.name, q.dns1, q.dns2, metric.as_deref()) {
                        pending = Some((q.dns1.to_string(), q.dns2.to_string()));
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some((d1, d2)) = pending {
            self.use_provider = false;
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
                self.start_test(true);
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
                    self.clear_results();
                }
            }
            ui.separator();
            if ui.checkbox(&mut self.ipv6, "IPv6").changed() {
                self.server_idx = 0;
                self.clear_results();
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
            .max_height((ui.available_height() - 64.0).max(120.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // reserve room for the scrollbar so the Speed column isn't clipped
                ui.set_max_width(ui.available_width() - 14.0);
                egui::Grid::new("servers")
                    .striped(true)
                    .num_columns(10)
                    .min_col_width(0.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.add_sized([22.0, 16.0], egui::Label::new(""));
                        ui.strong("DNS server");
                        ui.strong("Primary / Secondary");
                        ui.strong("Median").on_hover_text(format!(
                            "Middle value of {} cached lookups of {}",
                            probe::SAMPLES,
                            probe::WARM_DOMAIN
                        ));
                        ui.strong("p95").on_hover_text("Slowest of the cached lookups (95th percentile)");
                        ui.strong("Jitter").on_hover_text("Average change between consecutive lookups");
                        ui.strong("Loss").on_hover_text(format!(
                            "Lookups with no reply within {} ms",
                            probe::TIMEOUT_MS
                        ));
                        ui.strong("Cold").on_hover_text(
                            "A random name the resolver has never seen, so it must ask the authoritative servers",
                        );
                        ui.strong("CDN").on_hover_text(format!(
                            "Time to connect to the servers this resolver picks for big CDNs ({}). A resolver that answers fast but sends you to another region or country shows a high number here. Ranking uses lookup time plus this.",
                            probe::CDN_HOSTS.join(", ")
                        ));
                        ui.strong("Site").on_hover_text(
                            "Which anycast location answered (CHAOS TXT id.server), when the resolver reports it",
                        );
                        ui.end_row();
                        if !self.ipv6 && !self.provider.is_empty() {
                            {
                                let i = self.current_list().len();
                                if ui.add_sized([22.0, 18.0], egui::RadioButton::new(self.use_provider, "")).clicked() {
                                    self.use_provider = true;
                                    self.use_custom = false;
                                }
                                let shown = !self.tested_simple && self.tested_provider;
                                let mut name = egui::RichText::new("Your provider (automatic)");
                                if shown && self.best == Some(i) {
                                    name = name.color(GOOD).strong();
                                }
                                ui.label(name).on_hover_text("The DNS your router or internet provider hands out. Choosing it switches the adapter back to automatic.");
                                ui.label(egui::RichText::new(self.provider.join(" / ")).monospace().size(12.0));
                                let r = if shown { self.results.get(i).and_then(|r| r.as_ref()) } else { None };
                                probe_cells(ui, r, self.testing && shown);
                                ui.end_row();
                            }
                        }
                        let best = self.best;
                        let testing = self.testing;
                        let shown = !self.tested_simple;
                        let list = self.current_list();
                        let mut pick: Option<usize> = None;
                        for (i, p) in list.iter().enumerate() {
                            let sel = !self.use_provider && !self.use_custom && self.server_idx == i;
                            if ui.add_sized([22.0, 18.0], egui::RadioButton::new(sel, "")).clicked() {
                                pick = Some(i);
                            }
                            let mut name = egui::RichText::new(p.name);
                            if shown && best == Some(i) {
                                name = name.color(GOOD).strong();
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
                            let result = if shown { self.results.get(i).and_then(|r| r.as_ref()) } else { None };
                            probe_cells(ui, result, testing && shown);
                            ui.end_row();
                        }
                        if let Some(i) = pick {
                            self.server_idx = i;
                            self.use_custom = false;
                            self.use_provider = false;
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
            if ui.add_enabled(!self.testing, egui::Button::new("Find fastest").min_size(egui::vec2(120.0, 36.0))).clicked() {
                self.start_test(false);
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

/// The seven measurement cells of an advanced-mode row.
fn probe_cells(ui: &mut egui::Ui, r: Option<&probe::Probe>, pending: bool) {
    let Some(r) = r else {
        let mark = if pending { "…" } else { "–" };
        for _ in 0..7 {
            ui.weak(mark);
        }
        return;
    };
    ms_cell(ui, r.median());
    ms_cell(ui, r.p95());
    ms_cell(ui, r.jitter());
    let loss = r.loss_pct();
    let text = egui::RichText::new(format!("{loss}%")).monospace();
    ui.label(if loss > 20 { text.color(BAD) } else { text });
    ms_cell(ui, r.cold);
    let detail: Vec<String> = r
        .cdn_detail
        .iter()
        .map(|(h, ip, ms)| format!("{h} → {ip}: {}", ms.map(|m| format!("{m} ms")).unwrap_or("no connection".into())))
        .collect();
    let cdn = match r.cdn {
        Some(v) => {
            let t = egui::RichText::new(format!("{v} ms")).monospace();
            ui.label(if v >= 80 { t.color(BAD) } else { t })
        }
        None => ui.weak("–"),
    };
    if !detail.is_empty() {
        cdn.on_hover_text(detail.join("\n"));
    }
    match &r.site {
        Some(s) => {
            let short: String = s.chars().take(22).collect();
            let label = if short.len() < s.len() { format!("{short}…") } else { short };
            ui.label(egui::RichText::new(label).monospace().size(12.0)).on_hover_text(s)
        }
        None => ui.weak("–"),
    };
}

/// Right-hand text of a simple-mode row: lookup time and time to sites.
fn row_metric(r: Option<&Option<probe::Probe>>, best: bool, testing: bool) -> Option<String> {
    match r {
        Some(Some(r)) => Some(match (r.median(), r.cdn) {
            (Some(ms), cdn) => {
                let mut t = format!("{ms} ms");
                if let Some(c) = cdn {
                    t.push_str(&format!(" · sites {c} ms"));
                }
                if best {
                    t.push_str("  best");
                }
                t
            }
            (None, _) => "no reply".to_string(),
        }),
        Some(None) if testing => Some("testing…".to_string()),
        _ => None,
    }
}

/// Milliseconds in a grid cell, or a dash when there is no value.
fn ms_cell(ui: &mut egui::Ui, ms: Option<u32>) {
    match ms {
        Some(v) => ui.label(egui::RichText::new(format!("{v} ms")).monospace()),
        None => ui.weak("–"),
    };
}

/// A left-aligned, full-width selectable DNS row (two lines), clipped-safe.
fn dns_row(ui: &mut egui::Ui, selected: bool, name: &str, dns1: &str, dns2: &str, metric: Option<&str>) -> bool {
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
    p.rect_filled(rect, Rounding::same(8.0_f32), bg);
    if selected {
        p.rect_stroke(rect, Rounding::same(8.0_f32), Stroke::new(1.5_f32, visuals.selection.stroke.color));
    } else {
        p.rect_stroke(rect, Rounding::same(8.0_f32), Stroke::new(1.0_f32, visuals.widgets.inactive.bg_stroke.color));
    }
    let text_col = if selected { visuals.selection.stroke.color } else { visuals.text_color() };
    let weak = if selected { visuals.selection.stroke.color.gamma_multiply(0.75) } else { visuals.weak_text_color() };
    let x = rect.left() + 12.0;
    p.text(Pos2::new(x, rect.top() + 13.0), Align2::LEFT_CENTER, name, FontId::proportional(14.5), text_col);
    let ips = format!("{}  •  {}", dns1, if dns2.is_empty() { "—" } else { dns2 });
    p.text(Pos2::new(x, rect.bottom() - 13.0), Align2::LEFT_CENTER, ips, FontId::monospace(12.5), weak);
    if let Some(m) = metric {
        let col = if selected { text_col } else if m.ends_with("best") { GOOD } else { weak };
        p.text(Pos2::new(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, m, FontId::monospace(13.0), col);
    }
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

impl App {
    /// Notification-area icon. Failure only disables hide-to-tray.
    fn start_tray(&mut self, cc: &eframe::CreationContext<'_>) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = cc.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                tray::WINDOW.store(w.hwnd.get(), std::sync::atomic::Ordering::Relaxed);
            }
        }
        let ctx = cc.egui_ctx.clone();
        match tray::install(self.tray_adapter.clone(), move || ctx.request_repaint()) {
            Ok(()) => self.tray_ready = true,
            Err(e) => self.set_status(&format!("Tray icon unavailable: {e}"), false),
        }
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
    if args.iter().any(|a| a == "--service") {
        if let Err(e) = service::run() {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // Used by the installer and uninstaller, running as the signed-in user.
    for (flag, on) in [("--set-autostart", true), ("--clear-autostart", false)] {
        if args.iter().any(|a| a == flag) {
            std::process::exit(if tray::set_autostart(on).is_ok() { 0 } else { 1 });
        }
    }
    let start_hidden = args.iter().any(|a| a == "--tray");
    let mut shot_path = None;
    for i in 0..args.len() {
        if args[i] == "--screenshot" {
            shot_path = args.get(i + 1).cloned();
        }
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([880.0, 660.0])
        .with_min_inner_size([520.0, 480.0])
        .with_title("DNS Switcher");
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }

    if start_hidden {
        viewport = viewport.with_visible(false);
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
        Box::new(move |cc| {
            let mut app = App::new(cc, shot.clone());
            app.refresh_current();
            app.sync_tray_adapter();
            if shot.is_none() {
                app.start_tray(cc);
            }
            Ok(Box::new(app))
        }),
    )
}
