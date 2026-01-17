#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use egui::{RichText, TextureHandle, Vec2};
use print_helper::{
    layout::{Page, Paper, Project},
    settings::{self, Settings},
    updater,
    worker::{Event, Outcome, Task, WorkKind, Worker},
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
    if args.get(1).is_some_and(|arg| arg == "--apply-update") {
        anyhow::ensure!(args.len() == 2, "Invalid update helper arguments");
        return rust_auto_updator::apply_update(&updater::config()).map_err(anyhow::Error::msg);
    }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Sheet,
    Page,
}

#[derive(Clone, Copy)]
struct PageDrag {
    from: usize,
    revision: u64,
}

struct App {
    project: Project,
    worker: Worker,
    network: Worker,
    selected: usize,
    inspector: InspectorTab,
    sheet: usize,
    revision: u64,
    requested: Option<u64>,
    preview_revision: Option<u64>,
    preview_failed: bool,
    preview: Option<TextureHandle>,
    thumbnails: Vec<(Page, TextureHandle)>,
    thumbnail_requested: Option<Page>,
    undo: Vec<Project>,
    redo: Vec<Project>,
    dirty: bool,
    changed_at: Instant,
    status: String,
    error: Option<String>,
    progress: Option<(String, f32)>,
    settings: Settings,
    settings_open: bool,
    update: Option<Box<updater::Update>>,
    update_status: String,
    startup_check: bool,
    staged_update: Option<updater::Prepared>,
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
        let (settings, error) = match settings::load() {
            Ok(s) => (s, initial_error),
            Err(e) => (
                Settings {
                    startup_updates: false,
                },
                Some(format!("{e:#}")),
            ),
        };
        Self::with_settings(ctx, wake, settings, error)
    }
    fn with_settings(
        ctx: &egui::Context,
        wake: print_helper::software::BackgroundWake,
        settings: Settings,
        error: Option<String>,
    ) -> Self {
        design::apply(ctx);
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
            inspector: InspectorTab::Sheet,
            sheet: 0,
            revision: 0,
            requested: None,
            preview_revision: None,
            preview_failed: false,
            preview: None,
            thumbnails: Vec::new(),
            thumbnail_requested: None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            changed_at: Instant::now(),
            status: "Ready".into(),
            error,
            progress: None,
            settings,
            settings_open: false,
            update: None,
            update_status: "Not checked yet".into(),
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
        if self.thumbnails.iter().any(|(cached, _)| cached == &page) {
            return;
        }
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
        self.thumbnail_requested = None;
        if self.project.pages.is_empty() {
            self.inspector = InspectorTab::Sheet;
        }
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
    fn select_page(&mut self, index: usize) {
        if index >= self.project.pages.len() {
            return;
        }
        self.inspector = InspectorTab::Page;
        if self.selected != index {
            self.selected = index;
            self.thumbnail_requested = None;
        }
        let sheet = index / self.project.layout.per_sheet;
        if self.sheet != sheet {
            self.sheet = sheet;
            self.revision += 1;
            self.preview_failed = false;
            self.changed_at = Instant::now();
        }
    }
    fn move_selected(&mut self, offset: isize) {
        let Some(target) = self
            .selected
            .checked_add_signed(offset)
            .filter(|&index| index < self.project.pages.len())
        else {
            return;
        };
        self.project.pages.swap(self.selected, target);
        self.selected = target;
        self.sheet = target / self.project.layout.per_sheet;
    }
    fn reorder_page(&mut self, from: usize, insertion: usize) -> bool {
        let len = self.project.pages.len();
        if from >= len || insertion > len {
            return false;
        }
        let target = if insertion > from {
            insertion - 1
        } else {
            insertion
        };
        if target == from {
            self.select_page(from);
            return false;
        }
        let page = self.project.pages.remove(from);
        self.project.pages.insert(target, page);
        self.selected = target;
        self.sheet = target / self.project.layout.per_sheet;
        self.inspector = InspectorTab::Page;
        true
    }

    fn drop_blocked_reason(&self) -> Option<&'static str> {
        if self.installing || self.quit_requested {
            Some("PrintHelper is restarting. Add files after it opens again.")
        } else if self.discard.is_some() {
            Some("Finish the unsaved-layout choice before adding files.")
        } else if self.worker.foreground_busy() {
            Some("Wait for the current action, then drop your files again.")
        } else if self.project.pages.len() >= 500 {
            Some("This layout has 500 pages. Remove some pages before adding files.")
        } else {
            None
        }
    }

    fn handle_file_drop(&mut self, ctx: &egui::Context) {
        let files = ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
        if files.is_empty() {
            return;
        }
        if let Some(reason) = self.drop_blocked_reason() {
            self.status = format!("Files were not added. {reason}");
            return;
        }
        let Some(paths) = files
            .into_iter()
            .map(|file| file.path)
            .collect::<Option<Vec<_>>>()
        else {
            self.error =
                Some("Drop local PDFs or images from File Explorer. No files were added.".into());
            return;
        };
        let count = paths.len();
        if self.worker.submit(Task::ImportPaths {
            existing: self.project.pages.len(),
            paths,
        }) {
            self.error = None;
            self.status = format!("Adding {count} dropped file(s)...");
        } else {
            self.error =
                Some("Could not start the file import. Try dropping the files again.".into());
        }
    }

    fn file_drop_overlay(&self, ctx: &egui::Context) {
        if !ctx.input(|input| !input.raw.hovered_files.is_empty()) {
            return;
        }
        let rect = ctx.available_rect().shrink(8.0);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("file-drop-overlay"),
        ));
        let blocked = self.drop_blocked_reason();
        painter.rect(
            rect,
            10.0,
            egui::Color32::from_rgba_unmultiplied(250, 251, 253, 245),
            egui::Stroke::new(
                2.0,
                if blocked.is_some() {
                    design::ERROR
                } else {
                    design::ACCENT
                },
            ),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center() - Vec2::new(0.0, 18.0),
            egui::Align2::CENTER_CENTER,
            if blocked.is_some() {
                "Cannot add files right now"
            } else {
                "Drop to add files"
            },
            egui::FontId::proportional(21.0),
            design::INK,
        );
        let text = blocked.unwrap_or("PDFs and images. Your originals stay unchanged.");
        let galley = painter.layout(
            text.into(),
            egui::FontId::proportional(14.0),
            design::SECONDARY,
            (rect.width() - 40.0).max(1.0),
        );
        painter.galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y + 10.0,
            ),
            galley,
            design::SECONDARY,
        );
    }

    fn remove_selected(&mut self) {
        if self.selected < self.project.pages.len() {
            self.project.pages.remove(self.selected);
            self.selected = self
                .selected
                .min(self.project.pages.len().saturating_sub(1));
            self.sheet = self.selected / self.project.layout.per_sheet;
            if self.project.pages.is_empty() {
                self.inspector = InspectorTab::Sheet;
            }
        }
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
    fn receive_document(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Progress(text, ratio) => self.progress = Some((text, ratio)),
            Event::Finished(result) => {
                let kind = self.worker.finish();
                self.progress = None;
                match result {
                    Err(e) => {
                        if kind == Some(WorkKind::Preview) {
                            if self.requested != Some(self.revision) {
                                return;
                            }
                            self.preview_failed = true;
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
                        for (page, image) in self.project.pages.clone().into_iter().zip(images) {
                            self.cache(ctx, page, image);
                        }
                        self.touch();
                        self.dirty = false;
                        self.status = "Project opened; source hashes verified".into();
                    }
                    Ok(Outcome::Preview { revision, image }) => {
                        if revision == self.revision {
                            self.preview_failed = false;
                            self.preview = Some(Self::texture(ctx, image, "Output sheet"));
                            self.preview_revision = Some(revision);
                        }
                    }
                    Ok(Outcome::Thumbnail { page, image }) => self.cache(ctx, page, image),
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
    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.worker.receiver.try_recv() {
            self.receive_document(ctx, event);
        }
        while let Ok(event) = self.network.receiver.try_recv() {
            match event {
                Event::Progress(message, ratio) => {
                    self.update_progress = Some(ratio);
                    self.update_status = format!("{message}: {:.0}%", ratio * 100.0);
                }
                Event::Finished(result) => {
                    self.network.finish();
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
                        Ok(Outcome::Downloaded(prepared)) => {
                            self.staged_update = Some(prepared);
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
        egui::TopBottomPanel::top("toolbar")
            .frame(egui::Frame::new().fill(design::SURFACE).inner_margin(egui::Margin::symmetric(16, 8)))
            .exact_height(56.0).show(ctx, |ui| {
            if self.installing {
                ui.disable();
            }
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("PrintHelper")
                        .size(22.0)
                        .strong()
                        .color(design::INK),
                );
                ui.separator();
                ui.add_enabled_ui(!self.worker.foreground_busy(), |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Open layout...").clicked() {
                            self.request_discard(Discard::Open);
                            ui.close();
                        }
                        if ui
                            .button("Save layout as...")
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
                    .add_enabled(
                        !self.undo.is_empty() && !self.worker.foreground_busy(),
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
                        !self.redo.is_empty() && !self.worker.foreground_busy(),
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                    && let Some(next) = self.redo.pop()
                {
                    self.undo.push(std::mem::replace(&mut self.project, next));
                    self.touch();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let ready = !self.worker.foreground_busy() && !self.project.pages.is_empty();
                    if design::primary(ui, "Save PDF...", ready)
                        .on_disabled_hover_text("Add a document and wait for preparation to finish")
                        .on_hover_text("Save a new PDF. Pages become images; text will not be selectable. Your source files stay unchanged.")
                        .clicked()
                    {
                        self.error = None;
                        self.worker.submit(Task::Export(self.project.clone()));
                    }
                    if ui
                        .add_enabled(ready, egui::Button::new("Print..."))
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
    fn pages_panel(&mut self, ctx: &egui::Context) -> Option<egui::Rect> {
        egui::SidePanel::left("pages")
            .exact_width(216.0)
            .frame(egui::Frame::new().fill(design::PANEL).inner_margin(12))
            .show(ctx, |ui| {
                if self.installing {
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    ui.heading("Pages");
                    design::help(ui, self.project.pages.len().to_string());
                });
                ui.add_space(4.0);
                if self.project.pages.is_empty() {
                    if egui::DragAndDrop::has_payload_of_type::<PageDrag>(ctx) {
                        egui::DragAndDrop::clear_payload(ctx);
                    }
                    design::help(ui, "Your pages will appear here.");
                    return None;
                }
                if ui
                    .add_enabled(
                        !self.worker.foreground_busy(),
                        egui::Button::new("Add files...")
                            .min_size(Vec2::new(ui.available_width(), 36.0)),
                    )
                    .on_hover_text("Add PDFs or images to this layout")
                    .clicked()
                {
                    self.worker.submit(Task::Import(self.project.pages.len()));
                }
                ui.horizontal(|ui| {
                    let ready = !self.worker.foreground_busy();
                    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    if ui
                        .add_enabled(
                            ready && self.selected > 0,
                            egui::Button::new("Move up").min_size(Vec2::new(width, 34.0)),
                        )
                        .clicked()
                    {
                        self.move_selected(-1);
                    }
                    if ui
                        .add_enabled(
                            ready && self.selected + 1 < self.project.pages.len(),
                            egui::Button::new("Move down").min_size(Vec2::new(width, 34.0)),
                        )
                        .clicked()
                    {
                        self.move_selected(1);
                    }
                });
                design::help(ui, RichText::new("Drag pages to reorder").small());
                let can_reorder = !self.worker.foreground_busy()
                    && !self.installing
                    && self.discard.is_none()
                    && !self.settings_open
                    && self.error.is_none();
                let output = egui::ScrollArea::vertical()
                    .id_salt("page-list")
                    .auto_shrink([false, false])
                    .show_rows(
                        ui,
                        design::PAGE_ROW_HEIGHT,
                        self.project.pages.len(),
                        |ui, rows| {
                            for index in rows {
                                let page = &self.project.pages[index];
                                let texture = self
                                    .thumbnails
                                    .iter()
                                    .rev()
                                    .find(|(p, _)| p == page)
                                    .map(|(_, t)| t);
                                let response = design::page_card(
                                    ui,
                                    index,
                                    page,
                                    index == self.selected,
                                    texture,
                                    can_reorder,
                                );
                                if can_reorder {
                                    response.dnd_set_drag_payload(PageDrag {
                                        from: index,
                                        revision: self.revision,
                                    });
                                }
                                if response.clicked() {
                                    self.select_page(index);
                                }
                            }
                        },
                    );
                if let Some(payload) = egui::DragAndDrop::payload::<PageDrag>(ctx) {
                    if !can_reorder
                        || payload.revision != self.revision
                        || payload.from >= self.project.pages.len()
                        || !ctx.input(|input| input.focused)
                    {
                        egui::DragAndDrop::clear_payload(ctx);
                    } else if let Some(pointer) = ctx.pointer_interact_pos() {
                        let pitch = design::PAGE_ROW_HEIGHT + ui.spacing().item_spacing.y;
                        if let Some((insertion, marker)) = design::page_drop_slot(
                            pointer,
                            output.inner_rect,
                            output.state.offset.y,
                            pitch,
                            self.project.pages.len(),
                        ) {
                            ui.painter().with_clip_rect(output.inner_rect).line_segment(
                                [
                                    egui::pos2(output.inner_rect.left(), marker),
                                    egui::pos2(output.inner_rect.right(), marker),
                                ],
                                egui::Stroke::new(3.0, design::ACCENT),
                            );
                            if ctx.input(|input| input.pointer.any_released()) {
                                egui::DragAndDrop::clear_payload(ctx);
                                self.reorder_page(payload.from, insertion);
                                ctx.request_repaint();
                            } else if let Some(offset) = design::page_drag_scroll(
                                pointer,
                                output.inner_rect,
                                output.state.offset.y,
                                output.content_size.y,
                                ctx.input(|input| input.stable_dt),
                            ) {
                                let mut state = output.state;
                                state.offset.y = offset;
                                state.store(ctx, output.id);
                                ctx.request_repaint_after(Duration::from_millis(16));
                            }
                        }
                    }
                }
                Some(output.inner_rect)
            })
            .inner
    }
    fn layout_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("layout")
            .exact_width(288.0)
            .frame(egui::Frame::new().fill(design::SURFACE).inner_margin(16))
            .show(ctx, |ui| {
                if self.installing {
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                    if design::choice(ui, "Sheet", self.inspector == InspectorTab::Sheet, width)
                        .clicked()
                    {
                        self.inspector = InspectorTab::Sheet;
                    }
                    ui.add_enabled_ui(!self.project.pages.is_empty(), |ui| {
                        if design::choice(ui, "Page", self.inspector == InspectorTab::Page, width)
                            .clicked()
                        {
                            self.inspector = InspectorTab::Page;
                        }
                    });
                });
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .id_salt("inspector")
                    .show(ui, |ui| {
                        ui.add_enabled_ui(!self.worker.foreground_busy(), |ui| {
                            match self.inspector {
                                InspectorTab::Sheet => self.sheet_controls(ui),
                                InspectorTab::Page => self.page_controls(ui),
                            }
                        });
                    });
            });
    }
    fn sheet_controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("Sheet layout");
        let layout = &mut self.project.layout;
        design::section_label(ui, "Paper size");
        egui::ComboBox::from_id_salt("paper")
            .width(ui.available_width())
            .selected_text(match layout.paper {
                Paper::A4 => "A4 - 210 x 297 mm",
                Paper::A5 => "A5 - 148 x 210 mm",
                Paper::Letter => "Letter - 8.5 x 11 in",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut layout.paper, Paper::A4, "A4 - 210 x 297 mm");
                ui.selectable_value(&mut layout.paper, Paper::A5, "A5 - 148 x 210 mm");
                ui.selectable_value(&mut layout.paper, Paper::Letter, "Letter - 8.5 x 11 in");
            });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            if design::choice(ui, "Portrait", !layout.landscape, width).clicked() {
                layout.landscape = false;
            }
            if design::choice(ui, "Landscape", layout.landscape, width).clicked() {
                layout.landscape = true;
            }
        });
        design::section_label(ui, "Pages per sheet");
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
            for (count, label) in [(1, "1 page"), (2, "2 pages"), (4, "4 pages")] {
                if design::choice(ui, label, layout.per_sheet == count, width).clicked() {
                    layout.per_sheet = count;
                }
            }
        });
        design::section_label(ui, "Color mode");
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            if design::choice(ui, "Color", !layout.grayscale, width).clicked() {
                layout.grayscale = false;
            }
            if design::choice(ui, "Grayscale", layout.grayscale, width)
                .on_hover_text("Convert colors to shades of gray")
                .clicked()
            {
                layout.grayscale = true;
            }
        });
        ui.add_space(16.0);
        egui::CollapsingHeader::new("Margins and spacing").show(ui, |ui| {
            egui::Grid::new("sheet-dimensions")
                .num_columns(2)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Outer margins");
                    ui.add(
                        egui::DragValue::new(&mut layout.margin_mm)
                            .range(0.0..=50.0)
                            .speed(0.5)
                            .suffix(" mm"),
                    );
                    ui.end_row();
                    if layout.per_sheet > 1 {
                        ui.label("Between pages");
                        ui.add(
                            egui::DragValue::new(&mut layout.gap_mm)
                                .range(0.0..=20.0)
                                .speed(0.5)
                                .suffix(" mm"),
                        );
                        ui.end_row();
                    }
                });
        });
        egui::CollapsingHeader::new("Output quality").show(ui, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                if design::choice(ui, "Standard", layout.dpi == 150, width).clicked() {
                    layout.dpi = 150;
                }
                if design::choice(ui, "High", layout.dpi == 300, width).clicked() {
                    layout.dpi = 300;
                }
            });
            design::help(
                ui,
                if layout.dpi == 150 {
                    "150 DPI. Smaller PDF files."
                } else {
                    "300 DPI. Sharper small text."
                },
            );
        });
        ui.add_space(12.0);
        design::help(ui, "PDFs are saved as images.").on_hover_text(
            "Text in the saved PDF will not be selectable. Your source files stay unchanged.",
        );
    }
    fn page_controls(&mut self, ui: &mut egui::Ui) {
        let Some(page) = self.project.pages.get_mut(self.selected) else {
            design::help(ui, "Choose a page from the list.");
            return;
        };
        ui.heading(format!("Page {}", self.selected + 1));
        ui.add(
            egui::Label::new(
                RichText::new(page.path.file_name().unwrap_or_default().to_string_lossy())
                    .color(design::SECONDARY),
            )
            .truncate(),
        )
        .on_hover_text(page.path.display().to_string());
        design::section_label(ui, "Rotate");
        ui.horizontal(|ui| {
            let width = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
            if ui
                .add_sized([width, 36.0], egui::Button::new("Rotate left"))
                .clicked()
            {
                page.quarter_turns = (page.quarter_turns + 3) % 4;
            }
            if ui
                .add_sized([width, 36.0], egui::Button::new("Rotate right"))
                .clicked()
            {
                page.quarter_turns = (page.quarter_turns + 1) % 4;
            }
        });
        design::help(
            ui,
            format!("Rotation: {} degrees", page.quarter_turns as u16 * 90),
        );
        ui.add_space(16.0);
        let mut apply_crop = None;
        egui::CollapsingHeader::new("Crop edges").show(ui, |ui| {
            design::help(ui, "Trim a percentage from each edge.");
            egui::Grid::new("crop-edges").num_columns(2).show(ui, |ui| {
                for (index, label) in ["Left", "Top", "Right", "Bottom"].iter().enumerate() {
                    let mut value = page.crop[index] * 100.0;
                    ui.label(*label);
                    if ui
                        .add(
                            egui::DragValue::new(&mut value)
                                .range(0.0..=45.0)
                                .speed(0.5)
                                .suffix("%"),
                        )
                        .changed()
                    {
                        page.crop[index] = value / 100.0;
                    }
                    ui.end_row();
                }
            });
            if ui
                .button("Apply crop to all pages")
                .on_hover_text("Copies these crop values. Undo is available.")
                .clicked()
            {
                apply_crop = Some(page.crop);
            }
        });
        ui.add_space(12.0);
        let changed = page.quarter_turns != 0 || page.crop != [0.0; 4];
        if ui
            .add_enabled(changed, egui::Button::new("Reset page edits"))
            .clicked()
        {
            page.crop = [0.0; 4];
            page.quarter_turns = 0;
        }
        if let Some(crop) = apply_crop {
            for page in &mut self.project.pages {
                page.crop = crop;
            }
        }
        ui.add_space(20.0);
        ui.separator();
        if ui
            .add(
                egui::Button::new(RichText::new("Remove page").color(design::ERROR))
                    .min_size(Vec2::new(ui.available_width(), 36.0)),
            )
            .on_hover_text(
                "Remove from this layout. Undo restores the page; the source file stays unchanged.",
            )
            .clicked()
        {
            self.remove_selected();
        }
    }
    fn preview_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(design::CANVAS).inner_margin(16.0))
            .show(ctx, |ui| {
                let count = self.project.layout.sheet_count(self.project.pages.len());
                if count == 0 {
                    ui.add_space((ui.available_height() * 0.22).max(16.0));
                    ui.vertical_centered(|ui| {
                        ui.heading("Start with a PDF or image");
                        ui.add_space(8.0);
                        ui.add(egui::Label::new(RichText::new("Arrange your pages, choose a sheet layout,\nthen save a PDF or print.").color(design::SECONDARY)).wrap());
                        ui.add_space(20.0);
                        if design::primary(ui, "Add files...", !self.worker.foreground_busy() && !self.installing).clicked() {
                            self.worker.submit(Task::Import(0));
                        }
                        ui.add_space(12.0);
                        design::help(ui, RichText::new("PDF, PNG, JPEG, BMP, TIFF and WebP").small());
                        design::help(ui, RichText::new("Your source files stay unchanged.").small());
                    });
                    return;
                }
                ui.heading("Preview");
                let state = if self.preview_failed {
                    design::PreviewState::Failed
                } else if self.preview_revision != Some(self.revision) {
                    design::PreviewState::Updating
                } else {
                    design::PreviewState::Ready
                };
                if design::preview_status(ui, self.project.pages.len(), count, state) { self.requested = None; }
                design::sheet_stage(ui, self.preview.as_ref(), if self.preview_failed {
                    "Preview unavailable. Try again above."
                } else { "Preparing your preview..." });
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center), |ui| {
                    if ui.add_enabled(self.sheet > 0, egui::Button::new("Previous")).clicked() {
                        self.sheet -= 1; self.revision += 1;
                        self.preview_failed = false; self.changed_at = Instant::now();
                    }
                    ui.label(format!("Sheet {} of {}", self.sheet + 1, count));
                    if ui.add_enabled(self.sheet + 1 < count, egui::Button::new("Next")).clicked() {
                        self.sheet += 1; self.revision += 1;
                        self.preview_failed = false; self.changed_at = Instant::now();
                    }
                });
            });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.settings_open {
            let mut open = true;
            egui::Window::new("Settings").open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .default_width(440.0).max_height((ctx.content_rect().height() - 64.0).max(240.0))
                .resizable(false).collapsible(false).vscroll(true).show(ctx, |ui| {
                    ui.heading("Updates");
                    design::help(ui, format!("PrintHelper {} for Windows x64", env!("CARGO_PKG_VERSION")));
                    ui.add_space(12.0);
                    ui.add_enabled_ui(!self.worker.foreground_busy() && !self.installing, |ui| {
                        if ui.checkbox(&mut self.settings.startup_updates, "Check for updates on startup").changed() {
                            self.worker.submit(Task::Settings(self.settings.clone()));
                        }
                    });
                    if ui.add_enabled(!self.network.is_busy(), egui::Button::new("Check for updates")).clicked() {
                        self.startup_check = false;
                        self.update_status = "Checking...".into();
                        self.network.submit(Task::CheckUpdate);
                    }
                    design::help(ui, &self.update_status);
                    if let Some(ratio) = self.update_progress {
                        ui.add(egui::ProgressBar::new(ratio).show_percentage());
                    }
                    if let Some(update) = &self.update {
                        ui.add_space(12.0);
                        ui.label(RichText::new(format!("Version {} is available", update.version)).strong());
                        design::help(ui, "Installing closes and restarts PrintHelper. Your settings and source files are kept.");
                        if self.dirty { design::help(ui, "Save your layout before updating: File > Save layout as..."); }
                        if self.staged_update.is_none() && design::primary(ui, "Download and install",
                            !self.network.is_busy() && !self.worker.foreground_busy() && !self.dirty).clicked() {
                            self.downloading = self.network.submit(Task::Download(update.clone()));
                            self.update_status = "Starting download...".into();
                        }
                    }
                    if self.staged_update.is_some()
                        && design::primary(ui, "Install and restart",
                            !self.dirty && !self.worker.foreground_busy() && !self.network.is_busy()).clicked() {
                        if let Some(prepared) = self.staged_update.take() {
                            self.installing = self.network.submit(Task::Install(prepared));
                        }
                        self.update_status = "Preparing to restart...".into();
                    }
                    if self.downloading && ui.button("Cancel download").clicked() {
                        self.network.cancel.store(true, Ordering::Relaxed);
                    }
                    ui.add_space(16.0);
                    ui.separator();
                    ui.collapsing("Supported files", |ui| {
                        ui.label("PDF, PNG, JPEG, BMP, TIFF and WebP. Multi-frame images use the first frame.");
                        ui.label("Saved PDFs contain page images. Text will not be selectable or searchable.");
                    });
                    ui.collapsing("Printing tips", |ui| {
                        ui.label("Match the printer's paper size and orientation to your sheet layout.");
                        ui.label("In the printer's own settings, use 100% scale and one page per sheet.");
                    });
                    ui.hyperlink_to("PrintHelper project and releases", updater::REPOSITORY);
                });
            self.settings_open = open;
        }
        if let Some(message) = self.error.clone() {
            egui::Window::new("Could not finish this action")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .default_width(440.0)
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            ui.label(message);
                        });
                    ui.add_space(8.0);
                    if design::primary(ui, "Close", true).clicked() {
                        self.error = None;
                    }
                });
        }
        if let Some(action) = self.discard {
            egui::Window::new("Unsaved layout")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Discard unsaved layout changes? Source documents are unchanged.");
                    ui.horizontal(|ui| {
                        if design::primary(ui, "Keep editing", true).clicked() {
                            self.discard = None;
                        }
                        if ui
                            .button(RichText::new("Discard changes").color(design::ERROR))
                            .clicked()
                        {
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

impl App {
    fn workspace(&mut self, ctx: &egui::Context) -> Option<egui::Rect> {
        self.handle_file_drop(ctx);
        self.toolbar(ctx);
        let before = self.project.clone();
        design::status_panel(ctx, |ui| {
            if let Some((text, ratio)) = &self.progress {
                ui.add_sized([160.0, 20.0], egui::ProgressBar::new(*ratio).text(text));
            }
            if self.dirty {
                ui.label(RichText::new("Layout not saved").strong())
                    .on_hover_text(
                        "Save the arrangement with File > Save layout as. Saved PDFs are separate.",
                    );
                ui.separator();
            }
            if let Some(update) = &self.update
                && ui
                    .link(format!("Update {} available", update.version))
                    .clicked()
            {
                self.settings_open = true;
            }
            if self.worker.foreground_busy() {
                ui.label("Working...");
                if ui.button("Cancel").clicked() {
                    self.worker.cancel.store(true, Ordering::Relaxed);
                }
            }
            ui.add(egui::Label::new(&self.status).truncate())
                .on_hover_text(&self.status);
        });
        let page_list = self.pages_panel(ctx);
        self.layout_panel(ctx);
        if self.project != before {
            self.checkpoint(before);
        }
        self.preview_panel(ctx);
        self.dialogs(ctx);
        self.file_drop_overlay(ctx);
        if !self.project.pages.is_empty()
            && self.requested != Some(self.revision)
            && !self.worker.is_busy()
        {
            let delay = Duration::from_millis(120).saturating_sub(self.changed_at.elapsed());
            if delay.is_zero() {
                if self.worker.submit(Task::Preview {
                    project: self.project.clone(),
                    sheet: self.sheet,
                    revision: self.revision,
                }) {
                    self.preview_failed = false;
                    self.requested = Some(self.revision);
                }
            } else {
                ctx.request_repaint_after(delay);
            }
        }
        if !self.worker.is_busy()
            && self.preview_revision == Some(self.revision)
            && let Some(page) = self.project.pages.get(self.selected)
            && self.thumbnail_requested.as_ref() != Some(page)
            && !self.thumbnails.iter().any(|(cached, _)| cached == page)
            && self.worker.submit(Task::Thumbnail(page.clone()))
        {
            self.thumbnail_requested = Some(page.clone());
        }
        page_list
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
        self.workspace(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use print_helper::software::{BackgroundWake, DesktopApp};

    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    fn drag_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        time: f64,
    ) -> egui::Rect {
        let mut viewport = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1220.0, 850.0),
                )),
                events,
                time: Some(time),
                ..Default::default()
            },
            |ctx| {
                viewport = app.workspace(ctx);
            },
        );
        viewport.unwrap()
    }

    #[test]
    fn page_drag_commits_once_with_selection_and_one_undo_snapshot() {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        let before = app.project.clone();
        let textures: Vec<_> = app.thumbnails.iter().map(|(_, t)| t.id()).collect();
        let mut viewport = egui::Rect::NOTHING;
        for frame in 0..3 {
            viewport = drag_frame(&mut app, &ctx, vec![], frame as f64 * 0.016);
        }
        let source = viewport.min + Vec2::new(80.0, 50.0);
        let target = viewport.min + Vec2::new(80.0, 3.0 * 108.0 + 80.0);
        drag_frame(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(source),
                pointer_button(source, true),
            ],
            0.048,
        );
        drag_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(source + Vec2::new(0.0, 12.0))],
            0.064,
        );
        drag_frame(
            &mut app,
            &ctx,
            vec![egui::Event::PointerMoved(target)],
            0.080,
        );
        assert!(egui::DragAndDrop::has_payload_of_type::<PageDrag>(&ctx));
        assert_eq!(app.project, before);
        assert!(app.undo.is_empty());
        drag_frame(&mut app, &ctx, vec![pointer_button(target, false)], 0.096);
        assert_eq!(
            app.project.pages,
            vec![
                before.pages[1].clone(),
                before.pages[2].clone(),
                before.pages[3].clone(),
                before.pages[0].clone()
            ]
        );
        assert_eq!((app.selected, app.sheet), (3, 1));
        assert_eq!(app.undo, vec![before]);
        assert!(app.dirty);
        assert_eq!(
            app.thumbnails
                .iter()
                .map(|(_, t)| t.id())
                .collect::<Vec<_>>(),
            textures
        );
        assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    }

    #[test]
    fn cancelled_outside_and_same_position_drags_do_not_change_order_or_undo() {
        for scenario in 0..4 {
            let ctx = egui::Context::default();
            let mut app = fixture(&ctx);
            let before = app.project.clone();
            let mut viewport = egui::Rect::NOTHING;
            for frame in 0..3 {
                viewport = drag_frame(&mut app, &ctx, vec![], frame as f64 * 0.016);
            }
            let source = viewport.min + Vec2::new(80.0, 30.0);
            let target = match scenario {
                0 | 3 => viewport.min + Vec2::new(80.0, 260.0),
                1 => egui::pos2(-10.0, -10.0),
                _ => viewport.min + Vec2::new(80.0, 70.0),
            };
            drag_frame(
                &mut app,
                &ctx,
                vec![
                    egui::Event::PointerMoved(source),
                    pointer_button(source, true),
                ],
                0.048,
            );
            drag_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(source + Vec2::new(0.0, 12.0))],
                0.064,
            );
            drag_frame(
                &mut app,
                &ctx,
                vec![egui::Event::PointerMoved(target)],
                0.080,
            );
            if scenario == 3 {
                app.select_page(3);
            }
            if scenario == 0 {
                drag_frame(
                    &mut app,
                    &ctx,
                    vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Default::default(),
                    }],
                    0.096,
                );
            }
            drag_frame(&mut app, &ctx, vec![pointer_button(target, false)], 0.112);
            assert_eq!(app.project, before);
            assert!(app.undo.is_empty());
            assert!(!app.dirty);
            assert!(!egui::DragAndDrop::has_any_payload(&ctx));
        }
    }

    #[test]
    fn virtual_and_blocked_file_drops_leave_the_project_unchanged() {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        let before = app.project.clone();
        let _ = ctx.run(
            egui::RawInput {
                dropped_files: vec![egui::DroppedFile {
                    name: "virtual.png".into(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            |ctx| app.handle_file_drop(ctx),
        );
        assert!(app.error.as_ref().unwrap().contains("local"));
        assert!(!app.worker.is_busy());
        app.error = None;
        app.discard = Some(Discard::Close);
        let _ = ctx.run(
            egui::RawInput {
                dropped_files: vec![egui::DroppedFile {
                    path: Some("not-read.png".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            |ctx| app.handle_file_drop(ctx),
        );
        assert!(app.status.contains("not added"));
        assert_eq!(app.project, before);
        assert!(!app.worker.is_busy());
        app.discard = None;
        assert!(app.worker.submit(Task::Clear));
        let _ = ctx.run(
            egui::RawInput {
                dropped_files: vec![egui::DroppedFile {
                    path: Some("not-read.png".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            |ctx| app.handle_file_drop(ctx),
        );
        assert!(app.status.contains("not added"));
        assert_eq!(app.project, before);
    }

    #[test]
    fn native_drop_input_imports_once_across_layout_passes_without_a_picker() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".test-output")
            .join(format!("ui-drop-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("generated.png");
        let second_path = root.join("second.png");
        image::RgbImage::from_pixel(8, 12, image::Rgb([20, 50, 120]))
            .save(&path)
            .unwrap();
        image::RgbImage::from_pixel(8, 12, image::Rgb([150, 30, 20]))
            .save(&second_path)
            .unwrap();
        let original = std::fs::read(&path).unwrap();
        let ctx = egui::Context::default();
        let mut app = App::with_settings(
            &ctx,
            BackgroundWake::new(|| {}),
            Settings {
                startup_updates: false,
            },
            None,
        );
        let mut passes = 0;
        let _ = ctx.run(
            egui::RawInput {
                dropped_files: vec![
                    egui::DroppedFile {
                        path: Some(path.clone()),
                        ..Default::default()
                    },
                    egui::DroppedFile {
                        path: Some(second_path.clone()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                passes += 1;
                app.handle_file_drop(ctx);
                if passes == 1 {
                    ctx.request_discard("Test an extra layout pass");
                }
            },
        );
        assert!(
            passes >= 2,
            "The fixture must exercise a repeated layout pass"
        );
        assert!(!app.status.contains("not added"));
        while app.worker.is_busy() {
            let event = app
                .worker
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            app.receive_document(&ctx, event);
        }
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.project.pages.len(), 2);
        assert_eq!(app.undo.len(), 1);
        assert_eq!(app.project.pages[0].path, path.canonicalize().unwrap());
        assert_eq!(
            app.project.pages[1].path,
            second_path.canonicalize().unwrap()
        );
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.handle_file_drop(ctx));
        assert!(!app.worker.is_busy());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(second_path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    fn fixture(ctx: &egui::Context) -> App {
        // Inject settings: no real settings read, network, native window or dialog.
        let mut app = App::with_settings(
            ctx,
            BackgroundWake::new(|| {}),
            Settings {
                startup_updates: false,
            },
            None,
        );
        for index in 0..4 {
            let page = Page {
                path: PathBuf::from(format!("synthetic-{index}.png")),
                sha256: "0".repeat(64),
                source_page: 0,
                width_pt: 60.0,
                height_pt: 80.0,
                crop: [0.0; 4],
                quarter_turns: 0,
            };
            app.project.pages.push(page.clone());
            app.cache(ctx, page, image::RgbImage::new(6, 8));
        }
        app.revision = 7;
        app.requested = Some(7);
        app.preview_revision = Some(7);
        app.preview = Some(App::texture(ctx, image::RgbImage::new(60, 80), "sheet"));
        app
    }

    #[test]
    fn cached_selection_within_a_sheet_does_not_dispatch_work_or_replace_textures() {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        let sheet_id = app.preview.as_ref().unwrap().id();
        let thumbnail_ids: Vec<_> = app.thumbnails.iter().map(|(_, t)| t.id()).collect();
        for index in [0, 0, 1, 1, 0] {
            app.select_page(index);
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1220.0, 850.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.update(ctx),
            );
            assert_eq!(app.revision, 7);
            assert_eq!(app.requested, Some(7));
            assert_eq!(app.preview_revision, Some(7));
            assert_eq!(app.preview.as_ref().unwrap().id(), sheet_id);
            assert!(!app.worker.is_busy());
            assert!(!app.network.is_busy());
            assert!(!app.dirty);
            assert_eq!(
                app.thumbnails
                    .iter()
                    .map(|(_, t)| t.id())
                    .collect::<Vec<_>>(),
                thumbnail_ids
            );
        }
    }

    #[test]
    fn sheet_navigation_invalidates_once_and_duplicate_thumbnails_keep_their_texture() {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        let id = app.thumbnails[0].1.id();
        app.cache(
            &ctx,
            app.project.pages[0].clone(),
            image::RgbImage::new(6, 8),
        );
        assert_eq!(app.thumbnails.len(), 4);
        assert_eq!(app.thumbnails[0].1.id(), id);
        app.select_page(2);
        assert_eq!((app.sheet, app.revision), (1, 8));
        app.select_page(3);
        app.select_page(3);
        assert_eq!((app.sheet, app.revision), (1, 8));
        app.select_page(0);
        assert_eq!((app.sheet, app.revision), (0, 9));
        assert_eq!(app.preview_revision, Some(7));
        assert!(!app.dirty);
    }

    #[test]
    fn document_and_page_views_fit_minimum_window_without_dispatching_work() {
        for (width, height) in [(960.0, 640.0), (1220.0, 850.0)] {
            for scale in [1.0, 1.5, 2.0] {
                for state in 0..6 {
                    let ctx = egui::Context::default();
                    let mut app = if state == 0 {
                        App::with_settings(
                            &ctx,
                            BackgroundWake::new(|| {}),
                            Settings {
                                startup_updates: false,
                            },
                            None,
                        )
                    } else {
                        fixture(&ctx)
                    };
                    if state == 2 {
                        app.select_page(1);
                    }
                    if state == 3 {
                        app.settings_open = true;
                    }
                    if state == 4 {
                        app.error = Some(
                            "A generated error message with a long document path. ".repeat(40),
                        );
                    }
                    if state == 5 {
                        app.discard = Some(Discard::Close);
                        app.dirty = true;
                    }
                    for _ in 0..3 {
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    Vec2::new(width, height),
                                )),
                                viewports: [(
                                    egui::ViewportId::ROOT,
                                    egui::ViewportInfo {
                                        native_pixels_per_point: Some(scale),
                                        ..Default::default()
                                    },
                                )]
                                .into_iter()
                                .collect(),
                                ..Default::default()
                            },
                            |ctx| app.update(ctx),
                        );
                    }
                    let used = ctx.used_rect();
                    assert!(
                        used.max.x <= width + 1.0 && used.max.y <= height + 1.0,
                        "UI exceeds {width}x{height} at {scale} scale in state {state}: {used:?}"
                    );
                    assert!(!app.worker.is_busy());
                    assert!(!app.network.is_busy());
                    assert_eq!(app.dirty, state == 5);
                }
            }
        }
    }

    #[test]
    fn page_actions_follow_selection_and_last_removal_returns_to_sheet_setup() {
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx);
        app.select_page(2);
        assert!(matches!(app.inspector, InspectorTab::Page));
        let source = app.project.pages[2].clone();
        app.move_selected(-1);
        assert_eq!(app.selected, 1);
        assert_eq!(app.sheet, 0);
        assert_eq!(app.project.pages[1], source);
        while !app.project.pages.is_empty() {
            app.remove_selected();
        }
        assert!(matches!(app.inspector, InspectorTab::Sheet));
        assert_eq!((app.selected, app.sheet), (0, 0));
    }
}
