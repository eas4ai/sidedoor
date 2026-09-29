use std::path::{Path, PathBuf};

/// Moves what the app kept under its old name, "Sidekick Clone", to where
/// Sidedoor keeps it. Runs at launch, before anything is read.
#[cfg(target_os = "macos")]
pub fn migrate_old_name() {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let moves = [
        (
            home.join("Library/Application Support/SidekickClone"),
            support_dir(),
        ),
        (home.join("Library/Caches/SidekickClone"), cache_dir()),
    ];
    for (old, new) in moves {
        if old.exists()
            && !new.exists()
            && let Err(err) = std::fs::rename(&old, &new)
        {
            eprintln!("sidedoor: couldn't move {}: {err}", old.display());
        }
    }
    // Clipboard history refers to copied images by path.
    let history = support_dir().join("clipboard.json");
    if let Ok(json) = std::fs::read_to_string(&history)
        && json.contains("/SidekickClone/")
    {
        let _ = std::fs::write(&history, json.replace("/SidekickClone/", "/Sidedoor/"));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn migrate_old_name() {}

/// Writable per-user data, independent of the installation directory.
pub fn support_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    return windows_data_dir("APPDATA").join("Sidedoor");
    #[cfg(target_os = "linux")]
    return xdg_dir("XDG_DATA_HOME", ".local/share").join("sidedoor");
    #[cfg(target_os = "macos")]
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Library/Application Support/Sidedoor")
}

pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    return windows_data_dir("LOCALAPPDATA").join("Sidedoor/Cache");
    #[cfg(target_os = "linux")]
    return xdg_dir("XDG_CACHE_HOME", ".cache").join("sidedoor");
    #[cfg(target_os = "macos")]
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Library/Caches/Sidedoor")
}

/// An XDG base directory, or its documented default under `$HOME`.
#[cfg(target_os = "linux")]
pub fn xdg_dir(variable: &str, default: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map_or_else(|| PathBuf::from("."), PathBuf::from)
                .join(default)
        })
}

#[cfg(target_os = "windows")]
fn windows_data_dir(variable: &str) -> PathBuf {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .expect("Windows user profile is unavailable");
            home.join(if variable == "APPDATA" {
                "AppData/Roaming"
            } else {
                "AppData/Local"
            })
        })
}

/// Read-only assets shipped alongside the executable on Windows and Linux,
/// or in Contents/Resources on macOS. Development builds use repository assets.
pub fn bundled_resource(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundled_resource_at(&exe, name).filter(|path| path.is_dir())
}

fn bundled_resource_at(exe: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let resources = exe.parent()?.join("resources");
    #[cfg(target_os = "macos")]
    let resources = exe.parent()?.parent()?.join("Resources");
    Some(resources.join(name))
}
