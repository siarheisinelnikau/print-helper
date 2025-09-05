use anyhow::{Context, Result, ensure};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

mod install;
const MAX_ASSET_SIZE: u64 = 200_000_000;

pub const REPOSITORY: &str = "https://github.com/siarheisinelnikau/print-helper";
const API: &str = "https://api.github.com/repos/siarheisinelnikau/print-helper/releases/latest";

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    pub digest: Option<String>,
}
#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug)]
pub struct Update {
    pub version: Version,
    pub asset: Asset,
}

pub fn select(release: Release, installed: &Version) -> Result<Option<Update>> {
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )?;
    if release.draft
        || release.prerelease
        || !version.pre.is_empty()
        || !version.cmp_precedence(installed).is_gt()
    {
        return Ok(None);
    }
    ensure!(
        cfg!(target_arch = "x86_64") && cfg!(windows),
        "Updates are currently packaged for Windows x64 only"
    );
    let name = format!("PrintHelper-v{version}.exe");
    let asset = release
        .assets
        .into_iter()
        .find(|a| a.name == name)
        .context("New release has no compatible Windows x64 standalone asset")?;
    ensure!(
        asset.browser_download_url
            == format!(
                "{REPOSITORY}/releases/download/{}/{}",
                release.tag_name, name
            ),
        "Unexpected update asset URL"
    );
    ensure!(
        (1024..=MAX_ASSET_SIZE).contains(&asset.size),
        "Invalid update asset size"
    );
    let digest = asset
        .digest
        .as_deref()
        .context("Release has no SHA-256 digest; automatic installation is unavailable")?;
    ensure!(valid_digest(digest), "Release SHA-256 digest is invalid");
    Ok(Some(Update { version, asset }))
}
fn valid_digest(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
}
fn agent(seconds: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(seconds)))
        .build()
        .into()
}
pub fn check() -> Result<Option<Update>> {
    let mut response = agent(20).get(API).header("User-Agent", concat!("PrintHelper/", env!("CARGO_PKG_VERSION"))).header("Accept", "application/vnd.github+json").call()
        .context("Update check unavailable: network error, API limit, or repository/release not published")?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(1_000_000)
        .read_to_vec()?;
    select(
        serde_json::from_slice(&bytes)?,
        &Version::parse(env!("CARGO_PKG_VERSION"))?,
    )
}

pub fn validate_executable(path: &Path, expected: &str) -> Result<()> {
    ensure!(valid_digest(expected), "Invalid expected digest");
    let bytes = read_bounded(path)?;
    validate_pe(&bytes)?;
    ensure!(
        format!("sha256:{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected),
        "Update checksum does not match; installation stopped"
    );
    Ok(())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.is_file() && file.metadata()?.len() <= MAX_ASSET_SIZE,
        "Executable exceeds the update size limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_ASSET_SIZE + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_ASSET_SIZE,
        "Executable exceeds the update size limit"
    );
    Ok(bytes)
}

fn validate_pe(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() >= 1024 && &bytes[..2] == b"MZ",
        "Update is not a Windows executable"
    );
    let u16_at =
        |i: usize| u16::from_le_bytes(bytes[i..i + 2].try_into().expect("checked PE range"));
    let u32_at = |i: usize| {
        u32::from_le_bytes(bytes[i..i + 4].try_into().expect("checked PE range")) as usize
    };
    let pe = u32_at(60);
    ensure!(
        pe >= 64
            && pe <= bytes.len() - 24
            && &bytes[pe..pe + 4] == b"PE\0\0"
            && u16_at(pe + 4) == 0x8664,
        "Update is not an x64 PE executable"
    );
    let sections = u16_at(pe + 6) as usize;
    let flags = u16_at(pe + 22);
    let optional_len = u16_at(pe + 20) as usize;
    let optional = pe + 24;
    ensure!(
        (1..=96).contains(&sections) && flags & 0x0002 != 0 && flags & 0x2000 == 0,
        "Update must be an executable image, not a DLL or object file"
    );
    let table = optional + optional_len;
    ensure!(
        optional_len >= 112 && table <= bytes.len() && sections * 40 <= bytes.len() - table,
        "Truncated PE optional header or section table"
    );
    ensure!(
        u16_at(optional) == 0x20b && u16_at(optional + 68) == 2,
        "Update must use the x64 Windows GUI subsystem"
    );
    let headers = u32_at(optional + 60);
    let image_size = u32_at(optional + 56);
    let entry = u32_at(optional + 16);
    let directories = u32_at(optional + 108);
    ensure!(
        headers >= table + sections * 40
            && headers <= bytes.len()
            && image_size >= headers
            && entry > 0
            && entry < image_size
            && directories <= (optional_len - 112) / 8,
        "Invalid PE image/header bounds"
    );
    let mut executable_entry = false;
    for n in 0..sections {
        let section = table + n * 40;
        let virtual_size = u32_at(section + 8);
        let address = u32_at(section + 12);
        let raw_size = u32_at(section + 16);
        let raw = u32_at(section + 20);
        ensure!(
            address <= image_size
                && virtual_size.max(raw_size) <= image_size - address
                && (raw_size == 0
                    || (raw >= headers && raw <= bytes.len() && raw_size <= bytes.len() - raw)),
            "PE section exceeds the downloaded image"
        );
        if entry >= address && entry - address < raw_size && u32_at(section + 36) & 0x2000_0000 != 0
        {
            executable_entry = true;
        }
    }
    ensure!(
        executable_entry,
        "PE entry point is outside executable file data"
    );
    Ok(())
}

pub fn download(
    update: &Update,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf> {
    let installed = std::env::current_exe()?.canonicalize()?;
    let staged = installed
        .parent()
        .context("Executable directory is unavailable")?
        .join(format!(
            ".printhelper-update-{}-{}.exe",
            update.version,
            std::process::id()
        ));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&staged)
        .context(
            "Cannot stage update beside this executable. Move PrintHelper to a writable folder",
        )?;
    let result = (|| {
        let response = agent(180)
            .get(&update.asset.browser_download_url)
            .header("User-Agent", "PrintHelper")
            .call()?;
        let mut reader = response.into_body().into_reader();
        let mut buf = [0u8; 65536];
        let mut total = 0;
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "Update download cancelled");
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(total <= update.asset.size, "Update exceeds declared size");
            file.write_all(&buf[..n])?;
            progress(total, update.asset.size);
        }
        ensure!(total == update.asset.size, "Update download is incomplete");
        file.sync_all()?;
        drop(file);
        validate_executable(
            &staged,
            update.asset.digest.as_deref().context("Missing digest")?,
        )?;
        ensure!(!cancel.load(Ordering::Relaxed), "Update download cancelled");
        Ok(())
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    Ok(staged)
}

pub fn launch_install(staged: &Path, digest: &str) -> Result<()> {
    validate_executable(staged, digest)?;
    std::process::Command::new(staged)
        .arg("--finish-update")
        .arg(std::env::current_exe()?.canonicalize()?)
        .arg(std::process::id().to_string())
        .arg(digest)
        .spawn()
        .context("Cannot start verified update helper")?;
    Ok(())
}

/// Runs only in the verified new executable, never during tests or normal startup.
pub fn finish_install(target: &Path, pid: u32, digest: &str) -> Result<()> {
    let current = std::env::current_exe()?.canonicalize()?;
    let target = target.canonicalize()?;
    ensure!(
        current.parent() == target.parent() && current != target,
        "Update target must be a different executable in the same directory"
    );
    ensure!(
        target
            .extension()
            .is_some_and(|s| s.eq_ignore_ascii_case("exe")),
        "Update target must be an executable"
    );
    ensure!(
        current
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".printhelper-update-")),
        "Unexpected updater path"
    );
    validate_executable(&current, digest)?;
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::{CloseHandle, WAIT_OBJECT_0},
            System::Threading::*,
        };
        match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(process) => {
                let wait = WaitForSingleObject(process, 60_000);
                CloseHandle(process)?;
                ensure!(
                    wait == WAIT_OBJECT_0,
                    "Previous process has not exited; installation was not changed"
                );
            }
            // The original process may exit before the helper finishes initializing.
            Err(e) if e.code() == windows::core::HRESULT::from_win32(87) => {}
            Err(e) => return Err(e).context("Cannot wait for the previous process"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        anyhow::bail!("Installation requires Windows");
    }
    #[allow(unreachable_code)]
    install::finish(&target, &current, pid, digest)
}

pub fn record_install_error(message: &str) -> Result<()> {
    let current = std::env::current_exe()?;
    ensure!(
        current
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".printhelper-update-")),
        "Not an update helper"
    );
    crate::output::save_new(&current.with_extension("error.txt"), message.as_bytes())
}
