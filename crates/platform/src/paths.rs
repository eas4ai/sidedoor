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
    #[cfg(not(target_os = "windows"))]
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Library/Application Support/Sidedoor")
}

pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    return windows_data_dir("LOCALAPPDATA").join("Sidedoor/Cache");
    #[cfg(not(target_os = "windows"))]
    std::env::var_os("HOME")
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("Library/Caches/Sidedoor")
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

/// Read-only assets shipped alongside the executable on Windows, or in
/// Contents/Resources on macOS. Development builds use repository assets.
pub fn bundled_resource(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundled_resource_at(&exe, name).filter(|path| path.is_dir())
}

fn bundled_resource_at(exe: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let resources = exe.parent()?.join("resources");
    #[cfg(not(target_os = "windows"))]
    let resources = exe.parent()?.parent()?.join("Resources");
    Some(resources.join(name))
}
