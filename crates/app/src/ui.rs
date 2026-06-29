//! Main UI state and rendering for all panels.

use ca_actions::{clean_folder, clean_file, run_archive, CleanOutcome};
use ca_core::{
    archive::ArchivePlan,
    classifier::{classify, RiskLevel},
    rules::{CleaningTier, RuleSet},
    scanner::{format_bytes, scan_all, ScanResult},
};
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// Which tab is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Scan,
    Archive,
    Duplicates,
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

    // Duplicates state
    duplicate_groups: Vec<ca_core::DuplicateGroup>,
    dup_scanning: bool,
    dup_rx: Option<Receiver<Vec<ca_core::DuplicateGroup>>>,

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
            cleaning_name: None,
            clean_confirming: false,
            last_clean: None,
            archive_confirming: false,
            archive_result: None,
            archive_error: None,
            external_drive: "E:/".into(),

            duplicate_groups: Vec::new(),
            dup_scanning: false,
            dup_rx: None,

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
        self.scanning = true;
        self.last_clean = None;
        self.archive_result = None;
        self.archive_error = None;
        ctx.set_cursor_icon(egui::CursorIcon::Wait);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = scan_all(&folders, stale_days);
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

        #[cfg(feature = "ai")]
        self.poll_ai();

        // Periodic Scan Scheduler
        if self.settings.scheduler.enabled && !self.scanning && self.scan_rx.is_none() {
            let last = self.last_scan_time.unwrap_or_else(std::time::Instant::now);
            if std::time::Instant::now().duration_since(last) >= std::time::Duration::from_secs(self.settings.scheduler.interval_mins as u64 * 60) {
                log::info!("Triggering scheduled periodic scan...");
                self.start_scan(ctx);
            }
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
                #[cfg(feature = "ai")]
                ui.selectable_value(&mut self.selected_tab, Tab::AskAi, "🤖 Ask AI");
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
                #[cfg(feature = "ai")]
                Tab::AskAi => self.panel_ask_ai(ui, ctx),
            }
        });

        // ── Clean confirmation modal ──
        self.modal_clean(ctx);

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
                        match clean_folder(&path) {
                            Ok(out) => self.last_clean = Some(out),
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

    /// Spawn a thread to scan for duplicates.
    fn start_duplicates_scan(&mut self) {
        let directories: Vec<PathBuf> = self.rules.folders.iter().map(|f| f.path.clone()).collect();
        self.dup_scanning = true;
        self.duplicate_groups.clear();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = ca_core::find_duplicates(&directories);
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
                                        ui.label(path.display().to_string());
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // Enable deletion only if there is more than 1 instance left in the group.
                                            // This prevents deleting the last copy of the file.
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
                            match clean_file(path) {
                                Ok(freed) => {
                                    log::info!("Deleted duplicate file: {}, freed {} bytes", path.display(), freed);
                                    // Remove the path from our local state
                                    self.duplicate_groups[group_idx].file_paths.remove(path_idx);
                                    // If only one (or zero) path remains, remove the whole duplicate group
                                    if self.duplicate_groups[group_idx].file_paths.len() <= 1 {
                                        self.duplicate_groups.remove(group_idx);
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
}
