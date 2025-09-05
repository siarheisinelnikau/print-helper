use print_helper::updater::{self, Release};
use semver::Version;

fn release(version: &str) -> Release {
    serde_json::from_value(serde_json::json!({
        "tag_name": format!("v{version}"), "draft": false, "prerelease": false,
        "assets": [{"name": format!("PrintHelper-v{version}.exe"),
            "browser_download_url": format!("{}/releases/download/v{version}/PrintHelper-v{version}.exe", updater::REPOSITORY),
            "size": 10000, "digest": format!("sha256:{}", "a".repeat(64))}]
    })).unwrap()
}

#[test]
fn stable_version_order_is_numeric_and_prereleases_are_excluded() {
    let current = Version::parse("0.9.0").unwrap();
    assert!(
        updater::select(release("0.10.0"), &current)
            .unwrap()
            .is_some()
    );
    assert!(
        updater::select(release("0.8.0"), &current)
            .unwrap()
            .is_none()
    );
    assert!(
        updater::select(release("0.9.0"), &current)
            .unwrap()
            .is_none()
    );
    assert!(
        updater::select(release("1.0.0-beta.1"), &current)
            .unwrap()
            .is_none()
    );
    let mut draft = release("1.0.0");
    draft.draft = true;
    assert!(updater::select(draft, &current).unwrap().is_none());
}

#[test]
fn build_metadata_does_not_trigger_reinstallation_and_repeated_tag_prefixes_fail() {
    let current = Version::parse("0.9.0+build.1").unwrap();
    assert!(
        updater::select(release("0.9.0+build.2"), &current)
            .unwrap()
            .is_none()
    );
    assert!(
        updater::select(release("0.9.0"), &current)
            .unwrap()
            .is_none()
    );
    let mut r = release("0.10.0");
    r.tag_name = "vv0.10.0".into();
    assert!(updater::select(r, &current).is_err());
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

#[test]
fn incompatible_missing_untrusted_and_unverifiable_assets_fail_closed() {
    let current = Version::parse("0.1.0").unwrap();
    let mut r = release("0.2.0");
    r.assets.clear();
    assert!(updater::select(r, &current).is_err());
    let mut r = release("0.2.0");
    r.assets[0].browser_download_url = "https://example.com/a.exe".into();
    assert!(updater::select(r, &current).is_err());
    let mut r = release("0.2.0");
    r.assets[0].digest = None;
    assert!(updater::select(r, &current).is_err());
    let mut r = release("0.2.0");
    r.assets[0].digest = Some("sha256:wrong".into());
    assert!(updater::select(r, &current).is_err());
    let mut r = release("0.2.0");
    r.assets[0].size = 201_000_000;
    assert!(updater::select(r, &current).is_err());
}

#[test]
fn downloaded_pe_requires_matching_architecture_and_digest() {
    use sha2::{Digest, Sha256};
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-output");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("synthetic-pe-{}.bin", std::process::id()));
    let mut bytes = test_pe();
    let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
    std::fs::write(&path, &bytes).unwrap();
    assert!(updater::validate_executable(&path, &hash).is_ok());
    assert!(updater::validate_executable(&path, &format!("sha256:{}", "0".repeat(64))).is_err());
    bytes[132] = 0x4c;
    bytes[133] = 0x01;
    let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
    std::fs::write(&path, bytes).unwrap();
    assert!(updater::validate_executable(&path, &hash).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_pe_headers_and_sections_are_rejected_even_with_matching_hashes() {
    use sha2::{Digest, Sha256};
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-output");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("malformed-pe-{}.bin", std::process::id()));
    let mut cases = Vec::new();
    // A matching digest and machine field alone do not make a runnable application.
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
    bytes[128..134].copy_from_slice(b"PE\0\0\x64\x86");
    cases.push(bytes);
    for (offset, value) in [
        (134, 0_u16),
        (148, 32),
        (150, 0x2022),
        (152, 0x10b),
        (220, 3),
    ] {
        let mut bytes = test_pe();
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        cases.push(bytes);
    }
    for (offset, value) in [
        (60, u32::MAX),
        (168, 0_u32),
        (212, 300),
        (260, 17),
        (408, u32::MAX),
        (412, 1000),
        (428, 0),
    ] {
        let mut bytes = test_pe();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        cases.push(bytes);
    }
    for bytes in cases {
        let hash = format!("sha256:{:x}", Sha256::digest(&bytes));
        std::fs::write(&path, &bytes).unwrap();
        assert!(updater::validate_executable(&path, &hash).is_err());
    }
    std::fs::remove_file(path).unwrap();
}
