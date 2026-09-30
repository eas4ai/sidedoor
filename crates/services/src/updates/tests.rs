use super::*;

fn installation(owner: Owner, os: &str, arch: &str) -> Installation {
    Installation {
        owner,
        os: os.into(),
        arch: arch.into(),
        root: PathBuf::from("/app"),
        executable: PathBuf::from("/app/sidedoor"),
    }
}

#[test]
fn versions_use_semver_and_never_downgrade() {
    assert!(newer("v0.10.0", "0.9.0").unwrap());
    assert!(newer("v1.0.0", "1.0.0-beta.1").unwrap());
    assert!(!newer("v1.0.0-beta.1", "1.0.0").unwrap());
    assert!(!newer("v1.0.0", "1.0.0").unwrap());
    assert!(newer("invalid", "1.0.0").is_err());
}

#[test]
fn downloads_must_match_the_full_digest_and_exact_size() {
    let asset = ManifestAsset {
        name: "package.zip".into(),
        size: 5,
        sha256: format!("{:x}", Sha256::digest(b"whole")),
    };
    for (bytes, valid) in [
        (b"whole".as_slice(), true),
        (b"whol".as_slice(), false),
        (b"whole extra".as_slice(), false),
        (b"wrong".as_slice(), false),
    ] {
        let staging = tempfile::tempdir().unwrap();
        let package = staging.path().join("package.zip");
        assert_eq!(copy_verified(bytes, &package, &asset).is_ok(), valid);
    }
}

#[test]
fn each_installation_uses_its_own_complete_package() {
    for (owner, os, arch, name) in [
        (
            Owner::MacBundle,
            "macos",
            "arm64",
            "Sidedoor-macos-arm64.zip",
        ),
        (
            Owner::WindowsMsi,
            "windows",
            "x64",
            "Sidedoor-windows-x64.msi",
        ),
        (
            Owner::WindowsSetup,
            "windows",
            "x64",
            "Sidedoor-windows-x64-setup.exe",
        ),
        (
            Owner::Portable,
            "windows",
            "x64",
            "Sidedoor-windows-x64.zip",
        ),
        (
            Owner::Debian,
            "linux",
            "x86_64",
            "Sidedoor-linux-x86_64.deb",
        ),
        (Owner::Rpm, "linux", "x86_64", "Sidedoor-linux-x86_64.rpm"),
        (
            Owner::Portable,
            "linux",
            "x86_64",
            "Sidedoor-linux-x86_64.tar.gz",
        ),
    ] {
        assert_eq!(installation(owner, os, arch).asset_name(), name);
    }
}

fn metadata() -> (GithubRelease, Manifest, Installation) {
    let installation = installation(Owner::MacBundle, "macos", "arm64");
    let name = installation.asset_name();
    let release = GithubRelease {
        tag_name: "v1.0.0".into(),
        body: None,
        draft: false,
        prerelease: false,
        assets: vec![GithubAsset {
            name: name.clone(),
            size: 42,
            browser_download_url: format!("{REPO}download/v1.0.0/{name}"),
        }],
    };
    let manifest = Manifest {
        schema_version: 1,
        version: "1.0.0".into(),
        assets: vec![ManifestAsset {
            name,
            size: 42,
            sha256: "a".repeat(64),
        }],
    };
    (release, manifest, installation)
}

#[test]
fn selection_pins_identity_target_digest_size_and_version() {
    let (release, manifest, installation) = metadata();
    assert!(select(release, manifest, installation).is_ok());
    let (release, mut manifest, installation) = metadata();
    manifest.version = "2.0.0".into();
    assert!(select(release, manifest, installation).is_err());
    let (mut release, manifest, installation) = metadata();
    release.assets[0].browser_download_url = "http://evil.example/app.zip".into();
    assert!(select(release, manifest, installation).is_err());
    let (release, mut manifest, installation) = metadata();
    manifest.assets[0].size = 43;
    assert!(select(release, manifest, installation).is_err());
    let (release, mut manifest, installation) = metadata();
    manifest.assets[0].sha256 = "bad".into();
    assert!(select(release, manifest, installation).is_err());
    let (release, manifest, mut installation) = metadata();
    installation.arch = "x64".into();
    assert!(select(release, manifest, installation).is_err());
    let (release, mut manifest, installation) = metadata();
    manifest.assets.push(manifest.assets[0].clone());
    assert!(select(release, manifest, installation).is_err());
}

#[test]
fn archive_cannot_escape_staging_or_omit_runtime_files() {
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;
    let dir = tempfile::tempdir().unwrap();
    for (index, name) in [
        "../outside",
        "/absolute",
        "Sidedoor.app/../../outside",
        "Sidedoor.app/bad\\path",
        "Sidedoor.app/Contents/Info.plist",
    ]
    .iter()
    .enumerate()
    {
        let staging = dir.path().join(index.to_string());
        fs::create_dir(&staging).unwrap();
        let package = staging.join("update.zip");
        let mut zip = zip::ZipWriter::new(File::create(&package).unwrap());
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(b"fake").unwrap();
        zip.finish().unwrap();
        assert!(
            archive::extract(
                &package,
                &staging,
                &installation(Owner::MacBundle, "macos", "arm64")
            )
            .is_err()
        );
    }
    assert!(!dir.path().join("outside").exists());
}

#[test]
fn complete_archives_extract_but_links_and_colliding_paths_are_rejected() {
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;
    for invalid in [None, Some("link"), Some("collision")] {
        let staging = tempfile::tempdir().unwrap();
        let package = staging.path().join("update.zip");
        let mut zip = zip::ZipWriter::new(File::create(&package).unwrap());
        for name in [
            "Sidedoor.app/Contents/Info.plist",
            "Sidedoor.app/Contents/MacOS/sidedoor",
            "Sidedoor.app/Contents/MacOS/bun",
            "Sidedoor.app/Contents/Resources/sdk/package.json",
            "Sidedoor.app/Contents/Resources/sdk/src/index.ts",
            "Sidedoor.app/Contents/Resources/builtins/weather/index.js",
            "Sidedoor.app/Contents/Resources/builtins/clipboard/index.js",
            "Sidedoor.app/Contents/Resources/builtins/stats/index.js",
        ] {
            zip.start_file(name, SimpleFileOptions::default().unix_permissions(0o755))
                .unwrap();
            zip.write_all(b"payload").unwrap();
        }
        if invalid == Some("link") {
            zip.add_symlink(
                "Sidedoor.app/Contents/escape",
                "../../../outside",
                SimpleFileOptions::default(),
            )
            .unwrap();
        } else if invalid == Some("collision") {
            zip.start_file(
                "Sidedoor.app/Contents/MacOS/SIDEDOOR",
                SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(b"collision").unwrap();
        }
        zip.finish().unwrap();
        let result = archive::extract(
            &package,
            staging.path(),
            &installation(Owner::MacBundle, "macos", "arm64"),
        );
        assert_eq!(result.is_ok(), invalid.is_none());
        if let Ok(payload) = result {
            assert_eq!(
                fs::read(payload.join("Contents/MacOS/sidedoor")).unwrap(),
                b"payload"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(
                    fs::metadata(payload.join("Contents/MacOS/sidedoor"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o755
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn portable_helper_replaces_the_whole_folder_and_rolls_back_a_failed_start() {
    use std::os::unix::fs::PermissionsExt as _;
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("installed app");
        let stage = dir.path().join("staged update");
        let payload = stage.join("payload");
        fs::create_dir(&destination).unwrap();
        fs::create_dir_all(&payload).unwrap();
        let executable = destination.join("sidedoor");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        fs::write(destination.join("old-resource"), "old").unwrap();
        let new = payload.join("sidedoor");
        fs::write(
            &new,
            if fail {
                "#!/bin/sh\nexit 1\n"
            } else {
                "#!/bin/sh\nprintf '2.0.0' > \"$SIDEDOOR_UPDATE_RECEIPT\"\nsleep 2\n"
            },
        )
        .unwrap();
        for path in [&executable, &new] {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(payload.join("new-resource"), "new").unwrap();
        let package = stage.join("package");
        fs::write(&package, "verified package").unwrap();
        let hash = format!("{:x}", Sha256::digest(b"verified package"));
        let helper = stage.join("helper.sh");
        fs::write(&helper, include_str!("helpers/install.sh")).unwrap();
        let status = Command::new("/bin/sh")
            .arg(helper)
            .arg("2147483647")
            .arg(&destination)
            .arg(&payload)
            .arg(&executable)
            .arg(stage.join("receipt"))
            .arg("2.0.0")
            .arg("portable")
            .arg(dir.path().join("error"))
            .arg(hash)
            .arg(package)
            .arg(stage.join("helper-ready"))
            .status()
            .unwrap();
        assert_eq!(status.success(), !fail);
        assert_eq!(destination.join("old-resource").exists(), fail);
        assert_eq!(destination.join("new-resource").exists(), !fail);
        assert!(!stage.exists());
        assert!(!dir.path().join("installed app.sidedoor-backup").exists());
    }
}
