use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub startup_updates: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            startup_updates: true,
        }
    }
}
pub fn path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("PrintHelper")
        .join("settings.json")
}
pub fn load() -> Result<Settings> {
    match std::fs::read(path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)
            .context("Settings are unreadable; startup update checks were skipped")?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.into()),
    }
}
pub fn save(settings: &Settings) -> Result<()> {
    let path = path();
    std::fs::create_dir_all(path.parent().context("Missing settings directory")?)?;
    let mut temp = crate::output::NewFile::beside(&path)?;
    use std::io::Write;
    temp.file.write_all(&serde_json::to_vec_pretty(settings)?)?;
    temp.file.sync_all()?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };
        let from: Vec<u16> = temp.path.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
    }
    #[cfg(not(windows))]
    std::fs::rename(&temp.path, path)?;
    Ok(())
}
