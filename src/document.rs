use crate::i18n::{Message, tr};
use crate::layout::Page;
use anyhow::{Context, Result, ensure};
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: u64 = 256 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 32_000_000;

pub struct Source {
    pub digest: String,
    pub sizes: Vec<(f64, f64)>,
    pub cost: u64,
    data: SourceData,
}

enum SourceData {
    Image(RgbImage),
    #[cfg(windows)]
    Pdf(windows::Data::Pdf::PdfDocument),
}

pub type Catalog = HashMap<PathBuf, Arc<Source>>;

pub fn load(path: &Path) -> Result<Source> {
    let file = std::fs::File::open(path).with_context(|| {
        Message::new("Read {path}", &[("path", path.display().to_string())]).translated()
    })?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= MAX_FILE,
        tr("Input must be a regular file up to 64 MiB")
    );
    // Bound the read itself: a document may grow after its metadata was checked.
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= MAX_FILE, tr("Input exceeds 64 MiB"));
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if bytes.starts_with(b"%PDF-") {
        #[cfg(windows)]
        {
            let doc = pdf::load(&bytes).context(tr(
                "Windows could not open this PDF (encrypted or unsupported PDFs are not supported)",
            ))?;
            let n = doc.PageCount()?;
            ensure!(n > 0 && n <= 500, tr("PDF must have 1–500 pages"));
            let mut sizes = Vec::new();
            for i in 0..n {
                let p = doc.GetPage(i)?;
                let s = p.Size()?;
                sizes.push((s.Width as f64 * 0.75, s.Height as f64 * 0.75));
                p.Close()?;
            }
            Ok(Source {
                digest,
                sizes,
                cost: bytes.len() as u64,
                data: SourceData::Pdf(doc),
            })
        }
        #[cfg(not(windows))]
        anyhow::bail!(tr("PDF import requires Windows 10 or later"));
    } else {
        let reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
        let mut decoder = reader.into_decoder()?;
        let (w, h) = decoder.dimensions();
        ensure!(
            w as u64 * h as u64 <= MAX_PIXELS,
            tr("Image exceeds 32 megapixels")
        );
        let orientation = decoder.orientation()?;
        let mut decoded = DynamicImage::from_decoder(decoder)?;
        decoded.apply_orientation(orientation);
        // Composite alpha onto paper; simply dropping alpha would turn transparent areas black.
        let rgba = decoded.into_rgba8();
        let image = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let p = rgba.get_pixel(x, y).0;
            image::Rgb([0, 1, 2].map(|i| {
                ((p[i] as u32 * p[3] as u32 + 255 * (255 - p[3] as u32) + 127) / 255) as u8
            }))
        });
        let cost = image.as_raw().len() as u64;
        Ok(Source {
            digest,
            sizes: vec![(image.width() as f64 * 0.75, image.height() as f64 * 0.75)],
            cost,
            data: SourceData::Image(image),
        })
    }
}

pub fn import(paths: &[PathBuf], catalog: &mut Catalog) -> Result<Vec<Page>> {
    let mut pages = Vec::new();
    let mut pending = catalog.clone();
    for path in paths {
        let path = path.canonicalize().context(tr("Locate input document"))?;
        if !pending.contains_key(&path) {
            pending.insert(path.clone(), Arc::new(load(&path)?));
        }
        ensure!(
            pending.values().map(|s| s.cost).sum::<u64>() <= MAX_TOTAL,
            tr("Project input budget exceeds 256 MiB; use a smaller batch")
        );
        let s = &pending[&path];
        for (i, &(width_pt, height_pt)) in s.sizes.iter().enumerate() {
            pages.push(Page {
                path: path.clone(),
                sha256: s.digest.clone(),
                source_page: i as u32,
                width_pt,
                height_pt,
                crop: [0.0; 4],
                quarter_turns: 0,
            });
        }
        ensure!(pages.len() <= 500, tr("Import exceeds 500 pages"));
    }
    *catalog = pending;
    Ok(pages)
}

pub fn verify_page(page: &Page, source: &Source) -> Result<()> {
    page.validate()?;
    ensure!(
        source.digest == page.sha256,
        Message::new(
            "Source has changed: {path}",
            &[("path", page.path.display().to_string())]
        )
        .translated()
    );
    let size = source
        .sizes
        .get(page.source_page as usize)
        .context(tr("Source page no longer exists"))?;
    ensure!(
        (size.0 - page.width_pt).abs() < 0.01 && (size.1 - page.height_pt).abs() < 0.01,
        tr("Source dimensions differ from saved project")
    );
    Ok(())
}

pub fn render(page: &Page, catalog: &Catalog, target: (u32, u32), gray: bool) -> Result<RgbImage> {
    let source = catalog
        .get(&page.path)
        .context(tr("Source document is not loaded"))?;
    verify_page(page, source)?;
    ensure!(
        target.0 > 0 && target.1 > 0 && target.0 as u64 * target.1 as u64 <= MAX_PIXELS,
        tr("Page output must contain 1–32 million pixels")
    );
    let cropped = match &source.data {
        // Crop original pixels before any filtering. Resizing the whole source first
        // blended discarded margins into the preview and produced DPI-dependent crops.
        SourceData::Image(img) => crop_pixels(img, page.crop)?,
        #[cfg(windows)]
        SourceData::Pdf(doc) => {
            let (tw, th) = if page.quarter_turns % 2 == 1 {
                (target.1, target.0)
            } else {
                target
            };
            let w = (tw as f64 / (1.0 - page.crop[0] - page.crop[2])).ceil() as u32;
            let h = (th as f64 / (1.0 - page.crop[1] - page.crop[3])).ceil() as u32;
            crop_pixels(&pdf::render(doc, page.source_page, w, h)?, page.crop)?
        }
    };
    let mut img = match page.quarter_turns {
        1 => image::imageops::rotate90(&cropped),
        2 => image::imageops::rotate180(&cropped),
        3 => image::imageops::rotate270(&cropped),
        _ => cropped,
    };
    if gray {
        for p in img.pixels_mut() {
            let g = ((p[0] as u32 * 2126 + p[1] as u32 * 7152 + p[2] as u32 * 722 + 5000) / 10000)
                as u8;
            *p = image::Rgb([g; 3]);
        }
    }
    Ok(image::imageops::resize(
        &img,
        target.0,
        target.1,
        image::imageops::FilterType::Triangle,
    ))
}

fn crop_pixels(image: &RgbImage, crop: [f64; 4]) -> Result<RgbImage> {
    let (w, h) = image.dimensions();
    // Percentages such as 29% can multiply back to 28.999999999999996.
    // Snap only floating-point noise so floor/ceil do not retain an extra border.
    let snap = |value: f64| {
        let nearest = value.round();
        if (value - nearest).abs() <= value.abs().max(1.0) * f64::EPSILON * 8.0 {
            nearest
        } else {
            value
        }
    };
    let x = snap(crop[0] * w as f64).floor() as u32;
    let y = snap(crop[1] * h as f64).floor() as u32;
    let right = snap((1.0 - crop[2]) * w as f64).ceil().min(w as f64) as u32;
    let bottom = snap((1.0 - crop[3]) * h as f64).ceil().min(h as f64) as u32;
    ensure!(right > x && bottom > y, tr("Crop leaves no pixels"));
    Ok(image::imageops::crop_imm(image, x, y, right - x, bottom - y).to_image())
}

#[cfg(windows)]
pub mod pdf {
    use super::*;
    use windows::{
        Data::Pdf::{PdfDocument, PdfPageRenderOptions},
        Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
    };

    pub fn load(bytes: &[u8]) -> Result<PdfDocument> {
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(bytes)?;
        writer.StoreAsync()?.join()?;
        writer.DetachStream()?;
        stream.Seek(0)?;
        Ok(PdfDocument::LoadFromStreamAsync(&stream)?.join()?)
    }

    pub fn render(doc: &PdfDocument, index: u32, w: u32, h: u32) -> Result<RgbImage> {
        ensure!(
            w > 0 && h > 0 && w as u64 * h as u64 <= MAX_PIXELS,
            tr("Page render exceeds 32 megapixels; reduce DPI or crop")
        );
        let page = doc.GetPage(index)?;
        let result = (|| {
            let stream = InMemoryRandomAccessStream::new()?;
            let options = PdfPageRenderOptions::new()?;
            options.SetDestinationWidth(w)?;
            options.SetDestinationHeight(h)?;
            page.RenderWithOptionsToStreamAsync(&stream, &options)?
                .join()?;
            let len = stream.Size()?;
            ensure!(len <= MAX_FILE, tr("Encoded PDF page is too large"));
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
            let n = reader.LoadAsync(len as u32)?.join()?;
            ensure!(n as u64 == len, tr("Incomplete PDF render stream"));
            let mut bytes = vec![0; n as usize];
            reader.ReadBytes(&mut bytes)?;
            Ok(image::load_from_memory(&bytes)?.into_rgb8())
        })();
        page.Close()?;
        result
    }
}
