#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use egui::{RichText, TextureHandle, Vec2};
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

mod design;
#[cfg(windows)]
mod native;

fn main() -> anyhow::Result<()> {
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
    let diagnostics = if args.get(1).is_some_and(|s| s == "--render-diagnostics") {
        anyhow::ensure!(
            args.len() == 3,
            "Usage: PrintHelper --render-diagnostics <new-file.json>"
        );
        Some(PathBuf::from(&args[2]))
    } else {
        None
    };
    #[cfg(windows)]
    return native::run(
        move |ctx, wake| Box::new(App::new(ctx, wake, initial_error)),
        diagnostics,
    );
    #[cfg(not(windows))]
    anyhow::bail!("PrintHelper's native software window requires Windows");
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
    preview_failed: bool,
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
    quit_requested: bool,
    downloading: bool,
    update_progress: Option<f32>,
    discard: Option<Discard>,
}

impl App {
    fn new(
        ctx: &egui::Context,
        wake: print_helper::software::BackgroundWake,
        initial_error: Option<String>,
    ) -> Self {
        design::apply(ctx);
        let (settings, error) = match settings::load() {
            Ok(s) => (s, initial_error),
            Err(e) => (
                Settings {
                    startup_updates: false,
                },
                Some(format!("{e:#}")),
            ),
        };
        let mut network = Worker::new(wake.clone());
        let startup_check = settings.startup_updates;
        if startup_check {
            network.submit(Task::CheckUpdate);
        }
        Self {
            project: Project {
                format_version: 1,
                ..Default::default()
            },
            worker: Worker::new(wake),
            network,
            selected: 0,
            sheet: 0,
            revision: 0,
            requested: None,
            preview_revision: None,
            preview_failed: false,
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
            quit_requested: false,
            downloading: false,
            update_progress: None,
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
        self.preview_failed = false;
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
                        Err(e) => {
                            if self.requested == Some(self.revision)
                                && self.preview_revision != Some(self.revision)
                            {
                                self.preview_failed = true;
                                self.preview = None;
                            }
                            self.error = Some(e);
                        }
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
                                self.preview_failed = false;
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
                    self.update_progress = Some(ratio);
                    self.update_status = format!("{message}: {:.0}%", ratio * 100.0);
                }
                Event::Finished(result) => {
                    self.network.busy = false;
                    self.downloading = false;
                    self.update_progress = None;
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
                        Ok(Outcome::Restart) => self.quit_requested = true,
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
            if self.installing {
                ui.disable();
            }
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("PrintHelper")
                        .size(22.0)
                        .strong()
                        .color(design::ACCENT),
                );
                ui.separator();
                ui.add_enabled_ui(!self.worker.busy, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Open layout…").clicked() {
                            self.request_discard(Discard::Open);
                            ui.close();
                        }
                        if ui
                            .button("Save layout as…")
                            .on_hover_text(
                                "Creates a new layout recipe; source documents are unchanged",
                            )
                            .clicked()
                        {
                            self.worker.submit(Task::Save(self.project.clone()));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("New layout").clicked() {
                            self.request_discard(Discard::New);
                            ui.close();
                        }
                    });
                });
                if ui
                    .add_enabled(!self.worker.busy, egui::Button::new("Add files…"))
                    .clicked()
                {
                    self.worker.submit(Task::Import(self.project.pages.len()));
                }
                if ui
                    .add_enabled(
                        !self.undo.is_empty() && !self.worker.busy,
                        egui::Button::new("Undo"),
                    )
                    .clicked()
                    && let Some(old) = self.undo.pop()
                {
                    self.redo.push(std::mem::replace(&mut self.project, old));
                    self.touch();
                }
                if ui
                    .add_enabled(
                        !self.redo.is_empty() && !self.worker.busy,
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                    && let Some(next) = self.redo.pop()
                {
                    self.undo.push(std::mem::replace(&mut self.project, next));
                    self.touch();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = !self.worker.busy && !self.project.pages.is_empty();
                    if design::primary(ui, "Export PDF…", ready)
                        .on_disabled_hover_text("Add a document and wait for preparation to finish")
                        .clicked()
                    {
                        self.error = None;
                        self.worker.submit(Task::Export(self.project.clone()));
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Print…"))
                        .on_hover_text(
                            "Choose a printer and match the preview's paper size and orientation",
                        )
                        .clicked()
                    {
                        self.worker.submit(Task::Print(self.project.clone()));
                    }
                    if ui.button("Settings").clicked() {
                        self.settings_open = true;
                    }
                });
            });
        });
    }
    fn pages_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("pages")
            .exact_width(216.0)
            .show(ctx, |ui| {
                if self.installing {
                    ui.disable();
                }
                ui.heading("Pages");
                design::help(ui, "Select a page to edit it");
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
                    if ui.add_enabled(ready, egui::Button::new(RichText::new("Remove").color(design::ERROR)))
                        .on_hover_text("Remove from this layout. Undo is available; the source document is unchanged").clicked() {
                        self.project.pages.remove(self.selected);
                        self.selected = self
                            .selected
                            .min(self.project.pages.len().saturating_sub(1));
                    }
                });
                egui::ScrollArea::vertical().id_salt("page-list").show_rows(
                    ui,
                    196.0,
                    self.project.pages.len(),
                    |ui, rows| {
                        for i in rows {
                            let page = &self.project.pages[i];
                            let selected = i == self.selected;
                            let fill = if selected {
                                design::SELECTED
                            } else {
                                design::SURFACE
                            };
                            let response = egui::Frame::new()
                                .fill(fill)
                                .stroke(egui::Stroke::new(1.0, if selected { design::ACCENT } else { design::BORDER }))
                                .inner_margin(8.0)
                                .show(ui, |ui| {
                                    ui.set_min_size(Vec2::new(168.0, 180.0));
                                    if let Some((_, texture)) =
                                        self.thumbnails.iter().rev().find(|(p, _)| p == page)
                                    {
                                        ui.add(
                                            egui::Image::new(texture)
                                                .fit_to_exact_size(Vec2::new(110.0, 112.0))
                                                .maintain_aspect_ratio(true),
                                        );
                                    } else {
                                        design::help(ui, "Select to refresh thumbnail");
                                        ui.add_space(82.0);
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
        egui::SidePanel::right("layout").exact_width(272.0).show(ctx, |ui| {
            if self.installing { ui.disable(); }
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Print layout");
                ui.add_enabled_ui(!self.worker.busy, |ui| {
                    let l = &mut self.project.layout;
                    ui.label("Paper size");
                    egui::ComboBox::from_id_salt("paper").width(230.0).selected_text(format!("{:?}", l.paper)).show_ui(ui, |ui| {
                        for p in [Paper::A4, Paper::Letter, Paper::A5] { ui.selectable_value(&mut l.paper, p, format!("{p:?}")); }
                    });
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut l.landscape, false, "Portrait");
                        ui.selectable_value(&mut l.landscape, true, "Landscape");
                    });
                    ui.label("Pages per sheet");
                    ui.horizontal(|ui| { for (n, text) in [(1, "1 page"), (2, "2 pages"), (4, "4 pages")] { ui.selectable_value(&mut l.per_sheet, n, text); } });
                    egui::Grid::new("sheet-dimensions").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                        ui.label("Margins"); ui.add(egui::DragValue::new(&mut l.margin_mm).range(0.0..=50.0).speed(0.5).suffix(" mm")); ui.end_row();
                        if l.per_sheet > 1 { ui.label("Page gap"); ui.add(egui::DragValue::new(&mut l.gap_mm).range(0.0..=20.0).speed(0.5).suffix(" mm")); ui.end_row(); }
                    });
                    ui.checkbox(&mut l.grayscale, "Grayscale").on_hover_text("Convert colors to shades of gray");
                    ui.label("Export and print quality");
                    egui::ComboBox::from_id_salt("output-quality").width(230.0)
                        .selected_text(if l.dpi == 150 { "150 DPI · standard" } else { "300 DPI · fine text" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut l.dpi, 150, "150 DPI · standard");
                        ui.selectable_value(&mut l.dpi, 300, "300 DPI · fine text");
                    });
                    ui.separator();
                    ui.label(RichText::new("Selected page").strong());
                    if let Some(page) = self.project.pages.get_mut(self.selected) {
                        design::help(ui, format!("Page {} · {}° rotation", self.selected + 1, page.quarter_turns as u16 * 90));
                        if ui.button("Rotate clockwise").clicked() { page.quarter_turns = (page.quarter_turns + 1) % 4; }
                        let mut apply_crop = None;
                        egui::CollapsingHeader::new("Crop margins").show(ui, |ui| {
                            design::help(ui, "Remove a percentage of each original edge");
                            egui::Grid::new("crop-edges").num_columns(2).show(ui, |ui| {
                                for (i, label) in ["Left", "Top", "Right", "Bottom"].iter().enumerate() {
                                    let mut value = page.crop[i] * 100.0; ui.label(*label);
                                    if ui.add(egui::DragValue::new(&mut value).range(0.0..=45.0).speed(0.5).suffix("%" )).changed() { page.crop[i] = value / 100.0; } ui.end_row();
                                }
                            });
                            if ui.button("Apply crop to all pages").clicked() { apply_crop = Some(page.crop); }
                        });
                        if ui.button("Reset this page").clicked() { page.crop = [0.0; 4]; page.quarter_turns = 0; }
                        if let Some(crop) = apply_crop { for page in &mut self.project.pages { page.crop = crop; } }
                    } else { design::help(ui, "Add files, then select a page"); }
                });
                ui.separator();
                design::help(ui, "PDF export is rasterized.").on_hover_text("Text, links, forms and original vectors are not retained. Preview is lower resolution; export uses the quality selected above.");
                ui.collapsing("Before printing", |ui| {
                    ui.label("In printer Properties, match the paper size and orientation shown here. Use 100% scale and one driver page per sheet.");
                });
            });
        });
    }
    fn preview_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(design::CANVAS).inner_margin(16.0))
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
                design::help(
                    ui,
                    format!("{n} pages → {count} sheets · placement preview"),
                );
                if self.preview_failed && count > 0 {
                    ui.colored_label(design::ERROR, "Preview unavailable for this layout.");
                    if ui.button("Retry preview").clicked() {
                        self.requested = None;
                    }
                } else if self.preview_revision != Some(self.revision) && count > 0 {
                    ui.label("Updating preview…");
                }
                if count == 0 {
                    ui.add_space(80.0);
                    ui.heading("Prepare your next print");
                    design::help(
                        ui,
                        "Add documents, arrange the pages and choose a sheet layout.",
                    );
                    ui.add_space(16.0);
                    if design::primary(ui, "Add files…", !self.worker.busy && !self.installing)
                        .clicked()
                    {
                        self.worker.submit(Task::Import(0));
                    }
                    ui.add_space(8.0);
                    design::help(ui, "PDF · PNG · JPEG · BMP · TIFF · WebP");
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
            egui::Window::new("Settings").open(&mut open).default_width(440.0).resizable(false).show(ctx, |ui| {
                ui.heading("PrintHelper");
                design::help(ui, format!("Installed version {} · Windows x64", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label(RichText::new("Updates").strong());
                ui.add_enabled_ui(!self.worker.busy && !self.installing, |ui| {
                    if ui.checkbox(&mut self.settings.startup_updates, "Check for updates on startup").changed() { self.worker.submit(Task::Settings(self.settings.clone())); }
                });
                if ui.add_enabled(!self.network.busy, egui::Button::new("Check for updates")).clicked() {
                    self.startup_check = false; self.update_status = "Checking…".into(); self.network.submit(Task::CheckUpdate);
                }
                ui.label(&self.update_status);
                if let Some(ratio) = self.update_progress { ui.add(egui::ProgressBar::new(ratio).show_percentage()); }
                ui.hyperlink_to("Project and releases", updater::REPOSITORY);
                if let Some(update) = &self.update {
                    ui.label(format!("Available: {}", update.version));
                    ui.small("Save your layout first. Installation closes and restarts PrintHelper. Settings and source documents are preserved.");
                    if self.dirty { design::help(ui, "Save the layout with File → Save layout as… before updating."); }
                    if self.staged_update.is_none() && design::primary(ui, "Download and install", !self.network.busy && !self.worker.busy && !self.dirty).clicked() {
                        self.downloading = self.network.submit(Task::Download(update.clone()));
                        self.update_status = "Starting download…".into();
                    }
                }
                if let Some((path, digest)) = &self.staged_update
                    && ui.add_enabled(!self.dirty && !self.worker.busy && !self.network.busy, egui::Button::new("Install and restart")).clicked() {
                    self.installing = self.network.submit(Task::Install(path.clone(), digest.clone()));
                    self.update_status = "Verifying and preparing restart…".into();
                }
                if self.downloading && ui.button("Cancel download").clicked() { self.network.cancel.store(true, Ordering::Relaxed); }
                ui.separator();
                ui.collapsing("Supported documents and output", |ui| {
                    ui.label("PDF, PNG, JPEG, BMP, TIFF and WebP. Image inputs use 96 DPI; multi-frame images import the first frame only.");
                    ui.label("Export is a raster PDF at the selected quality. Text search, links, forms and original vectors are not retained. Booklet and automatic duplex are not included.");
                });
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
                                self.quit_requested = true;
                            } else {
                                self.perform(action);
                            }
                        }
                    });
                });
        }
    }
}

impl print_helper::software::DesktopApp for App {
    fn background(&mut self, ctx: &egui::Context) {
        self.receive(ctx);
    }
    fn request_close(&mut self, ctx: &egui::Context) -> bool {
        if self.dirty {
            self.discard = Some(Discard::Close);
            ctx.request_repaint();
            false
        } else {
            true
        }
    }
    fn should_exit(&self) -> bool {
        self.quit_requested
    }
    fn update(&mut self, ctx: &egui::Context) {
        self.toolbar(ctx);
        let before = self.project.clone();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if let Some((text, ratio)) = &self.progress {
                ui.add(egui::ProgressBar::new(*ratio).text(text));
            }
            ui.horizontal_wrapped(|ui| {
                if self.dirty {
                    ui.label(RichText::new("Unsaved layout").strong());
                    ui.separator();
                }
                if let Some(update) = &self.update
                    && ui
                        .link(format!("Update {} available", update.version))
                        .clicked()
                {
                    self.settings_open = true;
                }
                if self.worker.busy {
                    ui.spinner();
                    ui.label("Working…");
                    if ui.button("Cancel").clicked() {
                        self.worker.cancel.store(true, Ordering::Relaxed);
                    }
                }
                ui.add(egui::Label::new(&self.status).truncate())
                    .on_hover_text(&self.status);
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
                    self.preview_failed = false;
                    self.requested = Some(self.revision);
                }
            } else {
                ctx.request_repaint_after(delay);
            }
        }
    }
}
