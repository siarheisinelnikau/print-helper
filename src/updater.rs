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
    let version = Version::parse(release.tag_name.trim_start_matches('v'))?;
    if release.draft || release.prerelease || !version.pre.is_empty() || version <= *installed {
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
        (1024..=200_000_000).contains(&asset.size),
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
    let bytes = std::fs::read(path)?;
    ensure!(
        bytes.len() >= 1024 && bytes.len() <= 200_000_000 && &bytes[..2] == b"MZ",
        "Update is not a Windows executable"
    );
    let pe = u32::from_le_bytes(bytes[60..64].try_into()?) as usize;
    ensure!(
        pe <= bytes.len() - 6
            && &bytes[pe..pe + 4] == b"PE\0\0"
            && bytes[pe + 4..pe + 6] == [0x64, 0x86],
        "Update is not an x64 PE executable"
    );
    ensure!(
        format!("sha256:{:x}", Sha256::digest(&bytes)).eq_ignore_ascii_case(expected),
        "Update checksum does not match; installation stopped"
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
    {
        let backup = target.with_extension(format!("exe.previous-{pid}"));
        ensure!(
            !backup.exists(),
            "A previous executable backup already exists; move it aside before updating"
        );
        let replacement = target.with_extension("exe.next");
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&replacement)?;
        std::io::copy(&mut std::fs::File::open(&current)?, &mut output)?;
        output.sync_all()?;
        drop(output);
        validate_executable(&replacement, digest)?;
        if let Err(e) = std::fs::rename(&target, &backup) {
            let _ = std::fs::remove_file(&replacement);
            return Err(e.into());
        }
        if let Err(e) = std::fs::rename(&replacement, &target) {
            let _ = std::fs::rename(&backup, &target);
            let _ = std::fs::remove_file(&replacement);
            return Err(e.into());
        }
        std::process::Command::new(&target)
            .arg("--updated")
            .arg(&current)
            .spawn()?;
        Ok(())
    }
}
