use crate::{
    document::{self, Catalog},
    layout::{Project, fit},
};
use anyhow::{Context, Result, ensure};
use flate2::{Compression, write::ZlibEncoder};
use image::{Rgb, RgbImage};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Seek, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub fn render_sheet(
    project: &Project,
    catalog: &Catalog,
    sheet: usize,
    dpi: f64,
) -> Result<RgbImage> {
    project.validate()?;
    ensure!(
        sheet < project.layout.sheet_count(project.pages.len()),
        "Sheet is outside the project"
    );
    ensure!(
        dpi.is_finite() && (10.0..=300.0).contains(&dpi),
        "Invalid render resolution"
    );
    let (pw, ph) = project.layout.paper_pt();
    let scale = dpi / 72.0;
    let mut image = RgbImage::from_pixel(
        (pw * scale).round() as u32,
        (ph * scale).round() as u32,
        Rgb([255; 3]),
    );
    for (i, cell) in project.layout.cells().into_iter().enumerate() {
        let Some(page) = project.pages.get(sheet * project.layout.per_sheet + i) else {
            break;
        };
        let rect = fit(page.visible_size(), cell);
        let bitmap = document::render(
            page,
            catalog,
            (
                (rect.w * scale).round().max(1.0) as u32,
                (rect.h * scale).round().max(1.0) as u32,
            ),
            project.layout.grayscale,
        )?;
        image::imageops::overlay(
            &mut image,
            &bitmap,
            (rect.x * scale).round() as i64,
            (rect.y * scale).round() as i64,
        );
    }
    Ok(image)
}

/// A new sibling temporary file; never truncates a source or an existing output.
pub struct NewFile {
    pub path: PathBuf,
    pub file: std::fs::File,
}
impl NewFile {
    pub fn beside(target: &Path) -> Result<Self> {
        let parent = target
            .parent()
            .context("Destination has no parent directory")?;
        for n in 0..1000 {
            let path = parent.join(format!(".printhelper-{}-{n}.tmp", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .read(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => return Ok(Self { path, file }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        anyhow::bail!("Cannot allocate a temporary output")
    }
    pub fn publish(&mut self, target: &Path) -> Result<()> {
        self.file.sync_all()?;
        // Explicitly omit REPLACE_EXISTING. std::fs::rename replaces existing files.
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::{
                Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW},
                core::PCWSTR,
            };
            let from: Vec<u16> = self.path.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe { MoveFileExW(PCWSTR(from.as_ptr()), PCWSTR(to.as_ptr()), MOVEFILE_WRITE_THROUGH) }
                .context("Cannot save output. Choose a new filename in a writable folder; existing files are never overwritten")?;
        }
        #[cfg(not(windows))]
        std::fs::hard_link(&self.path, target)
            .context("Cannot save output without overwriting an existing file")?;
        Ok(())
    }
}
impl Drop for NewFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn save_new(target: &Path, bytes: &[u8]) -> Result<()> {
    let mut temp = NewFile::beside(target)?;
    temp.file.write_all(bytes)?;
    temp.publish(target)
}

pub fn export(
    project: &Project,
    catalog: &Catalog,
    target: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<()> {
    project.validate()?;
    ensure!(!project.pages.is_empty(), "Add at least one page");
    ensure!(
        !target.exists(),
        "Output already exists. Choose a new filename"
    );
    let mut temp = NewFile::beside(target)?;
    {
        let mut pdf = PdfWriter::new(BufWriter::new(&mut temp.file))?;
        let count = project.layout.sheet_count(project.pages.len());
        for sheet in 0..count {
            ensure!(
                !cancel.load(Ordering::Relaxed),
                "Export cancelled; no output was published"
            );
            let image = render_sheet(project, catalog, sheet, project.layout.dpi as f64)?;
            pdf.page(&image, project.layout.paper_pt())?;
            progress(sheet + 1, count);
        }
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "Export cancelled; no output was published"
        );
        pdf.finish()?;
    }
    temp.publish(target)
}

/// Minimal PDF 1.4 writer. Sheets are RGB raster images with lossless Flate compression.
pub struct PdfWriter<W: Write + Seek> {
    out: W,
    offsets: Vec<u64>,
    pages: Vec<usize>,
}
impl<W: Write + Seek> PdfWriter<W> {
    pub fn new(mut out: W) -> Result<Self> {
        out.write_all(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")?;
        Ok(Self {
            out,
            offsets: vec![0, 0, 0],
            pages: Vec::new(),
        })
    }
    fn begin(&mut self, id: usize) -> Result<()> {
        if self.offsets.len() <= id {
            self.offsets.resize(id + 1, 0);
        }
        self.offsets[id] = self.out.stream_position()?;
        writeln!(self.out, "{id} 0 obj")?;
        Ok(())
    }
    pub fn page(&mut self, image: &RgbImage, size: (f64, f64)) -> Result<()> {
        let page = self.offsets.len();
        let content = page + 1;
        let bitmap = page + 2;
        self.begin(page)?;
        writeln!(
            self.out,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.6} {:.6}] /Resources << /XObject << /Im {bitmap} 0 R >> >> /Contents {content} 0 R >>\nendobj",
            size.0, size.1
        )?;
        let commands = format!("q {:.6} 0 0 {:.6} 0 0 cm /Im Do Q\n", size.0, size.1);
        self.begin(content)?;
        writeln!(
            self.out,
            "<< /Length {} >>\nstream\n{}endstream\nendobj",
            commands.len(),
            commands
        )?;
        let mut z = ZlibEncoder::new(Vec::new(), Compression::fast());
        z.write_all(image.as_raw())?;
        let data = z.finish()?;
        self.begin(bitmap)?;
        writeln!(
            self.out,
            "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream",
            image.width(),
            image.height(),
            data.len()
        )?;
        self.out.write_all(&data)?;
        self.out.write_all(b"\nendstream\nendobj\n")?;
        self.pages.push(page);
        Ok(())
    }
    pub fn finish(mut self) -> Result<()> {
        self.begin(1)?;
        self.out
            .write_all(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n")?;
        self.begin(2)?;
        write!(
            self.out,
            "<< /Type /Pages /Count {} /Kids [",
            self.pages.len()
        )?;
        for id in &self.pages {
            write!(self.out, "{id} 0 R ")?;
        }
        self.out.write_all(b"] >>\nendobj\n")?;
        let xref = self.out.stream_position()?;
        writeln!(
            self.out,
            "xref\n0 {}\n0000000000 65535 f ",
            self.offsets.len()
        )?;
        for offset in &self.offsets[1..] {
            writeln!(self.out, "{offset:010} 00000 n ")?;
        }
        writeln!(
            self.out,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF",
            self.offsets.len()
        )?;
        self.out.flush()?;
        Ok(())
    }
}
