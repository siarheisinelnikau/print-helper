use crate::software::BackgroundWake;
use crate::{
    dialogs,
    document::{self, Catalog},
    layout::{Page, Project},
    output, settings, updater,
};
use anyhow::{Context, Result, ensure};
use image::RgbImage;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

pub enum Task {
    Import(usize),
    ImportPaths {
        existing: usize,
        paths: Vec<PathBuf>,
    },
    Open,
    Save(Project),
    Clear,
    Preview {
        project: Project,
        sheet: usize,
        revision: u64,
    },
    Thumbnail(Page),
    Export(Project),
    Print(Project),
    Settings(settings::Settings),
    CheckUpdate,
    Download(updater::Update),
    Install(PathBuf, String),
}
pub enum Outcome {
    Imported(Vec<(Page, RgbImage)>),
    Opened(Project, Vec<RgbImage>),
    Preview { revision: u64, image: RgbImage },
    Thumbnail { page: Page, image: RgbImage },
    Message(String),
    Cancelled,
    Checked(Option<updater::Update>),
    Downloaded(PathBuf, String),
    Restart,
}
pub enum Event {
    Progress(String, f32),
    Finished(std::result::Result<Outcome, String>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkKind {
    Preview,
    Thumbnail,
    Foreground,
}

pub struct Worker {
    sender: SyncSender<Task>,
    pub receiver: Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    in_flight: VecDeque<WorkKind>,
}
impl Worker {
    pub fn new(wake: BackgroundWake) -> Self {
        // Reserve room for one explicit action behind an in-progress preview.
        let (sender, tasks) = mpsc::sync_channel(2);
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
                    wake.notify();
                };
                #[cfg(windows)]
                let result = match &init {
                    Ok(_) if stop.load(Ordering::Relaxed) => Ok(Outcome::Cancelled),
                    Ok(_) => run(task, &mut catalog, &stop, progress),
                    Err(e) => Err(anyhow::anyhow!(
                        "Windows Runtime initialization failed: {e}"
                    )),
                };
                #[cfg(not(windows))]
                let result = if stop.load(Ordering::Relaxed) {
                    Ok(Outcome::Cancelled)
                } else {
                    run(task, &mut catalog, &stop, progress)
                };
                if events
                    .send(Event::Finished(result.map_err(|e| format!("{e:#}"))))
                    .is_err()
                {
                    break;
                }
                wake.notify();
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
            in_flight: VecDeque::with_capacity(2),
        }
    }
    pub fn submit(&mut self, task: Task) -> bool {
        let kind = match &task {
            Task::Preview { .. } => WorkKind::Preview,
            Task::Thumbnail(_) => WorkKind::Thumbnail,
            _ => WorkKind::Foreground,
        };
        if self.is_busy() && (kind != WorkKind::Foreground || self.foreground_busy()) {
            return false;
        }
        self.cancel.store(false, Ordering::Relaxed);
        if self.sender.try_send(task).is_ok() {
            self.in_flight.push_back(kind);
            true
        } else {
            false
        }
    }
    pub fn is_busy(&self) -> bool {
        !self.in_flight.is_empty()
    }
    pub fn foreground_busy(&self) -> bool {
        self.in_flight.contains(&WorkKind::Foreground)
    }
    /// Acknowledge exactly one Finished event; queued actions remain busy.
    pub fn finish(&mut self) -> Option<WorkKind> {
        self.in_flight.pop_front()
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
fn import_paths(
    existing: usize,
    paths: &[PathBuf],
    catalog: &mut Catalog,
    cancel: &AtomicBool,
    progress: &impl Fn(String, f32),
) -> Result<Outcome> {
    ensure!(!paths.is_empty(), "No local files were provided");
    ensure!(
        existing <= 500 && paths.len() <= 500 - existing,
        "Project would exceed 500 pages"
    );
    ensure!(!cancel.load(Ordering::Relaxed), "Import cancelled");
    let mut pending = catalog.clone();
    let pages = document::import(paths, &mut pending)?;
    ensure!(
        pages.len() <= 500 - existing,
        "Project would exceed 500 pages"
    );
    let images = thumbnails(&pages, &pending, cancel, progress)?;
    ensure!(!cancel.load(Ordering::Relaxed), "Import cancelled");
    *catalog = pending;
    Ok(Outcome::Imported(pages.into_iter().zip(images).collect()))
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
            let Some(paths) = dialogs::pick_inputs()? else {
                return Ok(Outcome::Cancelled);
            };
            import_paths(existing, &paths, catalog, cancel, &progress)
        }
        Task::ImportPaths { existing, paths } => {
            import_paths(existing, &paths, catalog, cancel, &progress)
        }
        Task::Open => {
            let Some(path) = dialogs::open_layout()? else {
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
            let Some(path) = dialogs::save_layout()? else {
                return Ok(Outcome::Cancelled);
            };
            output::save_new(&path, &serde_json::to_vec_pretty(&project)?)?;
            Ok(Outcome::Message(format!("Saved {}", path.display())))
        }
        Task::Preview {
            project,
            sheet,
            revision,
        } => {
            let (w, h) = project.layout.paper_pt();
            let dpi = (1000.0 / w.max(h) * 72.0).clamp(10.0, 100.0);
            let image = output::render_sheet(&project, catalog, sheet, dpi)?;
            Ok(Outcome::Preview { revision, image })
        }
        Task::Thumbnail(page) => {
            let image = thumb(&page, catalog)?;
            Ok(Outcome::Thumbnail { page, image })
        }
        Task::Export(project) => {
            let Some(path) = dialogs::export_pdf()? else {
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
        Task::Install(path, digest) => {
            updater::launch_install(&path, &digest)?;
            Ok(Outcome::Restart)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_path_import_preserves_order_and_rolls_back_failed_or_cancelled_batches() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".test-output")
            .join(format!(
                "drop-worker-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&root).unwrap();
        let paths: Vec<_> = ["red.png", "blue.png", "green.png"]
            .into_iter()
            .map(|name| root.join(name))
            .collect();
        for (path, color) in paths
            .iter()
            .zip([[200, 20, 10], [10, 20, 200], [20, 180, 10]])
        {
            RgbImage::from_pixel(8, 12, image::Rgb(color))
                .save(path)
                .unwrap();
        }
        let originals: Vec<_> = paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let cancel = AtomicBool::new(false);
        let mut catalog = Catalog::new();
        let result = run(
            Task::ImportPaths {
                existing: 0,
                paths: vec![paths[1].clone(), paths[0].clone()],
            },
            &mut catalog,
            &cancel,
            |_, _| {},
        )
        .unwrap();
        let Outcome::Imported(items) = result else {
            panic!("Expected imported pages")
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0.path, paths[1].canonicalize().unwrap());
        assert_eq!(*items[0].1.get_pixel(0, 0), image::Rgb([10, 20, 200]));
        assert_eq!(items[1].0.path, paths[0].canonicalize().unwrap());
        let bad = root.join("not-an-image.txt");
        std::fs::write(&bad, "Generated unsupported input").unwrap();
        assert!(
            run(
                Task::ImportPaths {
                    existing: 2,
                    paths: vec![paths[2].clone(), bad.clone()]
                },
                &mut catalog,
                &cancel,
                |_, _| {}
            )
            .is_err()
        );
        assert_eq!(catalog.len(), 2);
        assert!(!catalog.contains_key(&paths[2].canonicalize().unwrap()));
        assert!(
            run(
                Task::ImportPaths {
                    existing: 2,
                    paths: vec![paths[2].clone()]
                },
                &mut catalog,
                &cancel,
                |_, _| {
                    cancel.store(true, Ordering::Relaxed);
                }
            )
            .is_err()
        );
        assert_eq!(catalog.len(), 2);
        cancel.store(false, Ordering::Relaxed);
        assert!(
            run(
                Task::ImportPaths {
                    existing: 500,
                    paths: vec![paths[2].clone()]
                },
                &mut catalog,
                &cancel,
                |_, _| {}
            )
            .is_err()
        );
        assert!(
            run(
                Task::ImportPaths {
                    existing: 2,
                    paths: vec![]
                },
                &mut catalog,
                &cancel,
                |_, _| {}
            )
            .is_err()
        );
        for (path, original) in paths.iter().zip(originals) {
            assert_eq!(std::fs::read(path).unwrap(), original);
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_file(bad).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn sheet_render_does_not_depend_on_a_selected_thumbnail_from_another_sheet() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".test-output")
            .join(format!(
                "worker-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("generated.png");
        let color = image::Rgb([170, 30, 60]);
        RgbImage::from_pixel(60, 80, color).save(&path).unwrap();
        let mut catalog = Catalog::new();
        let pages = document::import(std::slice::from_ref(&path), &mut catalog).unwrap();
        let page = pages[0].clone();
        let mut missing = page.clone();
        missing.path = root.join("absent.png");
        let project = Project {
            format_version: 1,
            pages: vec![page.clone(), missing.clone()],
            layout: crate::layout::Layout {
                per_sheet: 1,
                ..Default::default()
            },
        };
        let cancel = AtomicBool::new(false);
        let no_progress =
            |_: String, _: f32| panic!("Background render must not show task progress");
        let result = run(
            Task::Preview {
                project,
                sheet: 0,
                revision: 9,
            },
            &mut catalog,
            &cancel,
            no_progress,
        )
        .unwrap();
        let Outcome::Preview { revision, image } = result else {
            panic!("Expected sheet")
        };
        assert_eq!(revision, 9);
        assert_eq!(
            *image.get_pixel(image.width() / 2, image.height() / 2),
            color
        );
        assert!(run(Task::Thumbnail(missing), &mut catalog, &cancel, no_progress).is_err());
        let result = run(Task::Thumbnail(page), &mut catalog, &cancel, no_progress).unwrap();
        let Outcome::Thumbnail { image, .. } = result else {
            panic!("Expected thumbnail")
        };
        assert!(image.width() <= 120 && image.height() <= 150);
        assert_eq!(
            *image.get_pixel(image.width() / 2, image.height() / 2),
            color
        );
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn explicit_action_can_queue_behind_preview_without_disabling_controls_first() {
        // No worker thread executes these tasks: inspect the real dispatch boundary.
        let (sender, tasks) = mpsc::sync_channel(2);
        let (_events, receiver) = mpsc::sync_channel(4);
        let mut worker = Worker {
            sender,
            receiver,
            cancel: Arc::new(AtomicBool::new(false)),
            in_flight: VecDeque::with_capacity(2),
        };
        let preview = || Task::Preview {
            project: Project::default(),
            sheet: 0,
            revision: 1,
        };
        assert!(worker.submit(preview()));
        assert!(worker.is_busy());
        assert!(!worker.foreground_busy());
        assert!(!worker.submit(preview()));
        assert!(worker.submit(Task::Clear));
        assert!(worker.foreground_busy());
        assert!(!worker.submit(Task::Clear));
        assert!(matches!(tasks.try_recv().unwrap(), Task::Preview { .. }));
        assert!(matches!(tasks.try_recv().unwrap(), Task::Clear));
        assert!(tasks.try_recv().is_err());
        assert_eq!(worker.finish(), Some(WorkKind::Preview));
        assert!(worker.is_busy());
        assert!(worker.foreground_busy());
        assert_eq!(worker.finish(), Some(WorkKind::Foreground));
        assert!(!worker.is_busy());
        assert!(!worker.foreground_busy());
    }
}
