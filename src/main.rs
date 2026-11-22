#![windows_subsystem = "windows"]

mod config;
mod modules;
mod state;

use config::AppConfig;
use eframe::egui;
use modules::process_list::{
    enable_privileges, get_process_path, get_system_ram_mb, kill_process, list_processes,
    pin_to_last_two_cores, resume_process, set_eco_qos, set_priority, set_timer_resolution_high,
    suspend_process, trim_process, ProcInfo,
};
use modules::utilities::{
    build_app_index, capture_screenshot_png, copy_to_clipboard, launch_path, try_quick_calc,
    IndexedItem,
};
use state::LiveState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(PartialEq)]
enum Tab {
    Runtime,
    TaskManager,
    Utilities,
    Network,
    Maintenance,
    Logs,
}

struct WinCoreApp {
    config: AppConfig,
    state: Arc<LiveState>,
    tab: Tab,
    procs: Vec<ProcInfo>,
    search_proc: String,
    last_refresh: Instant,
    new_exclusion: String,
    new_domain: String,
    launcher_query: String,
    indexed_items: Vec<IndexedItem>,
    service_statuses: Vec<(String, String)>,
    _tray: Option<TrayIcon>,
}

fn make_tray_icon() -> Option<Icon> {
    let mut rgba = vec![0u8; 16 * 16 * 4];
    for y in 0..16 {
        for x in 0..16 {
            let idx = (y * 16 + x) * 4;
            let border = x == 0 || y == 0 || x == 15 || y == 15;
            rgba[idx] = if border { 30 } else { 0 };
            rgba[idx + 1] = if border { 144 } else { 180 };
            rgba[idx + 2] = 255;
            rgba[idx + 3] = 255;
        }
    }
    Icon::from_rgba(rgba, 16, 16).ok()
}

const GUARDED_SERVICES: &[&str] = &["WSearch", "SysMain", "Spooler", "wuauserv", "DiagTrack"];

impl WinCoreApp {
    fn new(cc: &eframe::CreationContext<'_>, config: AppConfig, state: Arc<LiveState>) -> Self {
        let mut tray = None;
        if let Some(icon) = make_tray_icon() {
            let menu = Menu::new();
            let show_item = MenuItem::new("Buka WinCore", true, None);
            let exit_item = MenuItem::new("Keluar Total", true, None);
            let show_id = show_item.id().clone();
            let exit_id = exit_item.id().clone();
            let _ = menu.append(&show_item);
            let _ = menu.append(&exit_item);

            if let Ok(t) = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("WinCore v0.2 (Admin)")
                .with_icon(icon)
                .build()
            {
                tray = Some(t);
                let ctx = cc.egui_ctx.clone();
                let st = state.clone();
                thread::spawn(move || loop {
                    thread::sleep(Duration::from_millis(120));
                    if let Ok(ev) = MenuEvent::receiver().try_recv() {
                        if ev.id == show_id {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                            ctx.request_repaint();
                        } else if ev.id == exit_id {
                            if st.dns_adblock_on.load(Ordering::Relaxed) {
                                let _ = modules::dns_adblock::set_system_dns_local(false);
                            }
                            st.force_exit.store(true, Ordering::Relaxed);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            ctx.request_repaint();
                            break;
                        }
                    }
                    if let Ok(TrayIconEvent::DoubleClick { .. }) =
                        TrayIconEvent::receiver().try_recv()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    }
                });
            }
        }

        let indexed_items = build_app_index(&config.extra_index_dir);
        let service_statuses = GUARDED_SERVICES
            .iter()
            .map(|&s| (s.to_string(), modules::maintenance::get_service_status(s)))
            .collect();

        Self {
            config,
            state,
            tab: Tab::Runtime,
            procs: list_processes(),
            search_proc: String::new(),
            last_refresh: Instant::now(),
            new_exclusion: String::new(),
            new_domain: String::new(),
            launcher_query: String::new(),
            indexed_items,
            service_statuses,
            _tray: tray,
        }
    }

    fn refresh_services(&mut self) {
        self.service_statuses = GUARDED_SERVICES
            .iter()
            .map(|&s| (s.to_string(), modules::maintenance::get_service_status(s)))
            .collect();
    }
}

impl eframe::App for WinCoreApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Minimize ke System Tray saat tombol X diklik
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.config.minimize_to_tray_on_close && !self.state.force_exit.load(Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                return;
            } else if self.state.dns_adblock_on.load(Ordering::Relaxed) {
                let _ = modules::dns_adblock::set_system_dns_local(false);
            }
        }

        ctx.request_repaint_after(Duration::from_secs(2));

        egui::TopBottomPanel::top("top_nav").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("WinCore v0.2");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Runtime, "1. Runtime");
                ui.selectable_value(&mut self.tab, Tab::TaskManager, "2. Task Manager");
                ui.selectable_value(&mut self.tab, Tab::Utilities, "3. Utilities");
                ui.selectable_value(&mut self.tab, Tab::Network, "4. Network & DNS");
                ui.selectable_value(&mut self.tab, Tab::Maintenance, "5. Defender & Tweaks");
                ui.selectable_value(&mut self.tab, Tab::Logs, "6. Logs");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Runtime => {
                let (used, total) = get_system_ram_mb();
                ui.heading(format!("RAM Sistem: {} MB / {} MB", used, total));
                ui.separator();

                let mut cpu_on = self.config.cpu_governor_enabled;
                if ui.checkbox(&mut cpu_on, "CPU Governor (ProBalance + EcoQoS + I/O Low + Affinity Core 2-3)").changed() {
                    self.config.cpu_governor_enabled = cpu_on;
                    self.state.cpu_governor_on.store(cpu_on, Ordering::Relaxed);
                    self.config.save();
                }

                let mut fg_on = self.config.foreground_boost_enabled;
                if ui.checkbox(&mut fg_on, "Dynamic Foreground Boost (Prioritas jendela aktif)").changed() {
                    self.config.foreground_boost_enabled = fg_on;
                    self.state.foreground_boost_on.store(fg_on, Ordering::Relaxed);
                    self.config.save();
                }

                let mut ram_on = self.config.ram_trimmer_enabled;
                if ui.checkbox(&mut ram_on, "Auto RAM & Standby List Trimmer").changed() {
                    self.config.ram_trimmer_enabled = ram_on;
                    self.state.ram_trimmer_on.store(ram_on, Ordering::Relaxed);
                    self.config.save();
                }

                let mut timer_on = self.config.high_res_timer_enabled;
                if ui.checkbox(&mut timer_on, "High-Resolution Timer 0.5ms (Anti micro-stutter video/game)").changed() {
                    self.config.high_res_timer_enabled = timer_on;
                    let ok = set_timer_resolution_high(timer_on);
                    self.state.add_log(format!("Timer Resolution 0.5ms -> {}: {}", timer_on, ok));
                    self.config.save();
                }

                let mut tray_on = self.config.minimize_to_tray_on_close;
                if ui.checkbox(&mut tray_on, "Minimize ke System Tray saat tombol X diklik").changed() {
                    self.config.minimize_to_tray_on_close = tray_on;
                    self.config.save();
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Trim RAM & Purge Standby Sekarang").clicked() {
                        modules::ram_trimmer::trim_all_now(&self.state);
                    }
                    if ui.button("Sembunyikan ke Tray").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                });

                ui.separator();
                let throttled = *self.state.throttled_count.lock().unwrap();
                let runs = self.state.total_trim_runs.load(Ordering::Relaxed);
                let dns_blk = self.state.dns_blocked_count.load(Ordering::Relaxed);
                ui.label(format!("â€¢ Proses berat sedang di-throttle: {}", throttled));
                ui.label(format!("â€¢ Total eksekusi RAM Trim sesi ini: {}", runs));
                ui.label(format!("â€¢ Total domain iklan/telemetri diblokir: {}", dns_blk));
            }

            Tab::TaskManager => {
                if self.last_refresh.elapsed() >= Duration::from_secs(2) {
                    self.procs = list_processes();
                    self.last_refresh = Instant::now();
                }

                ui.horizontal(|ui| {
                    ui.label("Cari proses:");
                    ui.text_edit_singleline(&mut self.search_proc);
                    if ui.button("Refresh").clicked() {
                        self.procs = list_processes();
                    }
                });
                ui.separator();

                let query = self.search_proc.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for p in &self.procs {
                        if !query.is_empty() && !p.name.to_lowercase().contains(&query) {
                            continue;
                        }
                        let is_frozen = self
                            .state
                            .frozen_pids
                            .lock()
                            .map(|f| f.contains(&p.pid))
                            .unwrap_or(false);

                        ui.horizontal(|ui| {
                            let tag = if is_frozen { "[FROZEN] " } else { "" };
                            ui.label(format!(
                                "{}{:<22} | PID {:<6} | {:>6.1} MB",
                                tag, p.name, p.pid, p.ram_mb
                            ));
                            if ui.small_button("Trim").clicked() {
                                let _ = trim_process(p.pid);
                            }
                            if ui.small_button(if is_frozen { "Resume" } else { "Freeze" }).clicked() {
                                if is_frozen {
                                    if resume_process(p.pid) {
                                        if let Ok(mut f) = self.state.frozen_pids.lock() {
                                            f.remove(&p.pid);
                                        }
                                        self.state.add_log(format!("Resumed {} ({})", p.name, p.pid));
                                    }
                                } else if suspend_process(p.pid) {
                                    if let Ok(mut f) = self.state.frozen_pids.lock() {
                                        f.insert(p.pid);
                                    }
                                    self.state.add_log(format!("Frozen {} ({})", p.name, p.pid));
                                }
                            }
                            if ui.small_button("Eco").clicked() {
                                let _ = set_priority(p.pid, "idle");
                                let _ = set_eco_qos(p.pid, true);
                                let _ = pin_to_last_two_cores(p.pid);
                                self.state.add_log(format!("EcoQoS + Core 2-3: {}", p.name));
                            }
                            if ui.small_button("Block Net").clicked() {
                                if let Some(exe) = get_process_path(p.pid) {
                                    match modules::maintenance::block_app_firewall(&p.name, &exe, true) {
                                        Ok(_) => self.state.add_log(format!("Blokir internet: {}", p.name)),
                                        Err(e) => self.state.add_log(format!("Error Firewall: {}", e)),
                                    }
                                }
                            }
                            if ui.small_button("Kill").clicked() {
                                if kill_process(p.pid) {
                                    self.state.add_log(format!("Killed {} ({})", p.name, p.pid));
                                }
                            }
                        });
                    }
                });
            }

            Tab::Utilities => {
                ui.heading("App Launcher, File Search & Quick Calculator");
                ui.horizontal(|ui| {
                    ui.label("Cari / Hitung:");
                    ui.text_edit_singleline(&mut self.launcher_query);
                    if ui.button("Ambil Screenshot (PNG)").clicked() {
                        match capture_screenshot_png() {
                            Ok(p) => self.state.add_log(format!("Screenshot disimpan: {}", p)),
                            Err(e) => self.state.add_log(format!("Gagal screenshot: {}", e)),
                        }
                    }
                });

                if let Some(res) = try_quick_calc(&self.launcher_query) {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, format!("Hasil Kalkulator: {}", res));
                }

                ui.horizontal(|ui| {
                    ui.label("Folder ekstra untuk indeks:");
                    ui.text_edit_singleline(&mut self.config.extra_index_dir);
                    if ui.button("Re-Index").clicked() {
                        self.config.save();
                        self.indexed_items = build_app_index(&self.config.extra_index_dir);
                        self.state.add_log(format!("Terindeks {} item.", self.indexed_items.len()));
                    }
                });

                let q = self.launcher_query.to_lowercase();
                egui::ScrollArea::vertical().id_salt("launcher_scroll").max_height(180.0).show(ui, |ui| {
                    let mut shown = 0;
                    for item in &self.indexed_items {
                        if !q.is_empty() && !item.name.to_lowercase().contains(&q) {
                            continue;
                        }
                        if ui.button(format!("â–¶ {}", item.name)).clicked() {
                            launch_path(&item.path);
                        }
                        shown += 1;
                        if shown >= 40 {
                            break;
                        }
                    }
                });

                ui.separator();
                let mut clip_on = self.config.clipboard_enabled;
                if ui.checkbox(&mut clip_on, "Clipboard History Watcher (klik item untuk copy ulang)").changed() {
                    self.config.clipboard_enabled = clip_on;
                    self.state.clipboard_on.store(clip_on, Ordering::Relaxed);
                    self.config.save();
                }

                egui::ScrollArea::vertical().id_salt("clip_scroll").show(ui, |ui| {
                    if let Ok(hist) = self.state.clipboard_history.lock() {
                        for entry in hist.iter() {
                            let preview: String = entry.chars().take(90).collect();
                            if ui.button(preview.replace('\n', " ")).clicked() {
                                let _ = copy_to_clipboard(entry);
                            }
                        }
                    }
                });
            }

            Tab::Network => {
                ui.heading("Local DNS Adblock & Telemetry Sinkhole (127.0.0.1:53)");
                let mut dns_on = self.config.dns_adblock_enabled;
                if ui.checkbox(&mut dns_on, "Aktifkan Local DNS Adblock (Otomatis set/reset DNS Windows)").changed() {
                    self.config.dns_adblock_enabled = dns_on;
                    self.state.dns_adblock_on.store(dns_on, Ordering::Relaxed);
                    self.config.save();
                    match modules::dns_adblock::set_system_dns_local(dns_on) {
                        Ok(_) => self.state.add_log(format!("DNS Windows -> {}", if dns_on { "127.0.0.1" } else { "DHCP Normal" })),
                        Err(e) => self.state.add_log(format!("Error set DNS: {}", e)),
                    }
                }
                ui.label(format!(
                    "Domain diblokir sesi ini: {}",
                    self.state.dns_blocked_count.load(Ordering::Relaxed)
                ));

                ui.horizontal(|ui| {
                    ui.label("Tambah domain blokir:");
                    ui.text_edit_singleline(&mut self.new_domain);
                    if ui.button("Tambah").clicked() {
                        let d = self.new_domain.trim().to_lowercase();
                        if !d.is_empty() {
                            if let Ok(mut set) = self.state.custom_domains.lock() {
                                set.insert(d.clone());
                            }
                            self.config.custom_blocked_domains.push(d.clone());
                            self.config.save();
                            self.state.add_log(format!("Domain ditambahkan ke blocklist: {}", d));
                            self.new_domain.clear();
                        }
                    }
                });

                ui.separator();
                ui.heading("TCP/IP Latency Tweak");
                if ui.button("Matikan Network Throttling Index (Gaming & Browsing)").clicked() {
                    match modules::maintenance::apply_network_latency_tweak() {
                        Ok(_) => self.state.add_log("NetworkThrottlingIndex dimatikan.".into()),
                        Err(e) => self.state.add_log(format!("Error: {}", e)),
                    }
                }
            }

            Tab::Maintenance => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("1. Windows Defender Tuner & Exclusions");
                    ui.add(
                        egui::Slider::new(&mut self.config.defender_cpu_limit, 5..=50)
                            .text("ScanAvgCPULoadFactor (%)"),
                    );
                    ui.checkbox(&mut self.config.defender_low_priority, "EnableLowCpuPriority");
                    if ui.button("Terapkan Setting Defender").clicked() {
                        self.config.save();
                        match modules::defender::apply_tuning(
                            self.config.defender_cpu_limit,
                            self.config.defender_low_priority,
                        ) {
                            Ok(_) => self.state.add_log("Setting Defender berhasil diterapkan.".into()),
                            Err(e) => self.state.add_log(format!("Error Defender: {}", e)),
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("Folder Exclusion:");
                        ui.text_edit_singleline(&mut self.new_exclusion);
                        if ui.button("Tambah").clicked() {
                            match modules::defender::add_exclusion(&self.new_exclusion) {
                                Ok(_) => {
                                    self.state.add_log(format!("Exclusion ditambah: {}", self.new_exclusion));
                                    self.config.defender_exclusions.push(self.new_exclusion.clone());
                                    self.config.save();
                                    self.new_exclusion.clear();
                                }
                                Err(e) => self.state.add_log(format!("Gagal exclusion: {}", e)),
                            }
                        }
                    });

                    ui.separator();
                    ui.heading("2. Service Guard (ON / OFF Layanan Windows)");
                    if ui.button("Refresh Status Service").clicked() {
                        self.refresh_services();
                    }
                    let list = self.service_statuses.clone();
                    for (name, status) in list {
                        ui.horizontal(|ui| {
                            ui.label(format!("{:<14} : {}", name, status));
                            if ui.small_button("Start").clicked() {
                                let _ = modules::maintenance::set_service_running(&name, true);
                                self.refresh_services();
                            }
                            if ui.small_button("Stop & Disable").clicked() {
                                let _ = modules::maintenance::set_service_running(&name, false);
                                self.refresh_services();
                            }
                        });
                    }

                    ui.separator();
                    ui.heading("3. System Tweaks, Power Plan & Cleaner");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Autostart Admin (Tanpa UAC)").clicked() {
                            match modules::maintenance::set_autostart_admin(true) {
                                Ok(_) => self.state.add_log("Autostart Task Scheduler dibuat.".into()),
                                Err(e) => self.state.add_log(format!("Error Autostart: {}", e)),
                            }
                        }
                        if ui.button("Hapus Autostart").clicked() {
                            let _ = modules::maintenance::set_autostart_admin(false);
                            self.state.add_log("Autostart dihapus.".into());
                        }
                        if ui.button("Matikan 8dot3 Short Names").clicked() {
                            let _ = modules::maintenance::disable_8dot3_names();
                            self.state.add_log("8dot3name dimatikan.".into());
                        }
                        if ui.button("Memory Compression OFF").clicked() {
                            let _ = modules::maintenance::toggle_memory_compression(false);
                            self.state.add_log("Memory Compression dimatikan.".into());
                        }
                        if ui.button("Memory Compression ON").clicked() {
                            let _ = modules::maintenance::toggle_memory_compression(true);
                            self.state.add_log("Memory Compression dinyalakan.".into());
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Power Plan:");
                        if ui.button("Power Saver").clicked() {
                            let _ = modules::maintenance::set_power_plan("saver");
                        }
                        if ui.button("Balanced").clicked() {
                            let _ = modules::maintenance::set_power_plan("balanced");
                        }
                        if ui.button("High Performance").clicked() {
                            let _ = modules::maintenance::set_power_plan("high");
                        }
                    });

                    if ui.button("1-Klik Bersihkan File Temp & Cache").clicked() {
                        let (files, mb) = modules::maintenance::clean_temp_files();
                        self.state.add_log(format!("Cleaner: {} file dihapus ({} MB).", files, mb));
                    }
                });
            }

            Tab::Logs => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Ok(logs) = self.state.logs.lock() {
                        for line in logs.iter().rev() {
                            ui.label(line);
                        }
                    }
                });
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    enable_privileges();
    let config = AppConfig::load();
    let state = Arc::new(LiveState::new(
        config.cpu_governor_enabled,
        config.foreground_boost_enabled,
        config.ram_trimmer_enabled,
        config.dns_adblock_enabled,
        config.clipboard_enabled,
        &config.custom_blocked_domains,
    ));

    if config.high_res_timer_enabled {
        let _ = set_timer_resolution_high(true);
    }

    modules::cpu_governor::spawn(state.clone());
    modules::ram_trimmer::spawn(
        state.clone(),
        config.ram_trim_interval_sec,
        config.standby_purge_threshold_mb,
    );
    modules::dns_adblock::spawn(state.clone());
    modules::utilities::spawn_clipboard_watcher(state.clone());

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([920.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "WinCore v0.2",
        options,
        Box::new(move |cc| Ok(Box::new(WinCoreApp::new(cc, config, state)))),
    )
}