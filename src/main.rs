#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText, TextureHandle, Vec2};
use print_helper::{
    layout::{Page, Paper, Project},
    settings::{self, Settings},
    updater,
    worker::{Event, Outcome, Task, Worker},
};
use std::{
    path::PathBuf,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args_os().collect();
    if args.get(1).is_some_and(|s| s == "--finish-update") {
        let result = (|| -> anyhow::Result<()> {
            anyhow::ensure!(args.len() == 5, "Invalid update helper arguments");
            updater::finish_install(
                &PathBuf::from(&args[2]),
                args[3]
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid process ID"))?
                    .parse()?,
                args[4]
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid digest"))?,
            )
        })();
        if let Err(error) = result {
            // Safe relaunch/rollback belongs to the validated helper, after parent exit.
            let _ = updater::record_install_error(&format!("{error:#}"));
        }
        return Ok(());
    }
    let initial_error = if args.get(1).is_some_and(|s| s == "--update-error") {
        args.get(2).map(|s| s.to_string_lossy().into_owned())
    } else {
        None
    };
    // A helper can still be exiting when the new app starts. Cleanup is optional and narrow.
    if args.get(1).is_some_and(|s| s == "--updated")
        && let (Some(staged), Ok(current)) = (
            args.get(2),
            std::env::current_exe().and_then(|p| p.canonicalize()),
        )
        && let Ok(staged) = PathBuf::from(staged).canonicalize()
        && staged != current
        && staged.parent() == current.parent()
        && staged
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe"))
        && staged
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with(".printhelper-update-"))
    {
        let _ = std::fs::remove_file(staged);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PrintHelper")
            .with_inner_size([1220.0, 850.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PrintHelper",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, initial_error)))),
    )
}

#[derive(Clone, Copy)]
enum Discard {
    New,
    Open,
    Close,
}

struct App {
    project: Project,
    worker: Worker,
    network: Worker,
    selected: usize,
    sheet: usize,
    revision: u64,
    requested: Option<u64>,
    preview_revision: Option<u64>,
    preview: Option<TextureHandle>,
    thumbnails: Vec<(Page, TextureHandle)>,
    undo: Vec<Project>,
    redo: Vec<Project>,
    dirty: bool,
    changed_at: Instant,
    status: String,
    error: Option<String>,
    progress: Option<(String, f32)>,
    settings: Settings,
    settings_open: bool,
    update: Option<updater::Update>,
    update_status: String,
    startup_check: bool,
    staged_update: Option<(PathBuf, String)>,
    installing: bool,
    discard: Option<Discard>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, initial_error: Option<String>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        let (settings, error) = match settings::load() {
            Ok(s) => (s, initial_error),
            Err(e) => (
                Settings {
                    startup_updates: false,
                },
                Some(format!("{e:#}")),
            ),
        };
        let mut network = Worker::new(cc.egui_ctx.clone());
        let startup_check = settings.startup_updates;
        if startup_check {
            network.submit(Task::CheckUpdate);
        }
        Self {
            project: Project {
                format_version: 1,
                ..Default::default()
            },
            worker: Worker::new(cc.egui_ctx.clone()),
            network,
            selected: 0,
            sheet: 0,
            revision: 0,
            requested: None,
            preview_revision: None,
            preview: None,
            thumbnails: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            changed_at: Instant::now(),
            status: "Add PDF documents or images to begin".into(),
            error,
            progress: None,
            settings,
            settings_open: false,
            update: None,
            update_status: "No check completed".into(),
            startup_check,
            staged_update: None,
            installing: false,
            discard: None,
        }
    }
    fn texture(ctx: &egui::Context, image: image::RgbImage, name: &str) -> TextureHandle {
        ctx.load_texture(
            name,
            egui::ColorImage::from_rgb(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        )
    }
    fn cache(&mut self, ctx: &egui::Context, page: Page, image: image::RgbImage) {
        if self.thumbnails.len() >= 600 {
            drop(self.thumbnails.remove(0));
        }
        self.thumbnails
            .push((page, Self::texture(ctx, image, "Page thumbnail")));
    }
    fn touch(&mut self) {
        self.revision += 1;
        self.changed_at = Instant::now();
        self.dirty = true;
        self.selected = self
            .selected
            .min(self.project.pages.len().saturating_sub(1));
        self.sheet = self.sheet.min(
            self.project
                .layout
                .sheet_count(self.project.pages.len())
                .saturating_sub(1),
        );
    }
    fn checkpoint(&mut self, old: Project) {
        if self.undo.len() >= 64 {
            self.undo.remove(0);
        }
        self.undo.push(old);
        self.redo.clear();
        self.touch();
    }
    fn request_discard(&mut self, action: Discard) {
        if self.dirty {
            self.discard = Some(action);
        } else {
            self.perform(action);
        }
    }
    fn perform(&mut self, action: Discard) {
        match action {
            Discard::New => {
                self.worker.submit(Task::Clear);
                self.project = Project {
                    format_version: 1,
                    ..Default::default()
                };
                self.thumbnails.clear();
                self.undo.clear();
                self.redo.clear();
                self.preview = None;
                self.touch();
                self.dirty = false;
                self.status = "New layout".into();
            }
            Discard::Open => {
                self.worker.submit(Task::Open);
            }
            Discard::Close => {}
        }
    }
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.worker.receiver.try_recv() {
            match event {
                Event::Progress(text, ratio) => self.progress = Some((text, ratio)),
                Event::Finished(result) => {
                    self.worker.busy = false;
                    self.progress = None;
                    match result {
                        Err(e) => self.error = Some(e),
                        Ok(Outcome::Imported(items)) => {
                            let old = self.project.clone();
                            for (page, image) in items {
                                self.project.pages.push(page.clone());
                                self.cache(ctx, page, image);
                            }
                            self.checkpoint(old);
                            self.status = format!("{} pages ready", self.project.pages.len());
                        }
                        Ok(Outcome::Opened(project, images)) => {
                            self.project = project;
                            self.undo.clear();
                            self.redo.clear();
                            self.thumbnails.clear();
                            for (page, image) in self.project.pages.clone().into_iter().zip(images)
                            {
                                self.cache(ctx, page, image);
                            }
                            self.touch();
                            self.dirty = false;
                            self.status = "Project opened; source hashes verified".into();
                        }
                        Ok(Outcome::Preview {
                            revision,
                            image,
                            page,
                            thumbnail,
                        }) => {
                            self.cache(ctx, page, thumbnail);
                            if revision == self.revision {
                                self.preview = Some(Self::texture(ctx, image, "Output sheet"));
                                self.preview_revision = Some(revision);
                            }
                        }
                        Ok(Outcome::Message(message)) => {
                            if message.starts_with("Saved ") {
                                self.dirty = false;
                            }
                            self.status = message;
                        }
                        Ok(_) => {}
                    }
                }
            }
        }
        while let Ok(event) = self.network.receiver.try_recv() {
            match event {
                Event::Progress(message, ratio) => {
                    self.update_status = format!("{message}: {:.0}%", ratio * 100.0);
                }
                Event::Finished(result) => {
                    self.network.busy = false;
                    match result {
                        Ok(Outcome::Checked(update)) => {
                            self.update_status = update
                                .as_ref()
                                .map(|u| format!("Version {} is available", u.version))
                                .unwrap_or_else(|| "Up to date".into());
                            self.update = update;
                        }
                        Ok(Outcome::Downloaded(path, digest)) => {
                            self.staged_update = Some((path, digest));
                            self.update_status =
                                "Download verified. Save your layout, then install and restart."
                                    .into();
                        }
                        Ok(Outcome::Restart) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => {
                            self.installing = false;
                            self.update_status = e.clone();
                            if !self.startup_check {
                                self.error = Some(e);
                            }
                        }
                        _ => {}
                    }
                    self.startup_check = false;
                }
            }
        }
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            if self.installing { ui.disable(); }
            ui.horizontal(|ui| {
                ui.label(RichText::new("PrintHelper").size(24.0).strong().color(Color32::from_rgb(20, 84, 101)));
                ui.separator();
                ui.add_enabled_ui(!self.worker.busy, |ui| {
                    if ui.button("Add files…").clicked() { self.worker.submit(Task::Import(self.project.pages.len())); }
                    if ui.button("Open layout…").clicked() { self.request_discard(Discard::Open); }
                    if ui.button("Save layout…").on_hover_text("Saves a new .phlayout file referencing your unchanged sources").clicked() { self.worker.submit(Task::Save(self.project.clone())); }
                    if ui.button("New").clicked() { self.request_discard(Discard::New); }
                });
                if ui.button("Settings / Updates").clicked() { self.settings_open = true; }
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!self.worker.busy && !self.project.pages.is_empty(), |ui| {
                    if ui.button("Export PDF…").clicked() { self.error = None; self.worker.submit(Task::Export(self.project.clone())); }
                    if ui.button("Print…").on_hover_text("Opens the Windows print dialog. Match paper and orientation in printer Properties").clicked() { self.worker.submit(Task::Print(self.project.clone())); }
                });
                if ui.add_enabled(!self.undo.is_empty() && !self.worker.busy, egui::Button::new("Undo")).clicked()
                    && let Some(old) = self.undo.pop() { self.redo.push(std::mem::replace(&mut self.project, old)); self.touch(); }
                if ui.add_enabled(!self.redo.is_empty() && !self.worker.busy, egui::Button::new("Redo")).clicked()
                    && let Some(next) = self.redo.pop() { self.undo.push(std::mem::replace(&mut self.project, next)); self.touch(); }
                if self.dirty { ui.label("Unsaved layout"); }
                if let Some(update) = &self.update { ui.colored_label(Color32::from_rgb(20, 100, 90), format!("Update {} available — Settings", update.version)); }
            });
        });
    }
    fn pages_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("pages")
            .exact_width(195.0)
            .show(ctx, |ui| {
                if self.installing {
                    ui.disable();
                }
                ui.heading(format!("Pages · {}", self.project.pages.len()));
                ui.horizontal(|ui| {
                    let ready = !self.worker.busy && !self.project.pages.is_empty();
                    if ui
                        .add_enabled(ready && self.selected > 0, egui::Button::new("Up"))
                        .clicked()
                    {
                        self.project.pages.swap(self.selected, self.selected - 1);
                        self.selected -= 1;
                    }
                    if ui
                        .add_enabled(
                            ready && self.selected + 1 < self.project.pages.len(),
                            egui::Button::new("Down"),
                        )
                        .clicked()
                    {
                        self.project.pages.swap(self.selected, self.selected + 1);
                        self.selected += 1;
                    }
                    if ui.add_enabled(ready, egui::Button::new("Delete")).clicked() {
                        self.project.pages.remove(self.selected);
                        self.selected = self
                            .selected
                            .min(self.project.pages.len().saturating_sub(1));
                    }
                });
                egui::ScrollArea::vertical().id_salt("page-list").show_rows(
                    ui,
                    184.0,
                    self.project.pages.len(),
                    |ui, rows| {
                        for i in rows {
                            let page = &self.project.pages[i];
                            let selected = i == self.selected;
                            let fill = if selected {
                                Color32::from_rgb(222, 240, 242)
                            } else {
                                Color32::from_gray(245)
                            };
                            let response = egui::Frame::new()
                                .fill(fill)
                                .inner_margin(6.0)
                                .show(ui, |ui| {
                                    ui.set_min_size(Vec2::new(165.0, 166.0));
                                    if let Some((_, texture)) =
                                        self.thumbnails.iter().rev().find(|(p, _)| p == page)
                                    {
                                        ui.add(
                                            egui::Image::new(texture)
                                                .fit_to_exact_size(Vec2::new(110.0, 125.0))
                                                .maintain_aspect_ratio(true),
                                        );
                                    } else {
                                        ui.label("Preview updates on selection");
                                        ui.add_space(95.0);
                                    }
                                    ui.label(format!(
                                        "{} · source page {}",
                                        i + 1,
                                        page.source_page + 1
                                    ));
                                    ui.add(
                                        egui::Label::new(
                                            page.path
                                                .file_name()
                                                .unwrap_or_default()
                                                .to_string_lossy(),
                                        )
                                        .truncate(),
                                    )
                                    .on_hover_text(page.path.display().to_string());
                                })
                                .response
                                .interact(egui::Sense::click());
                            if response.clicked() {
                                self.selected = i;
                                self.sheet = i / self.project.layout.per_sheet;
                                self.revision += 1;
                                self.changed_at = Instant::now();
                            }
                        }
                    },
                );
            });
    }
    fn layout_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("layout").exact_width(255.0).show(ctx, |ui| {
            if self.installing { ui.disable(); }
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Sheet layout");
                ui.add_enabled_ui(!self.worker.busy, |ui| {
                    let l = &mut self.project.layout;
                    egui::ComboBox::from_label("Paper").selected_text(format!("{:?}", l.paper)).show_ui(ui, |ui| {
                        for p in [Paper::A4, Paper::Letter, Paper::A5] { ui.selectable_value(&mut l.paper, p, format!("{p:?}")); }
                    });
                    ui.checkbox(&mut l.landscape, "Landscape");
                    ui.label("Pages per sheet");
                    ui.horizontal(|ui| { for n in [1, 2, 4] { ui.selectable_value(&mut l.per_sheet, n, n.to_string()); } });
                    ui.add(egui::Slider::new(&mut l.margin_mm, 0.0..=50.0).text("Margins mm"));
                    ui.add(egui::Slider::new(&mut l.gap_mm, 0.0..=20.0).text("Gap mm"));
                    ui.checkbox(&mut l.grayscale, "Grayscale");
                    egui::ComboBox::from_label("Output DPI").selected_text(l.dpi.to_string()).show_ui(ui, |ui| {
                        for dpi in [150, 300] { ui.selectable_value(&mut l.dpi, dpi, dpi.to_string()); }
                    });
                    ui.separator();
                    ui.heading("Selected page");
                    if let Some(page) = self.project.pages.get_mut(self.selected) {
                        ui.label(format!("Page {} · rotation {}°", self.selected + 1, page.quarter_turns as u16 * 90));
                        if ui.button("Rotate clockwise").clicked() { page.quarter_turns = (page.quarter_turns + 1) % 4; }
                        ui.label("Crop original edges (%)");
                        for (i, label) in ["Left", "Top", "Right", "Bottom"].iter().enumerate() {
                            let mut value = page.crop[i] * 100.0;
                            if ui.add(egui::Slider::new(&mut value, 0.0..=45.0).text(*label)).changed() { page.crop[i] = value / 100.0; }
                        }
                        if ui.button("Reset page transforms").clicked() { page.crop = [0.0; 4]; page.quarter_turns = 0; }
                        let crop = page.crop;
                        if ui.button("Apply this crop to all pages").clicked() { for p in &mut self.project.pages { p.crop = crop; } }
                    } else { ui.label("Select a page after adding files"); }
                });
                ui.separator();
                ui.small("Output is a raster PDF. Text/search, links, forms and original vectors are not retained. Preview uses lower resolution; output uses the selected DPI.");
                ui.small("Image inputs use 96 DPI. Multi-frame images import the first frame only.");
                ui.small("Print: match paper and orientation in printer Properties. Use 1 driver page per sheet and 100% scale. No booklet or automatic duplex in this version.");
            });
        });
    }
    fn preview_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(229, 233, 235))
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                let count = self.project.layout.sheet_count(self.project.pages.len());
                ui.horizontal(|ui| {
                    ui.heading("Output preview");
                    if ui
                        .add_enabled(self.sheet > 0, egui::Button::new("Previous"))
                        .clicked()
                    {
                        self.sheet -= 1;
                        self.revision += 1;
                        self.changed_at = Instant::now();
                    }
                    ui.label(format!(
                        "Sheet {} of {}",
                        if count == 0 { 0 } else { self.sheet + 1 },
                        count
                    ));
                    if ui
                        .add_enabled(self.sheet + 1 < count, egui::Button::new("Next"))
                        .clicked()
                    {
                        self.sheet += 1;
                        self.revision += 1;
                        self.changed_at = Instant::now();
                    }
                });
                let n = self.project.pages.len();
                ui.label(format!(
                    "{n} source pages → {count} sheets · {} fewer sheets than 1-up (single sided)",
                    n.saturating_sub(count)
                ));
                if self.preview_revision != Some(self.revision) && count > 0 {
                    ui.label("Updating preview…");
                }
                if count == 0 {
                    ui.add_space(80.0);
                    ui.heading("A clearer layout. Fewer sheets.");
                    ui.label("Add PDF or image files, arrange pages, then export or print.");
                } else if let Some(texture) = &self.preview {
                    ui.add(
                        egui::Image::new(texture)
                            .max_size(ui.available_size())
                            .maintain_aspect_ratio(true),
                    );
                }
            });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.settings_open {
            let mut open = true;
            egui::Window::new("Settings / Updates").open(&mut open).resizable(false).show(ctx, |ui| {
                ui.label(format!("PrintHelper {} · Windows x64", env!("CARGO_PKG_VERSION")));
                ui.label("Updates");
                ui.add_enabled_ui(!self.worker.busy, |ui| {
                    if ui.checkbox(&mut self.settings.startup_updates, "Check for updates on startup").changed() { self.worker.submit(Task::Settings(self.settings.clone())); }
                });
                if ui.add_enabled(!self.network.busy, egui::Button::new("Check for updates")).clicked() {
                    self.startup_check = false; self.update_status = "Checking…".into(); self.network.submit(Task::CheckUpdate);
                }
                ui.label(&self.update_status);
                ui.hyperlink_to("Canonical repository", updater::REPOSITORY);
                if let Some(update) = &self.update {
                    ui.label(format!("Available: {}", update.version));
                    ui.small("Save your layout first. Installation closes and restarts PrintHelper. Settings and source documents are preserved.");
                    if self.staged_update.is_none() && ui.add_enabled(!self.network.busy && !self.worker.busy && !self.dirty, egui::Button::new("Download and install")).clicked() { self.network.submit(Task::Download(update.clone())); }
                }
                if let Some((path, digest)) = &self.staged_update
                    && ui.add_enabled(!self.dirty && !self.worker.busy && !self.network.busy, egui::Button::new("Install and restart")).clicked() {
                    self.installing = self.network.submit(Task::Install(path.clone(), digest.clone()));
                    self.update_status = "Verifying and preparing restart…".into();
                }
                if self.network.busy && ui.button("Cancel download").clicked() { self.network.cancel.store(true, Ordering::Relaxed); }
                ui.separator();
                ui.small("No printer driver or service is installed. Export and printing require explicit actions.");
            });
            self.settings_open = open;
        }
        if let Some(message) = self.error.clone() {
            egui::Window::new("Action could not be completed")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.set_max_width(560.0);
                    ui.label(message);
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
        }
        if let Some(action) = self.discard {
            egui::Window::new("Unsaved layout")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Discard unsaved layout changes? Source documents are unchanged.");
                    ui.horizontal(|ui| {
                        if ui.button("Keep editing").clicked() {
                            self.discard = None;
                        }
                        if ui.button("Discard changes").clicked() {
                            self.discard = None;
                            if matches!(action, Discard::Close) {
                                self.dirty = false;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            } else {
                                self.perform(action);
                            }
                        }
                    });
                });
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.discard = Some(Discard::Close);
        }
        self.toolbar(ctx);
        let before = self.project.clone();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if let Some((text, ratio)) = &self.progress {
                ui.add(egui::ProgressBar::new(*ratio).text(text));
            }
            ui.horizontal(|ui| {
                if self.worker.busy {
                    ui.spinner();
                    ui.label("Working…");
                    if ui.button("Cancel").clicked() {
                        self.worker.cancel.store(true, Ordering::Relaxed);
                    }
                }
                ui.label(&self.status);
            });
        });
        self.pages_panel(ctx);
        self.layout_panel(ctx);
        if self.project != before {
            self.checkpoint(before);
        }
        self.preview_panel(ctx);
        self.dialogs(ctx);
        if !self.project.pages.is_empty()
            && self.requested != Some(self.revision)
            && !self.worker.busy
        {
            let delay = Duration::from_millis(120).saturating_sub(self.changed_at.elapsed());
            if delay.is_zero() {
                if self.worker.submit(Task::Preview {
                    project: self.project.clone(),
                    sheet: self.sheet,
                    selected: self.selected,
                    revision: self.revision,
                }) {
                    self.requested = Some(self.revision);
                }
            } else {
                ctx.request_repaint_after(delay);
            }
        }
    }
}
