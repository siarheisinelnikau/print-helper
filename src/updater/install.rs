use super::*;
use crate::output::NewFile;

/// File transaction separated from process control so failures can be tested with
/// synthetic files and a non-executing restart callback.
fn replace_and_restart(
    staged: &Path,
    target: &Path,
    backup: &Path,
    digest: &str,
    restart: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    validate_executable(staged, digest)?;
    ensure!(
        staged != target && backup != target && backup != staged,
        "Update paths must be distinct"
    );
    ensure!(
        !backup.exists(),
        "Backup already exists: {}",
        backup.display()
    );

    let mut replacement = NewFile::beside(target)?;
    replacement.file()?.write_all(&read_bounded(staged)?)?;
    replacement.seal()?;
    validate_executable(&replacement.path, digest)?;

    // Copy and flush the old installation before changing its name or contents.
    // The backup is independent of ReplaceFile's partially completed failure modes.
    let original = read_bounded(target)?;
    let mut previous = NewFile::beside(backup)?;
    previous.file()?.write_all(&original)?;
    previous.publish(backup)?;

    let result = replace_existing(&replacement.path, target).and_then(|()| restart(target));
    if let Err(error) = result {
        let restore = (|| -> Result<()> {
            let mut restored = NewFile::beside(target)?;
            restored.file()?.write_all(&original)?;
            restored.seal()?;
            restore_existing(&restored.path, target)
        })();
        if let Err(restore_error) = restore {
            anyhow::bail!(
                "{error:#}. Automatic rollback failed: {restore_error:#}. Recover the previous executable from {}",
                backup.display()
            );
        }
        return Err(error).context("Update failed; the previous executable was restored");
    }
    Ok(())
}

fn replace_existing(replacement: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use windows::{
            Win32::Storage::FileSystem::{REPLACE_FILE_FLAGS, ReplaceFileW},
            core::PCWSTR,
        };
        let replacement = wide(replacement);
        let target = wide(target);
        // One OS replacement operation preserves the destination's ACLs/identity.
        // REPLACEFILE_WRITE_THROUGH is unsupported; staging/backup were flushed above.
        unsafe {
            ReplaceFileW(
                PCWSTR(target.as_ptr()),
                PCWSTR(replacement.as_ptr()),
                PCWSTR::null(),
                REPLACE_FILE_FLAGS(0),
                None,
                None,
            )?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(replacement, target)?;
        Ok(())
    }
}

fn restore_existing(replacement: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };
        let replacement = wide(replacement);
        let target = wide(target);
        // Also restores a target missing after an incomplete Windows replacement.
        unsafe {
            MoveFileExW(
                PCWSTR(replacement.as_ptr()),
                PCWSTR(target.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )?;
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(replacement, target)?;
        Ok(())
    }
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

pub(super) fn finish(target: &Path, staged: &Path, pid: u32, digest: &str) -> Result<()> {
    // This executes only after the parent process was confirmed to have exited.
    let original = read_bounded(target)?;
    validate_pe(&original)?;
    let old_digest = format!("sha256:{:x}", Sha256::digest(&original));
    drop(original);
    let result = replace_and_restart(
        staged,
        target,
        &target.with_extension(format!("exe.previous-{pid}")),
        digest,
        |target| {
            std::process::Command::new(target)
                .arg("--updated")
                .arg(staged)
                .spawn()?;
            Ok(())
        },
    );
    if let Err(error) = &result {
        // Only relaunch a verified original after parent exit and successful rollback.
        // Never execute the unchecked CLI path in main's generic error handler.
        validate_executable(target, &old_digest)
            .context("Previous executable is unavailable; recover the retained backup manually")?;
        std::process::Command::new(target)
            .arg("--update-error")
            .arg(format!("{error:#}"))
            .spawn()
            .context("Previous executable was restored but could not be restarted")?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(".test-output")
                .join(format!(
                    "install-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
        fn staged(&self) -> (PathBuf, String, Vec<u8>) {
            let bytes = test_pe();
            let digest = format!("sha256:{:x}", Sha256::digest(&bytes));
            let path = self.path("staged.bin");
            std::fs::write(&path, &bytes).unwrap();
            (path, digest, bytes)
        }
        fn assert_no_temporary_files(&self) {
            assert!(
                !std::fs::read_dir(&self.0).unwrap().any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp"))
            );
        }
    }
    fn test_pe() -> Vec<u8> {
        let mut bytes = vec![0; 1024];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        for (offset, value) in [
            (132, 0x8664_u16),
            (134, 1),
            (148, 240),
            (150, 0x22),
            (152, 0x20b),
            (220, 2),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (168, 0x1000_u32),
            (208, 0x2000),
            (212, 512),
            (260, 16),
            (400, 1),
            (404, 0x1000),
            (408, 512),
            (412, 512),
            (428, 0x6000_0020),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[512] = 0xc3;
        bytes
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for entry in std::fs::read_dir(&self.0).unwrap() {
                let _ = std::fs::remove_file(entry.unwrap().path());
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn restart_failure_restores_old_contents_and_preserves_backup() {
        let f = Fixture::new();
        let (staged, digest, bytes) = f.staged();
        let target = f.path("installed.bin");
        let backup = f.path("previous.bin");
        std::fs::write(&target, b"previous synthetic installation").unwrap();
        let mut restart_called = false;
        let error = replace_and_restart(&staged, &target, &backup, &digest, |path| {
            restart_called = true;
            assert_eq!(std::fs::read(path).unwrap(), bytes);
            anyhow::bail!("simulated process creation failure")
        })
        .unwrap_err();
        assert!(
            restart_called,
            "test must reach restart, not an earlier replacement error"
        );
        assert!(format!("{error:#}").contains("previous executable was restored"));
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"previous synthetic installation"
        );
        assert_eq!(
            std::fs::read(backup).unwrap(),
            b"previous synthetic installation"
        );
        f.assert_no_temporary_files();
    }

    #[test]
    fn successful_file_transaction_retains_backup_without_executing_anything() {
        let f = Fixture::new();
        let (staged, digest, bytes) = f.staged();
        let target = f.path("installed.bin");
        let backup = f.path("previous.bin");
        std::fs::write(&target, b"old").unwrap();
        let mut called = false;
        replace_and_restart(&staged, &target, &backup, &digest, |path| {
            called = true;
            assert_eq!(std::fs::read(path).unwrap(), bytes);
            Ok(())
        })
        .unwrap();
        assert!(called);
        assert_eq!(std::fs::read(target).unwrap(), bytes);
        assert_eq!(std::fs::read(backup).unwrap(), b"old");
        f.assert_no_temporary_files();
    }

    #[test]
    fn checksum_and_backup_conflicts_leave_existing_files_untouched() {
        let f = Fixture::new();
        let (staged, digest, _) = f.staged();
        let target = f.path("installed.bin");
        let backup = f.path("previous.bin");
        std::fs::write(&target, b"old").unwrap();
        assert!(
            replace_and_restart(
                &staged,
                &target,
                &backup,
                &format!("sha256:{}", "0".repeat(64)),
                |_| panic!("must not restart")
            )
            .is_err()
        );
        assert!(!backup.exists());
        std::fs::write(&backup, b"retained backup").unwrap();
        assert!(
            replace_and_restart(&staged, &target, &backup, &digest, |_| panic!(
                "must not restart"
            ))
            .is_err()
        );
        assert_eq!(std::fs::read(target).unwrap(), b"old");
        assert_eq!(std::fs::read(backup).unwrap(), b"retained backup");
        f.assert_no_temporary_files();
    }
}
