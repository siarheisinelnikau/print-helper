use image::{Rgb, RgbImage};
use print_helper::{
    document::{self, Catalog},
    layout::{Layout, PT_PER_MM, Paper, Project},
    output::{self, PdfWriter},
};
use std::{path::PathBuf, sync::atomic::AtomicBool};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".test-output")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn image(&self, name: &str, color: [u8; 3]) -> PathBuf {
        let p = self.0.join(name);
        RgbImage::from_pixel(60, 80, Rgb(color)).save(&p).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for entry in std::fs::read_dir(&self.0).unwrap() {
            let _ = std::fs::remove_file(entry.unwrap().path());
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

fn project(pages: Vec<print_helper::layout::Page>, per_sheet: usize) -> Project {
    Project {
        format_version: 1,
        pages,
        layout: Layout {
            per_sheet,
            ..Default::default()
        },
    }
}

#[test]
fn cells_fit_paper_and_count_partial_sheets() {
    for paper in [Paper::A4, Paper::A5, Paper::Letter] {
        for landscape in [false, true] {
            for n in [1, 2, 4] {
                let layout = Layout {
                    paper,
                    landscape,
                    per_sheet: n,
                    ..Default::default()
                };
                layout.validate().unwrap();
                assert_eq!(layout.cells().len(), n);
                assert_eq!(layout.sheet_count(5), 5_usize.div_ceil(n));
                let (w, h) = layout.paper_pt();
                for c in layout.cells() {
                    assert!(c.x >= 10.0 * PT_PER_MM - 0.01 && c.y >= 10.0 * PT_PER_MM - 0.01);
                    assert!(c.x + c.w <= w && c.y + c.h <= h);
                }
            }
        }
    }
    assert!(
        Layout {
            per_sheet: 0,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        Layout {
            margin_mm: f64::NAN,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn actual_pixels_preserve_order_margins_and_blank_slots() {
    let f = Fixture::new();
    let mut catalog = Catalog::new();
    let mut pages = document::import(
        &[
            f.image("red.png", [255, 0, 0]),
            f.image("blue.png", [0, 0, 255]),
        ],
        &mut catalog,
    )
    .unwrap();
    pages.swap(0, 1);
    let p = project(pages, 4);
    let img = output::render_sheet(&p, &catalog, 0, 72.0).unwrap();
    assert_eq!(img.dimensions(), (595, 842));
    assert_eq!(img.get_pixel(0, 0).0, [255; 3]);
    let cells = p.layout.cells();
    for (i, color) in [[0, 0, 255], [255, 0, 0], [255; 3], [255; 3]]
        .iter()
        .enumerate()
    {
        let c = cells[i];
        assert_eq!(
            &img.get_pixel((c.x + c.w / 2.0) as u32, (c.y + c.h / 2.0) as u32)
                .0,
            color
        );
    }
}

#[test]
fn asymmetric_crop_then_clockwise_rotation_and_grayscale() {
    let f = Fixture::new();
    let path = f.0.join("pattern.png");
    RgbImage::from_fn(100, 80, |x, y| {
        Rgb(if x < 40 {
            [255, 0, 0]
        } else if y < 40 {
            [0, 255, 0]
        } else {
            [0, 0, 255]
        })
    })
    .save(&path)
    .unwrap();
    let mut catalog = Catalog::new();
    let mut page = document::import(&[path], &mut catalog).unwrap().remove(0);
    page.crop = [0.4, 0.0, 0.0, 0.0];
    page.quarter_turns = 1;
    let img = document::render(&page, &catalog, (80, 60), false).unwrap();
    assert_eq!(img.get_pixel(10, 30).0, [0, 0, 255]);
    assert_eq!(img.get_pixel(70, 30).0, [0, 255, 0]);
    let gray = document::render(&page, &catalog, (80, 60), true).unwrap();
    for p in gray.pixels() {
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
    }
    page.crop[0] = 0.8;
    assert!(page.validate().is_err());
}

#[test]
fn transparent_pixels_become_white_paper() {
    let f = Fixture::new();
    let path = f.0.join("alpha.png");
    image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 0]))
        .save(&path)
        .unwrap();
    let mut catalog = Catalog::new();
    let page = document::import(&[path], &mut catalog).unwrap().remove(0);
    assert_eq!(
        document::render(&page, &catalog, (10, 10), false)
            .unwrap()
            .get_pixel(0, 0)
            .0,
        [255; 3]
    );
}

#[test]
fn saved_transforms_round_trip_and_changed_sources_are_rejected() {
    let f = Fixture::new();
    let path = f.image("input.png", [10, 20, 30]);
    let mut catalog = Catalog::new();
    let mut p = project(document::import(&[path.clone()], &mut catalog).unwrap(), 2);
    p.pages[0].crop = [0.1, 0.2, 0.05, 0.1];
    p.pages[0].quarter_turns = 3;
    let copy: Project = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
    assert_eq!(p, copy);
    p.validate().unwrap();
    RgbImage::from_pixel(60, 80, Rgb([99; 3]))
        .save(&path)
        .unwrap();
    assert!(document::verify_page(&p.pages[0], &document::load(&path).unwrap()).is_err());
}

#[test]
fn cancellation_and_existing_targets_preserve_files() {
    let f = Fixture::new();
    let path = f.image("original.png", [10, 20, 30]);
    let before = std::fs::read(&path).unwrap();
    let mut catalog = Catalog::new();
    let p = project(document::import(&[path.clone()], &mut catalog).unwrap(), 1);
    let cancelled = f.0.join("cancelled.pdf");
    assert!(output::export(&p, &catalog, &cancelled, &AtomicBool::new(true), |_, _| {}).is_err());
    assert!(!cancelled.exists());
    assert!(output::export(&p, &catalog, &path, &AtomicBool::new(false), |_, _| {}).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(output::save_new(&path, b"replace").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[cfg(windows)]
fn native(test: impl FnOnce()) {
    unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )
        .unwrap();
    }
    test();
    unsafe {
        windows::Win32::System::WinRT::RoUninitialize();
    }
}

#[test]
#[cfg(windows)]
fn native_pdf_import_and_export_round_trip() {
    native(|| {
        let f = Fixture::new();
        let input = f.0.join("generated.pdf");
        let mut data = std::io::Cursor::new(Vec::new());
        let mut writer = PdfWriter::new(&mut data).unwrap();
        writer
            .page(
                &RgbImage::from_pixel(30, 40, Rgb([255, 0, 0])),
                (300.0, 400.0),
            )
            .unwrap();
        writer
            .page(
                &RgbImage::from_pixel(40, 30, Rgb([0, 0, 255])),
                (400.0, 300.0),
            )
            .unwrap();
        writer.finish().unwrap();
        std::fs::write(&input, data.into_inner()).unwrap();
        let before = std::fs::read(&input).unwrap();
        let mut catalog = Catalog::new();
        let mut pages = document::import(&[input.clone()], &mut catalog).unwrap();
        assert_eq!(pages.len(), 2);
        assert!((pages[0].width_pt - 300.0).abs() < 0.01);
        assert!((pages[1].height_pt - 300.0).abs() < 0.01);
        pages.swap(0, 1);
        pages[0].quarter_turns = 1;
        let p = project(pages, 2);
        let output = f.0.join("output.pdf");
        output::export(&p, &catalog, &output, &AtomicBool::new(false), |_, _| {}).unwrap();
        let doc = document::pdf::load(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(doc.PageCount().unwrap(), 1);
        let size = doc.GetPage(0).unwrap().Size().unwrap();
        assert!((size.Width as f64 * 0.75 - p.layout.paper_pt().0).abs() < 0.01);
        assert!((size.Height as f64 * 0.75 - p.layout.paper_pt().1).abs() < 0.01);
        let image = document::pdf::render(&doc, 0, 595, 842).unwrap();
        let top = image.get_pixel(297, 210);
        let bottom = image.get_pixel(297, 630);
        assert!(
            top[2] > 240 && top[0] < 15,
            "top must contain the reordered blue page"
        );
        assert!(bottom[0] > 240 && bottom[2] < 15, "bottom must contain red");
        assert_eq!(std::fs::read(input).unwrap(), before);
    });
}
