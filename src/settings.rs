use crate::i18n::Language;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub startup_updates: bool,
    pub language: Language,
    pub theme: Theme,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            startup_updates: true,
            language: Language::English,
            theme: Theme::Light,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_keep_update_preferences_and_gain_english_light_defaults() {
        let old: Settings = serde_json::from_str(r#"{"startup_updates":false}"#).unwrap();
        assert!(!old.startup_updates);
        assert_eq!(old.language, Language::English);
        assert_eq!(old.theme, Theme::Light);
        for language in Language::ALL {
            for theme in [Theme::Light, Theme::Dark] {
                let settings = Settings {
                    language,
                    theme,
                    ..old.clone()
                };
                let restored: Settings =
                    serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
                assert_eq!(restored.language, language);
                assert_eq!(restored.theme, theme);
                assert!(!restored.startup_updates);
            }
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
    temp.file()?
        .write_all(&serde_json::to_vec_pretty(settings)?)?;
    temp.seal()?;
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
