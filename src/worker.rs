use crate::{
    document::{self, Catalog},
    layout::{Page, Project},
    output, settings, updater,
};
use anyhow::{Context, Result, ensure};
use eframe::egui;
use image::RgbImage;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

pub enum Task {
    Import(usize),
    Open,
    Save(Project),
    Clear,
    Preview {
        project: Project,
        sheet: usize,
        selected: usize,
        revision: u64,
    },
    Export(Project),
    Print(Project),
    Settings(settings::Settings),
    CheckUpdate,
    Download(updater::Update),
}
pub enum Outcome {
    Imported(Vec<(Page, RgbImage)>),
    Opened(Project, Vec<RgbImage>),
    Preview {
        revision: u64,
        image: RgbImage,
        page: Page,
        thumbnail: RgbImage,
    },
    Message(String),
    Cancelled,
    Checked(Option<updater::Update>),
    Downloaded(PathBuf, String),
}
pub enum Event {
    Progress(String, f32),
    Finished(std::result::Result<Outcome, String>),
}
pub struct Worker {
    sender: SyncSender<Task>,
    pub receiver: Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    pub busy: bool,
}
impl Worker {
    pub fn new(ctx: egui::Context) -> Self {
        let (sender, tasks) = mpsc::sync_channel(1);
        let (events, receiver) = mpsc::sync_channel(4);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            // MTA keeps WinRT PDF async completions independent of the UI message loop.
            #[cfg(windows)]
            let init = unsafe {
                windows::Win32::System::WinRT::RoInitialize(
                    windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
                )
            };
            let mut catalog = Catalog::new();
            while let Ok(task) = tasks.recv() {
                let progress = |message: String, ratio: f32| {
                    let _ = events.try_send(Event::Progress(message, ratio));
                    ctx.request_repaint();
                };
                #[cfg(windows)]
                let result = match &init {
                    Ok(_) => run(task, &mut catalog, &stop, progress),
                    Err(e) => Err(anyhow::anyhow!(
                        "Windows Runtime initialization failed: {e}"
                    )),
                };
                #[cfg(not(windows))]
                let result = run(task, &mut catalog, &stop, progress);
                if events
                    .send(Event::Finished(result.map_err(|e| format!("{e:#}"))))
                    .is_err()
                {
                    break;
                }
                ctx.request_repaint();
            }
            drop(catalog);
            #[cfg(windows)]
            if init.is_ok() {
                unsafe {
                    windows::Win32::System::WinRT::RoUninitialize();
                }
            }
        });
        Self {
            sender,
            receiver,
            cancel,
            busy: false,
        }
    }
    pub fn submit(&mut self, task: Task) -> bool {
        if self.busy {
            return false;
        }
        self.cancel.store(false, Ordering::Relaxed);
        if self.sender.try_send(task).is_ok() {
            self.busy = true;
            true
        } else {
            false
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn thumb(page: &Page, catalog: &Catalog) -> Result<RgbImage> {
    let (w, h) = page.visible_size();
    let scale = (120.0 / w).min(150.0 / h);
    document::render(
        page,
        catalog,
        (
            (w * scale).round().max(1.0) as u32,
            (h * scale).round().max(1.0) as u32,
        ),
        false,
    )
}
fn thumbnails(
    pages: &[Page],
    catalog: &Catalog,
    cancel: &AtomicBool,
    progress: &impl Fn(String, f32),
) -> Result<Vec<RgbImage>> {
    let mut images = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "Import cancelled");
        images.push(thumb(page, catalog)?);
        progress(
            format!("Preparing page {} of {}", i + 1, pages.len()),
            (i + 1) as f32 / pages.len() as f32,
        );
    }
    Ok(images)
}
fn run(
    task: Task,
    catalog: &mut Catalog,
    cancel: &AtomicBool,
    progress: impl Fn(String, f32),
) -> Result<Outcome> {
    match task {
        Task::Clear => {
            catalog.clear();
            Ok(Outcome::Cancelled)
        }
        Task::Import(existing) => {
            let Some(paths) = rfd::FileDialog::new()
                .add_filter(
                    "PDF and images",
                    &["pdf", "png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
                )
                .pick_files()
            else {
                return Ok(Outcome::Cancelled);
            };
            let mut pending = catalog.clone();
            let pages = document::import(&paths, &mut pending)?;
            ensure!(
                existing + pages.len() <= 500,
                "Project would exceed 500 pages"
            );
            let images = thumbnails(&pages, &pending, cancel, &progress)?;
            *catalog = pending;
            Ok(Outcome::Imported(pages.into_iter().zip(images).collect()))
        }
        Task::Open => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PrintHelper project", &["phlayout"])
                .pick_file()
            else {
                return Ok(Outcome::Cancelled);
            };
            ensure!(
                std::fs::metadata(&path)?.len() < 2_000_000,
                "Project file exceeds 2 MB"
            );
            let mut project: Project = serde_json::from_slice(&std::fs::read(&path)?)?;
            project.validate()?;
            let mut pending = Catalog::new();
            for page in &mut project.pages {
                if page.path.is_relative() {
                    page.path = path
                        .parent()
                        .context("Missing project directory")?
                        .join(&page.path);
                }
                page.path = page
                    .path
                    .canonicalize()
                    .with_context(|| format!("Locate {}", page.path.display()))?;
                if !pending.contains_key(&page.path) {
                    pending.insert(page.path.clone(), Arc::new(document::load(&page.path)?));
                }
                ensure!(
                    pending.values().map(|s| s.cost).sum::<u64>() <= 256 * 1024 * 1024,
                    "Project exceeds 256 MiB input budget"
                );
                document::verify_page(page, &pending[&page.path])?;
            }
            let images = thumbnails(&project.pages, &pending, cancel, &progress)?;
            *catalog = pending;
            Ok(Outcome::Opened(project, images))
        }
        Task::Save(project) => {
            project.validate()?;
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PrintHelper project", &["phlayout"])
                .set_file_name("Layout.phlayout")
                .save_file()
            else {
                return Ok(Outcome::Cancelled);
            };
            output::save_new(&path, &serde_json::to_vec_pretty(&project)?)?;
            Ok(Outcome::Message(format!("Saved {}", path.display())))
        }
        Task::Preview {
            project,
            sheet,
            selected,
            revision,
        } => {
            let page = project
                .pages
                .get(selected)
                .context("Select a page")?
                .clone();
            let thumbnail = thumb(&page, catalog)?;
            let (w, h) = project.layout.paper_pt();
            let dpi = (1000.0 / w.max(h) * 72.0).clamp(10.0, 100.0);
            let image = output::render_sheet(&project, catalog, sheet, dpi)?;
            Ok(Outcome::Preview {
                revision,
                image,
                page,
                thumbnail,
            })
        }
        Task::Export(project) => {
            let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF document", &["pdf"])
                .set_file_name("PrintHelper-output.pdf")
                .save_file()
            else {
                return Ok(Outcome::Cancelled);
            };
            output::export(&project, catalog, &path, cancel, |i, n| {
                progress(format!("Exporting sheet {i} of {n}"), i as f32 / n as f32)
            })?;
            Ok(Outcome::Message(format!("Exported {}", path.display())))
        }
        Task::Print(project) => {
            #[cfg(windows)]
            return Ok(Outcome::Message(crate::printing::print(
                &project,
                catalog,
                cancel,
                |i, n| progress(format!("Printing sheet {i} of {n}"), i as f32 / n as f32),
            )?));
            #[cfg(not(windows))]
            {
                let _ = project;
                anyhow::bail!("Printing requires Windows");
            }
        }
        Task::Settings(settings) => {
            settings::save(&settings)?;
            Ok(Outcome::Message("Settings saved".into()))
        }
        Task::CheckUpdate => Ok(Outcome::Checked(updater::check()?)),
        Task::Download(update) => {
            let path = updater::download(&update, cancel, |i, n| {
                progress("Downloading verified update".into(), i as f32 / n as f32)
            })?;
            Ok(Outcome::Downloaded(
                path,
                update.asset.digest.context("Missing update digest")?,
            ))
        }
    }
}
