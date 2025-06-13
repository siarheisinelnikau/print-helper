//! Only invoked after the user presses Print and confirms the Windows print dialog.
use crate::{document::Catalog, layout::Project, output::render_sheet};
use anyhow::{Result, bail, ensure};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    Win32::{Foundation::GlobalFree, Graphics::Gdi::*, Storage::Xps::*, UI::Controls::Dialogs::*},
    core::w,
};

struct Dialog(PRINTDLGW);
impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            if !self.0.hDC.is_invalid() {
                let _ = DeleteDC(self.0.hDC);
            }
            if !self.0.hDevMode.is_invalid() {
                let _ = GlobalFree(Some(self.0.hDevMode));
            }
            if !self.0.hDevNames.is_invalid() {
                let _ = GlobalFree(Some(self.0.hDevNames));
            }
        }
    }
}

pub fn print(
    project: &Project,
    catalog: &Catalog,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, usize),
) -> Result<String> {
    project.validate()?;
    ensure!(!project.pages.is_empty(), "Add pages before printing");
    let mut dialog = Dialog(PRINTDLGW {
        lStructSize: std::mem::size_of::<PRINTDLGW>() as u32,
        Flags: PD_RETURNDC
            | PD_NOSELECTION
            | PD_NOPAGENUMS
            | PD_HIDEPRINTTOFILE
            | PD_DISABLEPRINTTOFILE
            | PD_USEDEVMODECOPIESANDCOLLATE,
        nCopies: 1,
        ..Default::default()
    });
    // The HDC and dialog-owned global blocks live until all GDI operations finish.
    unsafe {
        if !PrintDlgW(&mut dialog.0).as_bool() {
            let code = CommDlgExtendedError();
            if code.0 == 0 {
                return Ok("Printing cancelled".into());
            }
            bail!("Windows print dialog failed (0x{:x})", code.0);
        }
        let dc = dialog.0.hDC;
        ensure!(!dc.is_invalid(), "Printer did not return a drawing context");
        let cap = |index| GetDeviceCaps(Some(dc), index);
        let (dx, dy) = (cap(LOGPIXELSX), cap(LOGPIXELSY));
        ensure!(dx > 0 && dy > 0, "Printer resolution is unavailable");
        let (pw, ph) = (cap(PHYSICALWIDTH), cap(PHYSICALHEIGHT));
        let (ox, oy) = (cap(PHYSICALOFFSETX), cap(PHYSICALOFFSETY));
        let (rw, rh) = (cap(HORZRES), cap(VERTRES));
        let (expected_w, expected_h) = project.layout.paper_pt();
        ensure!(
            (pw as f64 / dx as f64 * 72.0 - expected_w).abs() <= 6.0
                && (ph as f64 / dy as f64 * 72.0 - expected_h).abs() <= 6.0,
            "Printer paper size/orientation differs from the preview. Select matching paper and orientation in printer Properties, then retry"
        );
        let margin = project.layout.margin_mm / 25.4;
        ensure!(
            margin * dx as f64 + 1.0 >= ox.max(pw - ox - rw) as f64
                && margin * dy as f64 + 1.0 >= oy.max(ph - oy - rh) as f64,
            "The printer cannot print within these margins. Increase margins to avoid clipping"
        );
        let info = DOCINFOW {
            cbSize: std::mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: w!("PrintHelper layout"),
            ..Default::default()
        };
        ensure!(StartDocW(dc, &info) > 0, "Printer rejected the job");
        let result: Result<()> = (|| {
            let count = project.layout.sheet_count(project.pages.len());
            for s in 0..count {
                ensure!(
                    !cancel.load(Ordering::Relaxed),
                    "Printing cancelled; already printed sheets cannot be recalled"
                );
                let image = render_sheet(project, catalog, s, project.layout.dpi as f64)?;
                let (w, h) = image.dimensions();
                let mut bgra = Vec::with_capacity(w as usize * h as usize * 4);
                for p in image.pixels() {
                    bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
                }
                let bitmap = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w as i32,
                        biHeight: -(h as i32),
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                ensure!(StartPage(dc) > 0, "Cannot start printer page");
                SetStretchBltMode(dc, HALFTONE);
                let scanlines = StretchDIBits(
                    dc,
                    -ox,
                    -oy,
                    pw,
                    ph,
                    0,
                    0,
                    w as i32,
                    h as i32,
                    Some(bgra.as_ptr().cast()),
                    &bitmap,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                ensure!(scanlines > 0, "Printer could not draw the sheet");
                ensure!(EndPage(dc) > 0, "Cannot finish printer page");
                progress(s + 1, count);
            }
            ensure!(EndDoc(dc) > 0, "Printer could not finish the job");
            Ok(())
        })();
        if result.is_err() {
            AbortDoc(dc);
        }
        result?;
    }
    Ok("Job sent to the Windows print spooler (physical output is not confirmed)".into())
}
