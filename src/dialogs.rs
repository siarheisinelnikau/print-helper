//! Synchronous native file pickers isolated from the document worker's MTA.
use crate::i18n::tr;
use anyhow::{Context, Result};
use std::path::PathBuf;

fn on_dialog_thread<T: Send + 'static>(
    action: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    // At most one picker exists: the bounded document worker waits for completion.
    // A fresh thread also prevents a previous WinRT/COM user from fixing its mode to MTA.
    let thread = std::thread::Builder::new()
        .name("PrintHelper file dialog".into())
        .spawn(move || {
            #[cfg(windows)]
            let _apartment = StaApartment::new()?;
            action()
        })
        .context("Cannot start the file dialog thread")?;
    thread
        .join()
        .map_err(|_| anyhow::anyhow!("The file dialog thread stopped unexpectedly"))?
}

#[cfg(windows)]
struct StaApartment;
#[cfg(windows)]
impl StaApartment {
    fn new() -> Result<Self> {
        use windows::Win32::System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
        };
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok() }
            .context("Cannot initialize Windows file dialogs in STA")?;
        Ok(Self)
    }
}
#[cfg(windows)]
impl Drop for StaApartment {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}

pub fn pick_inputs() -> Result<Option<Vec<PathBuf>>> {
    on_dialog_thread(|| {
        Ok(rfd::FileDialog::new()
            .add_filter(
                tr("PDF and images"),
                &["pdf", "png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp"],
            )
            .pick_files())
    })
}
pub fn open_layout() -> Result<Option<PathBuf>> {
    on_dialog_thread(|| {
        Ok(rfd::FileDialog::new()
            .add_filter(tr("PrintHelper project"), &["phlayout"])
            .pick_file())
    })
}
pub fn save_layout() -> Result<Option<PathBuf>> {
    on_dialog_thread(|| {
        Ok(rfd::FileDialog::new()
            .add_filter(tr("PrintHelper project"), &["phlayout"])
            .set_file_name("Layout.phlayout")
            .save_file())
    })
}
pub fn export_pdf() -> Result<Option<PathBuf>> {
    on_dialog_thread(|| {
        Ok(rfd::FileDialog::new()
            .add_filter(tr("PDF document"), &["pdf"])
            .set_file_name("PrintHelper-output.pdf")
            .save_file())
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Win32::System::{Com::*, WinRT::*};

    fn apartment() -> APTTYPE {
        let mut kind = APTTYPE_CURRENT;
        let mut qualifier = APTTYPEQUALIFIER::default();
        unsafe {
            CoGetApartmentType(&mut kind, &mut qualifier).unwrap();
        }
        kind
    }

    #[test]
    fn mta_worker_gets_sta_picker_without_changing_its_own_apartment() {
        std::thread::spawn(|| {
            unsafe {
                RoInitialize(RO_INIT_MULTITHREADED).unwrap();
            }
            struct MtaGuard;
            impl Drop for MtaGuard {
                fn drop(&mut self) {
                    unsafe {
                        RoUninitialize();
                    }
                }
            }
            let _mta = MtaGuard;
            assert_eq!(apartment(), APTTYPE_MTA);
            // Reproduce the exact call that rfd makes in the old worker path.
            let old =
                unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
            assert_eq!(old, windows::core::HRESULT(0x80010106u32 as i32));
            let caller = std::thread::current().id();
            on_dialog_thread(move || {
                assert_ne!(std::thread::current().id(), caller);
                assert!([APTTYPE_STA, APTTYPE_MAINSTA].contains(&apartment()));
                // This must now succeed, as rfd requires; no dialog object is created.
                unsafe {
                    CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
                    CoUninitialize();
                }
                Ok(())
            })
            .unwrap();
            assert_eq!(apartment(), APTTYPE_MTA);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn cancellation_and_setup_errors_are_distinct() {
        assert_eq!(on_dialog_thread(|| Ok(None::<PathBuf>)).unwrap(), None);
        let error =
            on_dialog_thread(|| -> Result<()> { anyhow::bail!("synthetic picker failure") })
                .unwrap_err();
        assert!(error.to_string().contains("synthetic picker failure"));
    }

    #[test]
    fn a_failed_dialog_thread_does_not_disable_the_next_request() {
        let error =
            on_dialog_thread(|| -> Result<()> { panic!("synthetic dialog panic") }).unwrap_err();
        assert!(error.to_string().contains("stopped unexpectedly"));
        let mode = on_dialog_thread(|| Ok(apartment())).unwrap();
        assert!([APTTYPE_STA, APTTYPE_MAINSTA].contains(&mode));
    }
}
