//! PrintHelper configuration and compatibility with its released helper protocol.
use anyhow::{Context, Result, ensure};
pub use rust_auto_updator::{Prepared, Release as Update};
use semver::Version;
use std::{path::Path, sync::atomic::AtomicBool};
pub type Release = serde_json::Value;
pub const REPOSITORY: &str = "https://github.com/siarheisinelnikau/print-helper";

pub fn config() -> rust_auto_updator::Config {
    let mut config = rust_auto_updator::Config::new(
        "siarheisinelnikau",
        "print-helper",
        env!("CARGO_PKG_VERSION"),
        "PrintHelper",
    )
    .expect("valid PrintHelper update configuration");
    config.max_executable_size = 200_000_000;
    config.expected_subsystem = Some(2);
    config.allow_asset_digest = true;
    config.install.stage_prefix = ".printhelper-update-".into();
    config.install.helper_name = "PrintHelper.exe".into();
    config.install.startup_health_timeout = Some(std::time::Duration::from_secs(30));
    config
}

pub fn select(release: Release, installed: &Version) -> Result<Option<Update>> {
    let mut config = config();
    config.current_version = installed.clone();
    rust_auto_updator::parse_release(&release.to_string(), &config).map_err(anyhow::Error::msg)
}

pub fn check() -> Result<Option<Update>> {
    rust_auto_updator::check_latest(&config()).map_err(anyhow::Error::msg)
}

pub fn download(
    update: &Update,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, u64),
) -> Result<Prepared> {
    rust_auto_updator::prepare_download_cancellable(update, &config(), cancel, progress)
        .map_err(anyhow::Error::msg)
}

pub fn launch_install(prepared: &mut Prepared) -> Result<()> {
    rust_auto_updator::handoff(prepared).map_err(anyhow::Error::msg)
}

pub fn validate_executable(path: &Path, digest: &str) -> Result<()> {
    let config = config();
    let length = path.metadata()?.len();
    ensure!(
        length <= config.max_executable_size,
        "Executable exceeds the update size limit"
    );
    let expected = digest
        .strip_prefix("sha256:")
        .context("Invalid update digest")?;
    rust_auto_updator::verify_executable(path, expected, length, config.architecture)
        .map_err(anyhow::Error::msg)?;
    rust_auto_updator::verify_pe_subsystem(path, config.architecture, config.expected_subsystem)
        .map_err(anyhow::Error::msg)
}

/// Keep previously released --finish-update calls working without a second installer.
pub fn finish_install(target: &Path, pid: u32, digest: &str) -> Result<()> {
    let staged = std::env::current_exe()?.canonicalize()?;
    let target = target.canonicalize()?;
    ensure!(
        staged.parent() == target.parent() && staged != target,
        "Invalid legacy update target"
    );
    ensure!(
        staged
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(".printhelper-update-")),
        "Unexpected updater path"
    );
    let mut config = config();
    config.install.restart_args = vec!["--updated".into(), staged.to_string_lossy().into_owned()];
    rust_auto_updator::apply_legacy_update(
        &config,
        &target,
        &staged,
        digest
            .strip_prefix("sha256:")
            .context("Invalid update digest")?,
        pid,
        None,
    )
    .map_err(anyhow::Error::msg)
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

/// Cleanup starts only after the updated native UI has initialized successfully.
pub fn cleanup_after_startup() {
    let args: Vec<_> = std::env::args_os().collect();
    if let Some(index) = args.iter().position(|arg| arg == "--cleanup-update")
        && let Some(directory) = args.get(index + 1)
    {
        rust_auto_updator::cleanup_after_update(directory.into(), config().install);
    }
    // Older versions pass the legacy flat helper for narrowly scoped cleanup.
    if args.get(1).is_some_and(|arg| arg == "--updated")
        && let (Some(staged), Ok(current)) = (
            args.get(2),
            std::env::current_exe().and_then(|p| p.canonicalize()),
        )
        && let Ok(staged) = std::path::PathBuf::from(staged).canonicalize()
        && staged != current
        && staged.parent() == current.parent()
        && staged
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && staged
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".printhelper-update-"))
    {
        let _ = std::fs::remove_file(staged);
    }
}
