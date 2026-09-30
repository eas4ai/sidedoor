use super::{Installation, Owner, bun_name};
use flate2::read::GzDecoder;
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

const MAX_EXPANDED: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 20_000;

pub(super) fn extract(
    package: &Path,
    staging: &Path,
    installation: &Installation,
) -> Result<PathBuf, String> {
    let root = match installation.owner {
        Owner::MacBundle => "Sidedoor.app".to_string(),
        _ if installation.os == "windows" => "Sidedoor".to_string(),
        _ => format!("Sidedoor-linux-{}", installation.arch),
    };
    let into = staging.join("unpacked");
    fs::create_dir(&into).map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    let mut total = 0;
    if installation.os == "macos" || installation.os == "windows" {
        let mut zip = zip::ZipArchive::new(File::open(package).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if zip.len() > MAX_FILES {
            return Err("Too many files in the update archive.".into());
        }
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
            // ditto stores Finder metadata here; it is not application payload.
            if entry.name().starts_with("__MACOSX/") {
                continue;
            }
            let path = safe_path(Path::new(entry.name()), &root, &mut seen)?;
            let mode = entry.unix_mode().unwrap_or(0);
            if mode & 0o170000 != 0 && mode & 0o170000 != 0o100000 && mode & 0o170000 != 0o040000 {
                return Err("Links and special files aren't allowed in an update archive.".into());
            }
            let directory = entry.is_dir();
            write_entry(&into.join(path), &mut entry, directory, mode, &mut total)?;
        }
    } else {
        let mut tar = tar::Archive::new(GzDecoder::new(
            File::open(package).map_err(|e| e.to_string())?,
        ));
        for (index, entry) in tar.entries().map_err(|e| e.to_string())?.enumerate() {
            if index >= MAX_FILES {
                return Err("Too many files in the update archive.".into());
            }
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = safe_path(&entry.path().map_err(|e| e.to_string())?, &root, &mut seen)?;
            let kind = entry.header().entry_type();
            if !kind.is_dir() && !kind.is_file() {
                return Err("Links and special files aren't allowed in an update archive.".into());
            }
            let mode = entry.header().mode().map_err(|e| e.to_string())?;
            write_entry(
                &into.join(path),
                &mut entry,
                kind.is_dir(),
                mode,
                &mut total,
            )?;
        }
    }
    let payload = into.join(root);
    validate_payload(&payload, installation)?;
    Ok(payload)
}

fn safe_path(path: &Path, root: &str, seen: &mut HashSet<String>) -> Result<PathBuf, String> {
    let name = path.to_str().ok_or("Non-UTF-8 path in update archive.")?;
    if name.contains(['\\', ':'])
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path
            .components()
            .next()
            .and_then(|part| part.as_os_str().to_str())
            != Some(root)
    {
        return Err("Unsafe path in update archive.".into());
    }
    if !seen.insert(name.trim_end_matches('/').to_lowercase()) {
        return Err("Duplicate path in update archive.".into());
    }
    Ok(path.to_path_buf())
}

fn write_entry(
    to: &Path,
    input: &mut impl Read,
    directory: bool,
    mode: u32,
    total: &mut u64,
) -> Result<(), String> {
    if directory {
        fs::create_dir_all(to).map_err(|e| e.to_string())?;
    } else {
        fs::create_dir_all(to.parent().ok_or("Missing archive directory.")?)
            .map_err(|e| e.to_string())?;
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(to)
            .map_err(|e| e.to_string())?;
        *total += io::copy(
            &mut input.take(MAX_EXPANDED.saturating_sub(*total) + 1),
            &mut output,
        )
        .map_err(|e| e.to_string())?;
        if *total > MAX_EXPANDED {
            return Err("The expanded update archive is too large.".into());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(
            to,
            fs::Permissions::from_mode(if directory || mode & 0o111 != 0 {
                0o755
            } else {
                0o644
            }),
        )
        .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

pub(super) fn validate_payload(root: &Path, installation: &Installation) -> Result<(), String> {
    let (binary, resources) = if installation.owner == Owner::MacBundle {
        (root.join("Contents/MacOS"), root.join("Contents/Resources"))
    } else {
        (root.to_path_buf(), root.join("resources"))
    };
    let name = if installation.os == "windows" {
        "Sidedoor.exe"
    } else {
        "sidedoor"
    };
    for file in [
        binary.join(name),
        binary.join(bun_name(&installation.os)),
        resources.join("sdk/package.json"),
        resources.join("sdk/src/index.ts"),
    ] {
        if !file.is_file() {
            return Err(format!(
                "The update is missing {}.",
                file.strip_prefix(root).unwrap_or(&file).display()
            ));
        }
    }
    if installation.owner == Owner::MacBundle && !root.join("Contents/Info.plist").is_file() {
        return Err("The update is missing its app metadata.".into());
    }
    Ok(())
}
