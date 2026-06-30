//! Main UI state and rendering for all panels.

use ca_actions::{run_archive, CleanOutcome};
use ca_core::{
    archive::ArchivePlan,
    classifier::{classify, RiskLevel},
    rules::{CleaningTier, RuleSet},
    scanner::{format_bytes, scan_all, ScanResult},
};
use eframe::egui::{self, Color32, RichText};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use crate::tray::init_tray;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedirectionType {
    EnvVar,
    Junction,
}

struct RedirectionOption {
    name: &'static str,
    redirection_type: RedirectionType,
    env_vars: &'static [&'static str],
    c_path_var: &'static str,
    c_path_sub: &'static str,
    sub_folder: &'static str,
    process_names: &'static [&'static str],
    description: &'static str,
}

const REDIRECTION_OPTIONS: &[RedirectionOption] = &[
    RedirectionOption {
        name: "User Temp Files",
        redirection_type: RedirectionType::EnvVar,
        env_vars: &["TEMP", "TMP"],
        c_path_var: "",
        c_path_sub: "",
        sub_folder: "Temp",
        process_names: &[],
        description: "Redirects standard user temporary directories (%TEMP% / %TMP%) to USB",
    },
    RedirectionOption {
        name: "Python Pip Cache",
        redirection_type: RedirectionType::EnvVar,
        env_vars: &["PIP_CACHE_DIR"],
        c_path_var: "",
        c_path_sub: "",
        sub_folder: "pip_cache",
        process_names: &["python.exe", "pip.exe"],
        description: "Redirects pip packages download cache (PIP_CACHE_DIR) to USB",
    },
    RedirectionOption {
        name: "HuggingFace ML Models",
        redirection_type: RedirectionType::EnvVar,
        env_vars: &["HF_HOME"],
        c_path_var: "",
        c_path_sub: "",
        sub_folder: "huggingface",
        process_names: &["python.exe"],
        description: "Redirects large ML models and datasets downloaded via HuggingFace to USB",
    },
    RedirectionOption {
        name: "Rust Cargo & Rustup",
        redirection_type: RedirectionType::EnvVar,
        env_vars: &["CARGO_HOME", "RUSTUP_HOME"],
        c_path_var: "",
        c_path_sub: "",
        sub_folder: "rust_toolchain",
        process_names: &["cargo.exe", "rustc.exe"],
        description: "Redirects downloaded Rust toolchains and crate registries cache to USB",
    },
    RedirectionOption {
        name: "VS Code Cached Data",
        redirection_type: RedirectionType::Junction,
        env_vars: &[],
        c_path_var: "APPDATA",
        c_path_sub: "Code/CachedData",
        sub_folder: "vscode_cached_data",
        process_names: &["code.exe"],
        description: "Redirects VS Code caches to USB (reduces C: drive storage bloat)",
    },
    RedirectionOption {
        name: "CapCut Pre-Render Cache",
        redirection_type: RedirectionType::Junction,
        env_vars: &[],
        c_path_var: "LOCALAPPDATA",
        c_path_sub: "CapCut/segmentPrerenderCache",
        sub_folder: "capcut_prerender",
        process_names: &["CapCut.exe"],
        description: "Redirects heavy video pre-render caches of CapCut to USB",
    },
    RedirectionOption {
        name: "Spotify Local Cache",
        redirection_type: RedirectionType::Junction,
        env_vars: &[],
        c_path_var: "LOCALAPPDATA",
        c_path_sub: "Spotify/Storage",
        sub_folder: "spotify_cache",
        process_names: &["Spotify.exe"],
        description: "Redirects offline/downloaded songs cache of Spotify to USB",
    },
    RedirectionOption {
        name: "Discord Local Cache",
        redirection_type: RedirectionType::Junction,
        env_vars: &[],
        c_path_var: "APPDATA",
        c_path_sub: "discord/Cache",
        sub_folder: "discord_cache",
        process_names: &["Discord.exe"],
        description: "Redirects cache files from Discord to USB",
    },
];

fn resolve_c_path(opt: &RedirectionOption) -> Option<PathBuf> {
    if opt.redirection_type != RedirectionType::Junction {
        return None;
    }
    if let Ok(base) = std::env::var(opt.c_path_var) {
        Some(PathBuf::from(base).join(opt.c_path_sub))
    } else {
        None
    }
}

fn resolve_env_var_default_c_path(name: &str) -> Option<PathBuf> {
    let userprofile = std::env::var("USERPROFILE").ok()?;
    let profile_path = PathBuf::from(userprofile);
    match name {
        "Python Pip Cache" => Some(profile_path.join("AppData/Local/pip/cache")),
        "HuggingFace ML Models" => Some(profile_path.join(".cache/huggingface")),
        "Rust Cargo & Rustup" => Some(profile_path.join(".cargo")),
        _ => None,
    }
}

/// Which tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Scan,
    Archive,
    Duplicates,
    DiskMap,
    Portable,
    #[cfg(feature = "ai")]
    AskAi,
}

#[cfg(feature = "ai")]
use std::sync::mpsc::Sender;

#[cfg(feature = "ai")]
struct AiWorker {
    tx: Sender<AiRequest>,
    rx: Receiver<AiResponse>,
}

#[cfg(feature = "ai")]
enum AiRequest {
    Ask(String),
}

#[cfg(feature = "ai")]
enum AiResponse {
    Loaded,
    Answer(String),
    Error(String),
}

// ─── Application State ──────────────────────────────────────────────────────

pub struct App {
    settings: ca_core::Settings,
    rules: RuleSet,
    results: Vec<ScanResult>,
    scores: Vec<ca_core::classifier::RiskScore>,
    archive_plan: ArchivePlan,

    // Tabs
    selected_tab: Tab,

    // Scan state
    scanning: bool,
    scan_rx: Option<Receiver<Vec<ScanResult>>>,
    last_scan_time: Option<std::time::Instant>,

    // System Tray & Notifications
    #[allow(dead_code)]
    tray_icon: Option<tray_icon::TrayIcon>,
    show_window: bool,
    allow_close: bool,
    scheduled_scanning: bool,

    // Clean state
    cleaning_name: Option<String>,
    clean_confirming: bool,
    last_clean: Option<CleanOutcome>,

    // Archive state
    archive_confirming: bool,
    archive_result: Option<String>,
    archive_error: Option<String>,

    // Settings
    external_drive: String,
    exclusion_input: String,

    // Portable Mode (Auto-Routing)
    is_admin_user: bool,
    portable_drive: Option<PathBuf>,
    active_redirections: Vec<bool>,
    lock_warning_msg: Option<String>,
    is_migrating: bool,
    migration_rx: Option<Receiver<std::io::Result<()>>>,

    // Recycle Bin (Undo Cleaner)
    recycle_root: PathBuf,
    clean_sessions: Vec<ca_actions::CleanSession>,

    // Duplicates state
    duplicate_groups: Vec<ca_core::DuplicateGroup>,
    dup_scanning: bool,
    dup_rx: Option<Receiver<Vec<ca_core::DuplicateGroup>>>,
    duplicate_selections: Vec<Vec<bool>>,

    // Disk Map state
    disk_scan_drive: String,
    disk_scan_active: bool,
    disk_scan_rx: Option<Receiver<Option<ca_core::DiskNode>>>,
    disk_tree_root: Option<ca_core::DiskNode>,
    disk_tree_active: Option<ca_core::DiskNode>,
    disk_history: Vec<ca_core::DiskNode>,

    // AI state (feature-gated)
    #[cfg(feature = "ai")]
    ai_worker: Option<AiWorker>,
    #[cfg(feature = "ai")]
    ai_loading: bool,
    #[cfg(feature = "ai")]
    ai_generating: bool,
    #[cfg(feature = "ai")]
    ai_question: String,
    #[cfg(feature = "ai")]
    ai_response: String,
    #[cfg(feature = "ai")]
    ai_error: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let settings = ca_core::Settings::load(std::path::Path::new("settings.toml"));
        let rules = settings.ruleset();
        
        let is_admin_user = ca_core::is_admin();
        let portable_drive = if let Ok(exe_path) = std::env::current_exe() {
            if let Some(drive) = exe_path.to_string_lossy().split(":\\").next() {
                Some(PathBuf::from(format!("{}:\\", drive)))
            } else {
                None
            }
        } else {
            None
        };

        let mut active_redirections = Vec::new();
        if let Some(drive) = &portable_drive {
            let base_path = drive.join("cache_advisor_portable");
            for opt in REDIRECTION_OPTIONS {
                let mut all_match = true;
                for &var in opt.env_vars {
                    let target_path = base_path.join(opt.sub_folder).to_string_lossy().to_string();
                    if let Ok(Some(current_val)) = ca_core::get_user_env(var) {
                        if current_val.to_lowercase() != target_path.to_lowercase() {
                            all_match = false;
                        }
                    } else {
                        all_match = false;
                    }
                }
                active_redirections.push(all_match);
            }
        }

        Self {
            settings,
            rules,
            results: Vec::new(),
            scores: Vec::new(),
            archive_plan: ArchivePlan::default(),
            selected_tab: Tab::Scan,
            scanning: false,
            scan_rx: None,
            last_scan_time: Some(std::time::Instant::now()),
            tray_icon: init_tray().ok(),
            show_window: true,
            allow_close: false,
            scheduled_scanning: false,
            cleaning_name: None,
            clean_confirming: false,
            last_clean: None,
            archive_confirming: false,
            archive_result: None,
            archive_error: None,
            external_drive: "E:/".into(),
            exclusion_input: String::new(),
            is_admin_user,
            portable_drive,
            active_redirections,
            lock_warning_msg: None,
            is_migrating: false,
            migration_rx: None,

            recycle_root: {
                let p = std::env::temp_dir().join("cache_advisor_recycle_bin");
                let _ = std::fs::create_dir_all(&p);
                p
            },
            clean_sessions: {
                let r = std::env::temp_dir().join("cache_advisor_recycle_bin");
                let mut sessions = Vec::new();
                if let Ok(read_dir) = std::fs::read_dir(&r) {
                    for entry in read_dir.flatten() {
                        let manifest = entry.path().join(ca_actions::SESSION_MANIFEST_NAME);
                        if manifest.exists() {
                            if let Ok(content) = std::fs::read_to_string(&manifest) {
                                if let Ok(session) = serde_json::from_str::<ca_actions::CleanSession>(&content) {
                                    sessions.push(session);
                                }
                            }
                        }
                    }
                }
                sessions.sort_by(|a, b| b.timestamp_secs.cmp(&a.timestamp_secs));
                sessions
            },

            duplicate_groups: Vec::new(),
            dup_scanning: false,
            dup_rx: None,
            duplicate_selections: Vec::new(),

            disk_scan_drive: "C:\\".into(),
            disk_scan_active: false,
            disk_scan_rx: None,
            disk_tree_root: None,
            disk_tree_active: None,
            disk_history: Vec::new(),

            #[cfg(feature = "ai")]
            ai_worker: None,
            #[cfg(feature = "ai")]
            ai_loading: false,
            #[cfg(feature = "ai")]
            ai_generating: false,
            #[cfg(feature = "ai")]
            ai_question: String::new(),
            #[cfg(feature = "ai")]
            ai_response: String::new(),
            #[cfg(feature = "ai")]
            ai_error: None,
        }
    }

    /// Start a background scan of all monitored folders.
    fn start_scan(&mut self, ctx: &egui::Context) {
        let folders = self.rules.folders.clone();
        let stale_days = self.settings.stale_days;
        let exclusions = self.settings.exclusions.clone();
        self.scanning = true;
        self.last_clean = None;
        self.archive_result = None;
        self.archive_error = None;
        ctx.set_cursor_icon(egui::CursorIcon::Wait);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = scan_all(&folders, stale_days, &exclusions);
            let _ = tx.send(res);
        });
        self.scan_rx = Some(rx);
    }

    /// Poll background scan result (non-blocking).
    fn poll_scan(&mut self) {
        let rx = match &self.scan_rx {
            Some(rx) => rx,
            None => return,
        };
        match rx.try_recv() {
            Ok(results) => {
                self.scores = results.iter().map(|r| classify(&r.rule, &r.stats)).collect();
                self.results = results;
                self.scanning = false;
                self.last_scan_time = Some(std::time::Instant::now());
                let ext: PathBuf = self
                    .external_drive
                    .trim_end_matches('/')
                    .trim_end_matches('\\')
                    .into();
                self.archive_plan = ArchivePlan::suggest(&self.results, &self.scores, &ext);

                // Trigger Windows Toast Notification
                let body = if self.scheduled_scanning {
                    "Periodic background scan completed successfully."
                } else {
                    "Scan completed."
                };
                let _ = notify_rust::Notification::new()
                    .summary("Cache Advisor")
                    .body(body)
                    .show();
                self.scheduled_scanning = false;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.scan_rx = None;
            }
        }
    }
}

// ─── eframe App impl ────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_scan();
        self.poll_duplicates();
        self.poll_disk_scan();
        self.poll_migration();

        #[cfg(feature = "ai")]
        self.poll_ai();

        // Periodic Scan Scheduler
        if self.settings.scheduler.enabled && !self.scanning && self.scan_rx.is_none() {
            let last = self.last_scan_time.unwrap_or_else(std::time::Instant::now);
            if std::time::Instant::now().duration_since(last) >= std::time::Duration::from_secs(self.settings.scheduler.interval_mins as u64 * 60) {
                log::info!("Triggering scheduled periodic scan...");
                self.scheduled_scanning = true;
                self.start_scan(ctx);
            }
        }

        // Intercept close button to minimize to tray
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.allow_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_window = false;
            }
        }

        // Poll System Tray menu events
        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            log::info!("System Tray menu event: {:?}", event);
            if event.id.0 == "show_app" {
                self.show_window = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            } else if event.id.0 == "quit_app" {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Poll System Tray icon events (clicks)
        if let Ok(_event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            self.show_window = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        // Handle minimize to tray flag
        if !self.show_window {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.show_window = true;
        }

        // Track tab transitions
        #[cfg(feature = "ai")]
        let prev_tab = self.selected_tab;

        // ── Top bar ──
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🧹 Cache Advisor").size(22.0));
                ui.separator();
                if ui.button("⟳ Rescan All").clicked() {
                    self.start_scan(ctx);
                }
                if self.scanning {
                    ui.spinner();
                    ui.label("Scanning...");
                }

                // Layout aligned to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Theme Toggle
                    let is_dark = ctx.style().visuals.dark_mode;
                    let theme_btn_text = if is_dark { "☀ Light Mode" } else { "🌙 Dark Mode" };
                    if ui.button(theme_btn_text).clicked() {
                        ctx.set_visuals(if is_dark {
                            egui::Visuals::light()
                        } else {
                            egui::Visuals::dark()
                        });
                    }

                    // Export Button
                    let has_results = !self.results.is_empty();
                    let export_btn = ui.add_enabled(has_results, egui::Button::new("📤 Export Report"));
                    if export_btn.clicked() {
                        if let Err(e) = self.export_scan_report() {
                            log::error!("Failed to export report: {}", e);
                            self.archive_error = Some(format!("Failed to export report: {}", e));
                        } else {
                            self.archive_result = Some("Laporan pemindaian berhasil diekspor ke cache-advisor-report.json dan .txt".into());
                        }
                    }
                });
            });
        });

        // ── Tab bar ──
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, Tab::Scan, "📊 Scan");
                ui.selectable_value(&mut self.selected_tab, Tab::Archive, "📦 Archive");
                ui.selectable_value(&mut self.selected_tab, Tab::Duplicates, "🔍 Duplicates");
                ui.selectable_value(&mut self.selected_tab, Tab::DiskMap, "🗺 Disk Map");
                ui.selectable_value(&mut self.selected_tab, Tab::Portable, "🔌 Portable Mode");
                #[cfg(feature = "ai")]
                ui.selectable_value(&mut self.selected_tab, Tab::AskAi, "🤖 Ask AI");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.is_admin_user {
                        ui.label(RichText::new("🛡 Admin Mode").color(Color32::GREEN).strong());
                    } else {
                        ui.label(RichText::new("👤 User Mode").color(Color32::GRAY));
                    }
                });
            });
        });

        #[cfg(feature = "ai")]
        if prev_tab != self.selected_tab {
            self.handle_tab_change(prev_tab, self.selected_tab);
        }

        // ── Main panel ──
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                Tab::Scan => self.panel_scan(ui, ctx),
                Tab::Archive => self.panel_archive(ui),
                Tab::Duplicates => self.panel_duplicates(ui),
                Tab::DiskMap => self.panel_disk_map(ui, ctx),
                Tab::Portable => self.panel_portable(ui),
                #[cfg(feature = "ai")]
                Tab::AskAi => self.panel_ask_ai(ui, ctx),
            }
        });

        // ── Clean confirmation modal ──
        self.modal_clean(ctx);
        self.modal_lock_warning(ctx);

        // ── Bottom status bar ──
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if let Some(out) = &self.last_clean {
                    ui.label(
                        RichText::new(format!(
                            "✅ Cleaned: freed {}, {} files, {} dirs, {} skipped",
                            format_bytes(out.freed_bytes),
                            out.files_removed,
                            out.folders_removed,
                            out.skipped
                        ))
                        .color(Color32::LIGHT_GREEN),
                    );
                }
                if let Some(msg) = &self.archive_result {
                    ui.separator();
                    ui.label(RichText::new(msg).color(Color32::LIGHT_GREEN));
                }
                if let Some(err) = &self.archive_error {
                    ui.separator();
                    ui.label(RichText::new(err).color(Color32::LIGHT_RED));
                }
            });
        });

        if self.settings.scheduler.enabled {
            ctx.request_repaint_after(std::time::Duration::from_secs(10));
        }
    }
}

// ─── Scan Panel ────────────────────────────────────────────────────────────

impl App {
    fn panel_scan(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        if self.results.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.label(RichText::new("Click ⟳ Rescan All to begin").size(20.0));
            });
            return;
        }

        // Summary
        let total_bytes: u64 = self.results.iter().map(|r| r.stats.total_bytes).sum();
        ui.horizontal(|ui| {
            ui.label(format!("{} folders", self.results.len()));
            ui.separator();
            ui.strong(format!("Total: {}", format_bytes(total_bytes)));
        });
        ui.add_space(6.0);

        // Table
        use egui_extras::{Column, TableBuilder};

        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(200.0).clip(true))
            .column(Column::initial(100.0).clip(true))
            .column(Column::initial(90.0))
            .column(Column::initial(55.0))
            .column(Column::initial(65.0))
            .column(Column::initial(55.0))
            .column(Column::initial(55.0))
            .column(Column::initial(300.0).clip(true))
            .column(Column::remainder().at_least(80.0))
            .header(20.0, |mut header| {
                header.col(|ui| { ui.strong("Name"); });
                header.col(|ui| { ui.strong("Path"); });
                header.col(|ui| { ui.strong("Size"); });
                header.col(|ui| { ui.strong("Files"); });
                header.col(|ui| { ui.strong("Tier"); });
                header.col(|ui| { ui.strong("Risk"); });
                header.col(|ui| { ui.strong("Score"); });
                header.col(|ui| { ui.strong("Reason"); });
                header.col(|ui| { ui.strong("Action"); });
            })
            .body(|mut body| {
                for (res, score) in self.results.iter().zip(self.scores.iter()) {
                    let color = risk_color(score.level);
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            ui.label(&res.rule.name);
                        });
                        row.col(|ui| {
                            let p = res.rule.path.display().to_string();
                            ui.label(if p.len() > 45 { format!("{}…", &p[..42]) } else { p });
                        });
                        row.col(|ui| {
                            if res.stats.exists {
                                ui.strong(RichText::new(res.stats.human_size()).color(color));
                            } else {
                                ui.label(RichText::new("—").color(Color32::GRAY));
                            }
                        });
                        row.col(|ui| {
                            ui.label(if res.stats.exists {
                                res.stats.file_count.to_string()
                            } else {
                                "—".into()
                            });
                        });
                        row.col(|ui| {
                            let t = match res.rule.tier {
                                CleaningTier::Cache => "Cache",
                                CleaningTier::Cautious => "⚠ Cautious",
                                CleaningTier::MonitorOnly => "🔒 Monitor",
                            };
                            ui.label(t);
                        });
                        row.col(|ui| {
                            let (txt, c) = risk_badge(score.level);
                            ui.strong(RichText::new(txt).color(c));
                        });
                        row.col(|ui| {
                            ui.add(
                                egui::ProgressBar::new(score.urgency as f32 / 100.0)
                                    .fill(color)
                                    .show_percentage(),
                            );
                        });
                        row.col(|ui| {
                            ui.label(&score.reason);
                        });
                        row.col(|ui| {
                            ui.horizontal(|ui| {
                                if score.auto_cleanable && res.stats.exists {
                                    if ui.button("🧹 Clean").clicked() {
                                        self.cleaning_name = Some(res.rule.name.clone());
                                        self.clean_confirming = true;
                                    }
                                }
                                if score.archive_candidate && res.stats.exists {
                                    if ui.button("📦").clicked() {
                                        self.selected_tab = Tab::Archive;
                                    }
                                }
                            });
                        });
                    });
                }
            });

        ui.add_space(6.0);
        // Legend
        ui.horizontal(|ui| {
            ui.label(RichText::new("●").color(Color32::from_rgb(40, 180, 40)));
            ui.label("Healthy  ");
            ui.label(RichText::new("●").color(Color32::from_rgb(220, 180, 30)));
            ui.label("Watch  ");
            ui.label(RichText::new("●").color(Color32::from_rgb(220, 60, 60)));
            ui.label("Heavy  ");
            ui.label(RichText::new("●").color(Color32::GRAY));
            ui.label("Protected");
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Recycle Bin / Undo Sessions
        ui.collapsing("🗑 Recent Cleaning Sessions (Undo / Recycle Bin)", |ui| {
            if self.clean_sessions.is_empty() {
                ui.label("No recent cleaning sessions.");
                return;
            }

            let mut session_to_restore = None;
            let mut session_to_purge = None;

            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                for (idx, session) in self.clean_sessions.iter().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let formatted_time = format_timestamp(session.timestamp_secs);

                            ui.label(format!(
                                "Session {} (Freed {})",
                                formatted_time,
                                format_bytes(session.freed_bytes)
                            ));
                            ui.label(format!("({} items)", session.entries.len()));

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("🗑 Purge").clicked() {
                                    session_to_purge = Some(idx);
                                }
                                if ui.button("⬆ Undo").clicked() {
                                    session_to_restore = Some(idx);
                                }
                            });
                        });
                    });
                }
            });

            if let Some(idx) = session_to_restore {
                let session = &self.clean_sessions[idx];
                let manifest_path = self.recycle_root.join(&session.session_id).join(ca_actions::SESSION_MANIFEST_NAME);
                match ca_actions::restore_clean_session(&manifest_path) {
                    Ok(_) => {
                        self.clean_sessions.remove(idx);
                        self.archive_result = Some("Pembersihan berhasil dibatalkan (Undo Clean sukses).".into());
                        // Trigger Windows Toast Notification
                        let _ = notify_rust::Notification::new()
                            .summary("Cache Advisor")
                            .body("Undo Clean: files successfully restored to their original paths.")
                            .show();
                    }
                    Err(e) => {
                        log::error!("Failed to restore clean session: {}", e);
                        self.archive_error = Some(format!("Failed to undo clean: {}", e));
                    }
                }
            }

            if let Some(idx) = session_to_purge {
                let session = &self.clean_sessions[idx];
                let manifest_path = self.recycle_root.join(&session.session_id).join(ca_actions::SESSION_MANIFEST_NAME);
                match ca_actions::purge_clean_session(&manifest_path) {
                    Ok(_) => {
                        self.clean_sessions.remove(idx);
                        self.archive_result = Some("Sesi pembersihan berhasil dihapus secara permanen.".into());
                    }
                    Err(e) => {
                        log::error!("Failed to purge clean session: {}", e);
                        self.archive_error = Some(format!("Failed to purge clean: {}", e));
                    }
                }
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Exclusion List / Whitelist Management
        ui.collapsing("🛡 Monitored Folders Exclusion List (Whitelist)", |ui| {
            ui.horizontal(|ui| {
                ui.label("Add Path to Exclude:");
                ui.text_edit_singleline(&mut self.exclusion_input);
                if ui.button("➕ Add").clicked() {
                    let path_str = self.exclusion_input.trim().to_string();
                    if !path_str.is_empty() && !self.settings.exclusions.contains(&path_str) {
                        self.settings.exclusions.push(path_str);
                        self.exclusion_input.clear();
                        let _ = self.settings.save(Path::new("settings.toml"));
                    }
                }
            });

            ui.add_space(6.0);

            if self.settings.exclusions.is_empty() {
                ui.label("No folder or file paths excluded.");
            } else {
                let mut index_to_remove = None;
                egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    for (idx, exclusion) in self.settings.exclusions.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(exclusion);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("❌ Remove").clicked() {
                                    index_to_remove = Some(idx);
                                }
                            });
                        });
                    }
                });

                if let Some(idx) = index_to_remove {
                    self.settings.exclusions.remove(idx);
                    let _ = self.settings.save(Path::new("settings.toml"));
                }
            }
        });
    }

    // ─── Clean modal ────────────────────────────────────────────────────────

    fn modal_clean(&mut self, ctx: &egui::Context) {
        if !self.clean_confirming {
            return;
        }
        let name = match &self.cleaning_name {
            Some(n) => n.clone(),
            None => {
                self.clean_confirming = false;
                return;
            }
        };

        // Find the path for this folder name
        let path = self
            .results
            .iter()
            .find(|r| r.rule.name == name)
            .map(|r| r.rule.path.clone());

        let path = match path {
            Some(p) => p,
            None => {
                self.clean_confirming = false;
                return;
            }
        };

        // Show confirmation dialog
        egui::Window::new("Confirm Clean")
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("⚠ Confirm deletion").strong().size(18.0));
                ui.add_space(8.0);
                ui.label(format!(
                    "Delete all contents of:\n  {}",
                    path.display()
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("✅ Yes, Clean").color(Color32::GREEN)).clicked() {
                        match ca_actions::clean_folder_to_recycle_bin(&path, &self.recycle_root) {
                            Ok(session) => {
                                self.clean_sessions.insert(0, session.clone());
                                let out = ca_actions::CleanOutcome {
                                    freed_bytes: session.freed_bytes,
                                    files_removed: session.entries.iter().filter(|e| !e.is_dir).count() as u64,
                                    folders_removed: session.entries.iter().filter(|e| e.is_dir).count() as u64,
                                    skipped: 0,
                                };
                                self.last_clean = Some(out);
                                // Trigger Toast Notification
                                let _ = notify_rust::Notification::new()
                                    .summary("Cache Advisor")
                                    .body(&format!(
                                        "Cleaned: freed {}",
                                        format_bytes(session.freed_bytes)
                                    ))
                                    .show();
                            }
                            Err(e) => log::error!("Clean failed: {}", e),
                        }
                        self.clean_confirming = false;
                        self.cleaning_name = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.clean_confirming = false;
                        self.cleaning_name = None;
                    }
                });
            });
    }

    fn modal_lock_warning(&mut self, ctx: &egui::Context) {
        if let Some(msg) = &self.lock_warning_msg {
            let msg_clone = msg.clone();
            egui::Window::new("⚠ Warning")
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_width(350.0)
                .show(ctx, |ui| {
                    ui.label(RichText::new("Application Lock / Notification").strong().size(16.0));
                    ui.add_space(8.0);
                    ui.label(&msg_clone);
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            self.lock_warning_msg = None;
                        }
                    });
                });
        }
    }
}

// ─── Archive Panel ─────────────────────────────────────────────────────────

impl App {
    fn panel_archive(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("External drive:");
            let resp = ui.text_edit_singleline(&mut self.external_drive);
            if resp.lost_focus() || ui.button("🔄 Refresh").clicked() {
                let ext: PathBuf = self
                    .external_drive
                    .trim_end_matches('/')
                    .trim_end_matches('\\')
                    .into();
                self.archive_plan = ArchivePlan::suggest(&self.results, &self.scores, &ext);
            }
        });
        ui.add_space(4.0);

        if self.archive_plan.entries.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("No archive candidates. Run a scan first.");
                ui.label("Candidates appear when folders are large and fresh (>500MB, <30% stale).");
            });
            return;
        }

        ui.horizontal(|ui| {
            ui.strong(format!(
                "{} folders to archive — {} total",
                self.archive_plan.entries.len(),
                format_bytes(self.archive_plan.total_bytes())
            ));
        });
        ui.add_space(4.0);

        use egui_extras::{Column, TableBuilder};
        TableBuilder::new(ui)
            .striped(true)
            .column(Column::initial(250.0).clip(true))
            .column(Column::initial(90.0))
            .column(Column::initial(250.0).clip(true))
            .column(Column::remainder().clip(true))
            .header(20.0, |mut header| {
                header.col(|ui| { ui.strong("Source"); });
                header.col(|ui| { ui.strong("Size"); });
                header.col(|ui| { ui.strong("Destination"); });
                header.col(|ui| { ui.strong("Reason"); });
            })
            .body(|mut body| {
                for entry in &self.archive_plan.entries {
                    body.row(20.0, |mut row| {
                        row.col(|ui| { ui.label(entry.source.display().to_string()); });
                        row.col(|ui| { ui.label(format_bytes(entry.bytes)); });
                        row.col(|ui| { ui.label(entry.destination.display().to_string()); });
                        row.col(|ui| { ui.label(&entry.reason); });
                    });
                }
            });

        ui.add_space(12.0);

        if !self.archive_confirming {
            if ui.button(RichText::new("✅ Confirm Archive").size(18.0)).clicked() {
                self.archive_confirming = true;
            }
        } else {
            ui.horizontal(|ui| {
                ui.label(RichText::new(
                    "⚠ Move selected folders to external drive? A manifest for undo will be saved.",
                ).color(Color32::YELLOW));
                if ui.button("Yes, proceed").clicked() {
                    let ext: PathBuf = self
                        .external_drive
                        .trim_end_matches('/')
                        .trim_end_matches('\\')
                        .into();
                    match run_archive(&self.archive_plan.entries, &ext) {
                        Ok(out) => {
                            self.archive_result = Some(format!(
                                "📦 Archived: {} items, {} moved, {} skipped",
                                out.moved.len(),
                                format_bytes(out.bytes_moved),
                                out.skipped
                            ));
                            // Trigger Toast Notification
                            let _ = notify_rust::Notification::new()
                                .summary("Cache Advisor")
                                .body(&format!(
                                    "Archiving completed. Moved {}",
                                    format_bytes(out.bytes_moved)
                                ))
                                .show();
                        }
                        Err(e) => {
                            self.archive_error = Some(format!("Archive error: {e}"));
                        }
                    }
                    self.archive_confirming = false;
                }
                if ui.button("Cancel").clicked() {
                    self.archive_confirming = false;
                }
            });
        }
    }
}

// ─── Color helpers ──────────────────────────────────────────────────────────

fn risk_color(level: RiskLevel) -> Color32 {
    match level {
        RiskLevel::Healthy => Color32::from_rgb(40, 180, 40),
        RiskLevel::Watch => Color32::from_rgb(220, 180, 30),
        RiskLevel::Heavy => Color32::from_rgb(220, 60, 60),
        RiskLevel::Protected => Color32::GRAY,
    }
}

fn risk_badge(level: RiskLevel) -> (&'static str, Color32) {
    match level {
        RiskLevel::Healthy => ("✅", Color32::LIGHT_GREEN),
        RiskLevel::Watch => ("⚠️", Color32::YELLOW),
        RiskLevel::Heavy => ("🔴", Color32::LIGHT_RED),
        RiskLevel::Protected => ("🔒", Color32::GRAY),
    }
}

// ─── AI helpers (feature-gated) ──────────────────────────────────────────────

#[cfg(feature = "ai")]
impl App {
    /// Poll the background AI worker for updates.
    fn poll_ai(&mut self) {
        let worker = match &self.ai_worker {
            Some(w) => w,
            None => return,
        };
        while let Ok(res) = worker.rx.try_recv() {
            match res {
                AiResponse::Loaded => {
                    self.ai_loading = false;
                }
                AiResponse::Answer(ans) => {
                    self.ai_generating = false;
                    self.ai_response = ans;
                }
                AiResponse::Error(err) => {
                    self.ai_loading = false;
                    self.ai_generating = false;
                    self.ai_error = Some(err);
                }
            }
        }
    }

    /// Manage LLM memory on tab changes.
    fn handle_tab_change(&mut self, prev: Tab, next: Tab) {
        if prev == Tab::AskAi && next != Tab::AskAi {
            log::info!("Leaving Ask AI tab. Dropping AI worker to free RAM.");
            self.ai_worker = None;
            self.ai_loading = false;
            self.ai_generating = false;
            self.ai_error = None;
        } else if prev != Tab::AskAi && next == Tab::AskAi {
            log::info!("Entering Ask AI tab. Starting AI worker on-demand.");
            self.start_ai_worker();
        }
    }

    /// Spawn the background AI worker thread and load the GGUF model.
    fn start_ai_worker(&mut self) {
        self.ai_loading = true;
        self.ai_error = None;
        self.ai_response.clear();

        let path = if let Some(ref custom_path) = self.settings.llm.model_path {
            PathBuf::from(custom_path)
        } else {
            PathBuf::from(ca_llm::DEFAULT_MODEL_PATH)
        };
        if !ca_llm::LlmEngine::model_available(&path) {
            self.ai_error = Some(format!(
                "Berkas model GGUF tidak ditemukan di:\n  {}\n\nPastikan model Qwen2 1.5B sudah ada di path tersebut.",
                path.display()
            ));
            self.ai_loading = false;
            return;
        }

        let (req_tx, req_rx) = std::sync::mpsc::channel();
        let (res_tx, res_rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let mut engine = match ca_llm::LlmEngine::load(&path) {
                Ok(eng) => {
                    let _ = res_tx.send(AiResponse::Loaded);
                    Some(eng)
                }
                Err(e) => {
                    let _ = res_tx.send(AiResponse::Error(format!("Failed to load model: {e}")));
                    None
                }
            };

            if engine.is_some() {
                while let Ok(req) = req_rx.recv() {
                    match req {
                        AiRequest::Ask(prompt) => {
                            if let Some(ref mut eng) = engine {
                                match eng.generate(&prompt) {
                                    Ok(ans) => {
                                        let _ = res_tx.send(AiResponse::Answer(ans));
                                    }
                                    Err(e) => {
                                        let _ = res_tx.send(AiResponse::Error(format!("Generation failed: {e}")));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        self.ai_worker = Some(AiWorker {
            tx: req_tx,
            rx: res_rx,
        });
    }

    /// Render the "Ask AI" panel.
    fn panel_ask_ai(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.vertical(|ui| {
            ui.heading("🤖 Ask AI (Local Storage Advisor)");
            ui.add_space(8.0);

            // Status bar
            ui.horizontal(|ui| {
                ui.label("Status Model:");
                if self.ai_loading {
                    ui.spinner();
                    ui.label(RichText::new("Loading model into memory... (On-Demand)").color(Color32::LIGHT_BLUE));
                } else if self.ai_worker.is_some() {
                    ui.strong(RichText::new("Loaded & Ready (Qwen2 1.5B Local)").color(Color32::LIGHT_GREEN));
                } else if let Some(_) = &self.ai_error {
                    ui.strong(RichText::new("Error Loading Model").color(Color32::LIGHT_RED));
                } else {
                    ui.label("Disconnected");
                }
            });

            if let Some(err) = &self.ai_error {
                ui.add_space(8.0);
                ui.colored_label(Color32::LIGHT_RED, err);
            }

            ui.separator();
            ui.add_space(8.0);

            if self.ai_worker.is_some() && !self.ai_loading {
                ui.horizontal(|ui| {
                    if ui.button("🔍 Analisis Hasil Scan Utama").clicked() {
                        if !self.ai_generating {
                            self.ai_generating = true;
                            self.ai_error = None;
                            self.ai_response = "Thinking... Please wait.".into();
                            let prompt = ca_llm::build_scan_prompt(&self.results, &self.scores);
                            if let Some(worker) = &self.ai_worker {
                                let _ = worker.tx.send(AiRequest::Ask(prompt));
                            }
                        }
                    }
                    ui.label("(Menganalisis folder cache & resiko disk)");
                });

                ui.add_space(12.0);

                ui.label("Ajukan pertanyaan kustom tentang penyimpanan Anda:");
                ui.horizontal(|ui| {
                    let resp = ui.text_edit_singleline(&mut self.ai_question);
                    if (ui.button("Tanyakan").clicked() || (resp.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))))
                        && !self.ai_question.trim().is_empty()
                    {
                        if !self.ai_generating {
                            self.ai_generating = true;
                            self.ai_error = None;
                            let prompt = ca_llm::build_custom_prompt(&self.results, &self.scores, &self.ai_question);
                            self.ai_question.clear();
                            self.ai_response = "Thinking... Please wait.".into();
                            if let Some(worker) = &self.ai_worker {
                                let _ = worker.tx.send(AiRequest::Ask(prompt));
                            }
                        }
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);

                ui.label("Jawaban AI:");
                ui.add_space(4.0);

                egui::ScrollArea::vertical()
                    .max_height(350.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let text_color = if self.ai_generating {
                            Color32::GRAY
                        } else {
                            Color32::WHITE
                        };
                        ui.add(
                            egui::TextEdit::multiline(&mut self.ai_response)
                                .text_color(text_color)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(f32::INFINITY)
                                .desired_rows(12)
                                .interactive(false),
                        );
                    });
            } else if !self.ai_loading {
                ui.colored_label(Color32::YELLOW, "Model tidak dimuat. Masuk kembali ke tab ini untuk mencoba memuat ulang.");
            }
        });
    }
}

impl App {
    /// Poll the background duplicates scan thread.
    fn poll_duplicates(&mut self) {
        let rx = match &self.dup_rx {
            Some(r) => r,
            None => return,
        };
        match rx.try_recv() {
            Ok(groups) => {
                self.duplicate_selections = groups.iter()
                    .map(|group| vec![false; group.file_paths.len()])
                    .collect();
                self.duplicate_groups = groups;
                self.dup_scanning = false;
                self.dup_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.dup_scanning = false;
                self.dup_rx = None;
            }
        }
    }

    fn check_redirections_status(&mut self) {
        if let Some(drive) = &self.portable_drive {
            let base_path = drive.join("cache_advisor_portable");
            self.active_redirections.clear();

            for opt in REDIRECTION_OPTIONS {
                match opt.redirection_type {
                    RedirectionType::EnvVar => {
                        let mut all_match = true;
                        for &var in opt.env_vars {
                            let target_path = base_path.join(opt.sub_folder).to_string_lossy().to_string();
                            if let Ok(Some(current_val)) = ca_core::get_user_env(var) {
                                if current_val.to_lowercase() != target_path.to_lowercase() {
                                    all_match = false;
                                }
                            } else {
                                all_match = false;
                            }
                        }
                        self.active_redirections.push(all_match);
                    }
                    RedirectionType::Junction => {
                        if let Some(c_path) = resolve_c_path(opt) {
                            self.active_redirections.push(ca_core::is_junction(&c_path));
                        } else {
                            self.active_redirections.push(false);
                        }
                    }
                }
            }
        }
    }

    fn poll_migration(&mut self) {
        let rx = match &self.migration_rx {
            Some(r) => r,
            None => return,
        };
        match rx.try_recv() {
            Ok(result) => {
                self.is_migrating = false;
                self.migration_rx = None;
                self.check_redirections_status();
                match result {
                    Ok(_) => {
                        let _ = notify_rust::Notification::new()
                            .summary("Cache Advisor")
                            .body("Relocation and redirection completed successfully.")
                            .show();
                    }
                    Err(e) => {
                        self.lock_warning_msg = Some(format!("Migration failed: {}", e));
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.is_migrating = false;
                self.migration_rx = None;
                self.check_redirections_status();
            }
        }
    }

    /// Spawn a thread to scan for duplicates.
    fn start_duplicates_scan(&mut self) {
        let directories: Vec<PathBuf> = self.rules.folders.iter().map(|f| f.path.clone()).collect();
        let exclusions = self.settings.exclusions.clone();
        self.dup_scanning = true;
        self.duplicate_groups.clear();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = ca_core::find_duplicates(&directories, &exclusions);
            let _ = tx.send(res);
        });
        self.dup_rx = Some(rx);
    }

    /// Render the duplicate files finder panel.
    fn panel_duplicates(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.heading("🔍 Duplicate Files Finder (SHA-256)");
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button("Scan for Duplicate Files").clicked() {
                    self.start_duplicates_scan();
                }
                if self.dup_scanning {
                    ui.spinner();
                    ui.label("Scanning folders and computing SHA-256 hashes...");
                }
            });

            ui.add_space(6.0);

            if !self.duplicate_groups.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Smart Selection:");
                    if ui.button("Keep Oldest").clicked() {
                        self.select_duplicates_keep_oldest();
                    }
                    if ui.button("Keep Newest").clicked() {
                        self.select_duplicates_keep_newest();
                    }
                    ui.separator();
                    
                    let mut selected_count = 0;
                    let mut selected_bytes = 0;
                    for (g_idx, group) in self.duplicate_groups.iter().enumerate() {
                        for (p_idx, _) in group.file_paths.iter().enumerate() {
                            if g_idx < self.duplicate_selections.len() && p_idx < self.duplicate_selections[g_idx].len() && self.duplicate_selections[g_idx][p_idx] {
                                selected_count += 1;
                                selected_bytes += group.file_size;
                            }
                        }
                    }

                    let btn_label = if selected_count > 0 {
                        format!("🧹 Clean Selected Duplicates ({}, {})", selected_count, format_bytes(selected_bytes))
                    } else {
                        "🧹 Clean Selected Duplicates".to_string()
                    };

                    if ui.add_enabled(selected_count > 0, egui::Button::new(RichText::new(btn_label).color(Color32::RED))).clicked() {
                        self.delete_selected_duplicates();
                    }
                });
                ui.add_space(8.0);
            }

            ui.separator();
            ui.add_space(8.0);

            if self.duplicate_groups.is_empty() {
                if !self.dup_scanning {
                    ui.label("No duplicate groups found or scan not run yet.");
                }
            } else {
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut to_remove: Option<(usize, usize)> = None;

                        for (group_idx, group) in self.duplicate_groups.iter().enumerate() {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(format!(
                                        "File size: {} | Hash: {:.8}...",
                                        format_bytes(group.file_size),
                                        group.hash
                                    ));
                                    ui.label(format!("({} copies)", group.file_paths.len()));
                                });
                                ui.add_space(4.0);

                                for (path_idx, path) in group.file_paths.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if group_idx < self.duplicate_selections.len() && path_idx < self.duplicate_selections[group_idx].len() {
                                            ui.checkbox(&mut self.duplicate_selections[group_idx][path_idx], "");
                                        }
                                        ui.label(path.display().to_string());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let can_delete = group.file_paths.len() > 1;
                                            let btn = ui.add_enabled(can_delete, egui::Button::new("🗑 Delete"));
                                            if btn.clicked() {
                                                to_remove = Some((group_idx, path_idx));
                                            }
                                        });
                                    });
                                }
                            });
                            ui.add_space(8.0);
                        }

                        if let Some((group_idx, path_idx)) = to_remove {
                            let path = &self.duplicate_groups[group_idx].file_paths[path_idx];
                            match ca_actions::clean_file_to_recycle_bin(path, &self.recycle_root) {
                                Ok(session) => {
                                    self.clean_sessions.insert(0, session.clone());
                                    let freed = session.freed_bytes;
                                    log::info!("Deleted duplicate file: {}, freed {} bytes", path.display(), freed);
                                    self.duplicate_groups[group_idx].file_paths.remove(path_idx);
                                    if self.duplicate_groups[group_idx].file_paths.len() <= 1 {
                                        self.duplicate_groups.remove(group_idx);
                                        self.duplicate_selections.remove(group_idx);
                                    } else {
                                        self.duplicate_selections[group_idx] = vec![false; self.duplicate_groups[group_idx].file_paths.len()];
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to delete file {}: {}", path.display(), e);
                                }
                            }
                        }
                    });
            }
        });
    }

    fn select_duplicates_keep_oldest(&mut self) {
        self.duplicate_selections.clear();
        for group in &self.duplicate_groups {
            let mut selections = vec![false; group.file_paths.len()];
            let mut oldest_idx = 0;
            let mut oldest_time = std::time::SystemTime::now();

            for (idx, path) in group.file_paths.iter().enumerate() {
                if let Ok(meta) = std::fs::metadata(path) {
                    if let Ok(mtime) = meta.modified() {
                        if mtime < oldest_time {
                            oldest_time = mtime;
                            oldest_idx = idx;
                        }
                    }
                }
            }

            for idx in 0..group.file_paths.len() {
                if idx != oldest_idx {
                    selections[idx] = true;
                }
            }
            self.duplicate_selections.push(selections);
        }
    }

    fn select_duplicates_keep_newest(&mut self) {
        self.duplicate_selections.clear();
        for group in &self.duplicate_groups {
            let mut selections = vec![false; group.file_paths.len()];
            let mut newest_idx = 0;
            let mut newest_time = std::time::SystemTime::UNIX_EPOCH;

            for (idx, path) in group.file_paths.iter().enumerate() {
                if let Ok(meta) = std::fs::metadata(path) {
                    if let Ok(mtime) = meta.modified() {
                        if mtime > newest_time {
                            newest_time = mtime;
                            newest_idx = idx;
                        }
                    }
                }
            }

            for idx in 0..group.file_paths.len() {
                if idx != newest_idx {
                    selections[idx] = true;
                }
            }
            self.duplicate_selections.push(selections);
        }
    }

    fn delete_selected_duplicates(&mut self) {
        let mut total_freed = 0;
        let mut files_removed = 0;
        let mut sessions = Vec::new();

        for group_idx in (0..self.duplicate_groups.len()).rev() {
            let mut paths_to_remove = Vec::new();
            for path_idx in (0..self.duplicate_groups[group_idx].file_paths.len()).rev() {
                if group_idx < self.duplicate_selections.len() && path_idx < self.duplicate_selections[group_idx].len() && self.duplicate_selections[group_idx][path_idx] {
                    paths_to_remove.push(path_idx);
                }
            }

            if paths_to_remove.len() == self.duplicate_groups[group_idx].file_paths.len() {
                paths_to_remove.remove(0);
            }

            for &path_idx in &paths_to_remove {
                let path = &self.duplicate_groups[group_idx].file_paths[path_idx];
                match ca_actions::clean_file_to_recycle_bin(path, &self.recycle_root) {
                    Ok(session) => {
                        total_freed += session.freed_bytes;
                        files_removed += 1;
                        sessions.push(session);
                    }
                    Err(e) => {
                        log::error!("Failed to delete duplicate file {}: {}", path.display(), e);
                    }
                }
            }

            // Remove files from back to front
            for path_idx in paths_to_remove {
                self.duplicate_groups[group_idx].file_paths.remove(path_idx);
            }

            if self.duplicate_groups[group_idx].file_paths.len() <= 1 {
                self.duplicate_groups.remove(group_idx);
                self.duplicate_selections.remove(group_idx);
            } else {
                self.duplicate_selections[group_idx] = vec![false; self.duplicate_groups[group_idx].file_paths.len()];
            }
        }

        for session in sessions {
            self.clean_sessions.insert(0, session);
        }

        if files_removed > 0 {
            let _ = notify_rust::Notification::new()
                .summary("Cache Advisor")
                .body(&format!(
                    "Cleaned Selected Duplicates: removed {} files, freed {}",
                    files_removed,
                    format_bytes(total_freed)
                ))
                .show();
        }
    }

    fn panel_portable(&mut self, ui: &mut egui::Ui) {
        if self.is_migrating {
            ui.vertical_centered(|ui| {
                ui.add_space(100.0);
                ui.spinner();
                ui.add_space(10.0);
                ui.label(RichText::new("Migrating data and configuring links. Please wait...").strong().size(16.0));
                ui.label("Do not disconnect your external storage drive.");
            });
            return;
        }

        ui.vertical(|ui| {
            ui.heading("🔌 Portable Mode & Auto-Routing Agent");
            ui.add_space(8.0);
            ui.label(
                "Run Cache Advisor from an external USB/HDD drive to redirect application cache folders into it. \
                This prevents your C: drive from filling up by automatically routing data to the external drive."
            );
            ui.add_space(12.0);

            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("Privilege Status:");
                    if self.is_admin_user {
                        ui.label(RichText::new("🛡 Administrator").color(Color32::GREEN).strong());
                    } else {
                        ui.label(RichText::new("👤 User").color(Color32::YELLOW).strong());
                        ui.label("(Some system-wide temp files may need Admin privileges to clean)");
                    }
                });

                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Detected Program Drive:");
                    if let Some(drive) = &self.portable_drive {
                        ui.strong(format!("{}", drive.display()));
                    } else {
                        ui.label(RichText::new("Not detected").color(Color32::RED));
                    }
                });
            });

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(16.0);

            ui.heading("📦 Application Cache Auto-Routing");
            ui.add_space(8.0);

            if let Some(drive) = &self.portable_drive {
                let base_path = drive.join("cache_advisor_portable");

                for (idx, opt) in REDIRECTION_OPTIONS.iter().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let mut is_active = idx < self.active_redirections.len() && self.active_redirections[idx];
                            if ui.checkbox(&mut is_active, RichText::new(opt.name).strong().size(14.0)).clicked() {
                                // 1. Check running processes to prevent locked file issues
                                let mut running_processes = Vec::new();
                                for &proc in opt.process_names {
                                    if ca_core::is_process_running(proc) {
                                        running_processes.push(proc);
                                    }
                                }

                                if !running_processes.is_empty() {
                                    self.lock_warning_msg = Some(format!(
                                        "Cannot modify redirection because associated applications are currently running.\n\
                                        Please close the following apps and try again:\n  {}",
                                        running_processes.join(", ")
                                    ));
                                    return;
                                }

                                // 2. Trigger async background migration
                                let target_usb_path = base_path.join(opt.sub_folder);
                                let (tx, rx) = std::sync::mpsc::channel();
                                self.migration_rx = Some(rx);
                                self.is_migrating = true;

                                let opt_name = opt.name.to_string();
                                let target_usb_path_clone = target_usb_path.clone();
                                let is_active_clone = is_active;

                                std::thread::spawn(move || {
                                    let opt_ref = REDIRECTION_OPTIONS.iter().find(|o| o.name == opt_name).unwrap();
                                    
                                    let res = match opt_ref.redirection_type {
                                        RedirectionType::EnvVar => {
                                            let res_migration = if opt_ref.name != "User Temp Files" {
                                                if let Some(c_path) = resolve_env_var_default_c_path(&opt_name) {
                                                    if is_active_clone {
                                                        ca_core::migrate_folder(&c_path, &target_usb_path_clone)
                                                    } else {
                                                        let _ = std::fs::create_dir_all(&c_path);
                                                        ca_core::migrate_folder(&target_usb_path_clone, &c_path)
                                                    }
                                                } else {
                                                    Ok(())
                                                }
                                            } else {
                                                Ok(())
                                            };

                                            res_migration.and_then(|_| {
                                                let target_path = target_usb_path_clone.to_string_lossy().to_string();
                                                for &var in opt_ref.env_vars {
                                                    if is_active_clone {
                                                        let _ = std::fs::create_dir_all(&target_usb_path_clone);
                                                        let _ = ca_core::set_user_env(var, &target_path);
                                                    } else {
                                                        if opt_ref.name == "User Temp Files" {
                                                            let default_temp = "%USERPROFILE%\\AppData\\Local\\Temp";
                                                            let _ = ca_core::set_user_env(var, default_temp);
                                                        } else {
                                                            let _ = ca_core::unset_user_env(var);
                                                        }
                                                    }
                                                }
                                                Ok(())
                                            })
                                        }
                                        RedirectionType::Junction => {
                                            if let Some(c_path) = resolve_c_path(opt_ref) {
                                                if is_active_clone {
                                                    if c_path.exists() && !ca_core::is_junction(&c_path) {
                                                        ca_core::migrate_folder(&c_path, &target_usb_path_clone)
                                                            .and_then(|_| ca_core::create_junction(&c_path, &target_usb_path_clone).map(|_| ()))
                                                    } else {
                                                        let _ = std::fs::create_dir_all(&target_usb_path_clone);
                                                        ca_core::create_junction(&c_path, &target_usb_path_clone).map(|_| ())
                                                    }
                                                } else {
                                                    if ca_core::is_junction(&c_path) {
                                                        let _ = ca_core::delete_junction(&c_path);
                                                    }
                                                    let _ = std::fs::create_dir_all(&c_path);
                                                    ca_core::migrate_folder(&target_usb_path_clone, &c_path)
                                                }
                                            } else {
                                                Err(std::io::Error::new(
                                                    std::io::ErrorKind::NotFound,
                                                    "C: Drive path could not be resolved."
                                                ))
                                            }
                                        }
                                    };
                                    let _ = tx.send(res);
                                });
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if idx < self.active_redirections.len() && self.active_redirections[idx] {
                                    ui.label(RichText::new("ACTIVE (Routed to USB)").color(Color32::GREEN).strong());
                                } else {
                                    ui.label(RichText::new("DEFAULT (C: Drive)").color(Color32::GRAY));
                                }
                            });
                        });

                        ui.add_space(4.0);
                        ui.label(opt.description);
                        
                        let target_dir = base_path.join(opt.sub_folder);
                        ui.label(RichText::new(format!("USB Target Path: {}", target_dir.display())).weak());
                    });
                    ui.add_space(8.0);
                }
            } else {
                ui.label(
                    RichText::new("Please run the application from an external drive to enable Auto-Routing features.")
                        .color(Color32::RED)
                );
            }
        });
    }

    /// Export the scan results and duplicate files info to JSON and formatted text.
    fn export_scan_report(&self) -> Result<(), Box<dyn std::error::Error>> {
        use std::fs::File;
        use std::io::Write;

        let now_str = jiff::Zoned::now().to_string();

        // 1. Export JSON report
        #[derive(serde::Serialize)]
        struct ReportData<'a> {
            timestamp: String,
            stale_days: u32,
            results: &'a [ca_core::ScanResult],
            scores: &'a [ca_core::RiskScore],
        }

        let data = ReportData {
            timestamp: now_str.clone(),
            stale_days: self.settings.stale_days,
            results: &self.results,
            scores: &self.scores,
        };

        let json_str = serde_json::to_string_pretty(&data)?;
        let mut json_file = File::create("cache-advisor-report.json")?;
        json_file.write_all(json_str.as_bytes())?;

        // 2. Export text/markdown report
        let mut txt = String::new();
        txt.push_str("=========================================\n");
        txt.push_str("        CACHE ADVISOR SCAN REPORT\n");
        txt.push_str("=========================================\n");
        txt.push_str(&format!("Generated: {}\n", now_str));
        txt.push_str(&format!("Stale Days Threshold: {}\n\n", self.settings.stale_days));

        txt.push_str("MONITORED FOLDERS:\n");
        for (res, score) in self.results.iter().zip(self.scores.iter()) {
            if !res.stats.exists {
                txt.push_str(&format!("- {} (Not Found)\n", res.rule.name));
                continue;
            }
            let tier_str = match res.rule.tier {
                ca_core::rules::CleaningTier::Cache => "cache",
                ca_core::rules::CleaningTier::Cautious => "cautious",
                ca_core::rules::CleaningTier::MonitorOnly => "monitor-only",
            };
            txt.push_str(&format!(
                "- {} ({}):\n  tier={}, urgency={}/100, files={}, stale={}/{}\n  reason: {}\n",
                res.rule.name,
                format_bytes(res.stats.total_bytes),
                tier_str,
                score.urgency,
                res.stats.file_count,
                res.stats.stale_file_count,
                res.stats.file_count,
                score.reason
            ));
        }

        if !self.duplicate_groups.is_empty() {
            txt.push_str("\n=========================================\n");
            txt.push_str("        DUPLICATE FILES FOUND\n");
            txt.push_str("=========================================\n");
            for group in &self.duplicate_groups {
                txt.push_str(&format!(
                    "Size: {} | Hash: {}\n",
                    format_bytes(group.file_size),
                    group.hash
                ));
                for path in &group.file_paths {
                    txt.push_str(&format!("  - {}\n", path.display()));
                }
                txt.push_str("\n");
            }
        }

        let mut txt_file = File::create("cache-advisor-report.txt")?;
        txt_file.write_all(txt.as_bytes())?;

        Ok(())
    }

    /// Poll the background disk drive scan thread.
    fn poll_disk_scan(&mut self) {
        let rx = match &self.disk_scan_rx {
            Some(r) => r,
            None => return,
        };
        match rx.try_recv() {
            Ok(result) => {
                if let Some(root) = result {
                    self.disk_tree_root = Some(root.clone());
                    self.disk_tree_active = Some(root);
                    self.disk_history.clear();
                    let _ = notify_rust::Notification::new()
                        .summary("Cache Advisor")
                        .body(&format!(
                            "Disk Map scan for {} completed successfully.",
                            self.disk_scan_drive
                        ))
                        .show();
                } else {
                    let _ = notify_rust::Notification::new()
                        .summary("Cache Advisor")
                        .body(&format!(
                            "Disk Map scan for {} failed or returned empty.",
                            self.disk_scan_drive
                        ))
                        .show();
                }
                self.disk_scan_active = false;
                self.disk_scan_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.disk_scan_active = false;
                self.disk_scan_rx = None;
            }
        }
    }

    /// Spawn a thread to scan a drive for disk map visualization.
    fn start_disk_scan(&mut self) {
        let drive = PathBuf::from(&self.disk_scan_drive);
        self.disk_scan_active = true;
        self.disk_tree_root = None;
        self.disk_tree_active = None;
        self.disk_history.clear();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // Prune at 5 MB
            let min_bytes = 5 * 1024 * 1024;
            let res = ca_core::scan_drive(&drive, min_bytes);
            let _ = tx.send(res);
        });
        self.disk_scan_rx = Some(rx);
    }

    fn panel_disk_map(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (active_path, active_size, can_go_up) = if let Some(active) = &self.disk_tree_active {
            (Some(active.path.clone()), Some(active.size), !self.disk_history.is_empty())
        } else {
            (None, None, false)
        };

        ui.vertical(|ui| {
            // Controls row
            ui.horizontal(|ui| {
                ui.label("Drive or Folder Path:");
                ui.text_edit_singleline(&mut self.disk_scan_drive);
                
                if self.disk_scan_active {
                    ui.add_enabled_ui(false, |ui| {
                        let _ = ui.button("Scanning...");
                    });
                } else {
                    if ui.button("⟳ Scan").clicked() {
                        self.start_disk_scan();
                    }
                }

                // If zoomed in, show breadcrumbs and "Go Up"
                if let Some(path) = active_path {
                    ui.separator();
                    if ui.add_enabled(can_go_up, egui::Button::new("⬆ Go Up")).clicked() {
                        if let Some(parent) = self.disk_history.pop() {
                            self.disk_tree_active = Some(parent);
                        }
                    }
                    ui.label(format!("Location: {}", path.display()));
                    ui.separator();
                    if let Some(size) = active_size {
                        ui.label(format!("Size: {}", format_bytes(size)));
                    }
                }
            });

            ui.add_space(8.0);

            // Display loading or the TreeMap
            if self.disk_scan_active {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                    ui.label("Scanning drive in background... Please wait.");
                });
            } else if let Some(active_node) = self.disk_tree_active.clone() {
                let available_size = ui.available_size();
                if available_size.x > 50.0 && available_size.y > 50.0 {
                    let (rect, _) = ui.allocate_exact_size(available_size, egui::Sense::hover());
                    
                    // Call recursive treemap layout with max depth = 2
                    let mut items = Vec::new();
                    layout_treemap(&active_node, rect, 0, 2, &mut items);

                    // Render items
                    for item in &items {
                        let response = ui.allocate_rect(item.rect, egui::Sense::click())
                            .on_hover_text(format!(
                                "{}\nSize: {}\nType: {}",
                                item.node.path.display(),
                                format_bytes(item.node.size),
                                if item.node.is_dir { "Folder" } else { "File" }
                            ));

                        if response.double_clicked() && item.node.is_dir {
                            if let Some(current) = &self.disk_tree_active {
                                self.disk_history.push(current.clone());
                            }
                            self.disk_tree_active = Some(item.node.clone());
                            ctx.request_repaint();
                        }

                        let stroke_color = if response.hovered() {
                            Color32::WHITE
                        } else {
                            Color32::from_gray(50)
                        };
                        
                        ui.painter().rect_filled(item.rect, 2.0, item.color);
                        ui.painter().rect_stroke(item.rect, 2.0, (1.0, stroke_color));

                        if item.rect.width() > 60.0 && item.rect.height() > 20.0 {
                            let label_text = format!("{} ({})", item.node.name, format_bytes(item.node.size));
                            let font_id = egui::FontId::proportional(11.0);
                            let text_color = Color32::WHITE;
                            
                            let max_width = item.rect.width() - 8.0;
                            let galley = ui.painter().layout_no_wrap(label_text, font_id, text_color);
                            
                            let text_pos = egui::pos2(
                                item.rect.min.x + 4.0,
                                item.rect.min.y + 4.0,
                            );
                            
                            if galley.rect.width() <= max_width {
                                ui.painter().galley(text_pos, galley, Color32::WHITE);
                            } else {
                                let name_galley = ui.painter().layout_no_wrap(item.node.name.clone(), egui::FontId::proportional(11.0), text_color);
                                if name_galley.rect.width() <= max_width {
                                    ui.painter().galley(text_pos, name_galley, Color32::WHITE);
                                }
                            }
                        }
                    }
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Enter a drive path above (e.g. C:\\ or D:\\) and click Scan to visualize disk usage.");
                });
            }
        });
    }
}

struct TreeMapItem {
    rect: egui::Rect,
    node: ca_core::DiskNode,
    color: egui::Color32,
}

fn layout_treemap(
    node: &ca_core::DiskNode,
    rect: egui::Rect,
    depth: u32,
    max_depth: u32,
    items: &mut Vec<TreeMapItem>,
) {
    if depth >= max_depth || node.children.is_empty() {
        let color = get_node_color(&node.path, node.is_dir);
        items.push(TreeMapItem {
            rect,
            node: node.clone(),
            color,
        });
        return;
    }

    let total_size = node.size;
    if total_size == 0 {
        return;
    }

    let horizontal = rect.width() > rect.height();
    let mut current_offset = if horizontal { rect.min.x } else { rect.min.y };
    let rect_size = if horizontal { rect.width() } else { rect.height() };

    for child in &node.children {
        let ratio = child.size as f32 / total_size as f32;
        let slice_size = rect_size * ratio;
        if slice_size < 1.0 {
            continue;
        }

        let child_rect = if horizontal {
            egui::Rect::from_min_max(
                egui::pos2(current_offset, rect.min.y),
                egui::pos2(current_offset + slice_size, rect.max.y),
            )
        } else {
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, current_offset),
                egui::pos2(rect.max.x, current_offset + slice_size),
            )
        };

        layout_treemap(child, child_rect, depth + 1, max_depth, items);
        current_offset += slice_size;
    }
}

fn get_node_color(path: &std::path::Path, is_dir: bool) -> egui::Color32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let hash = hasher.finish();

    let hue = (hash % 360) as f32;
    if is_dir {
        hsl_to_color32(hue, 0.45, 0.35)
    } else {
        hsl_to_color32((hue + 120.0) % 360.0, 0.55, 0.45)
    }
}

fn hsl_to_color32(h: f32, s: f32, l: f32) -> egui::Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    egui::Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

fn format_timestamp(secs: u64) -> String {
    if let Ok(ts) = jiff::Timestamp::from_second(secs as i64) {
        ts.to_string()
    } else {
        secs.to_string()
    }
}
