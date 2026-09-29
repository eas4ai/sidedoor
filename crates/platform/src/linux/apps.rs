//! Installed applications, as freedesktop.org desktop entries.
//!
//! An app's identity is its desktop file ID without the `.desktop` suffix
//! (`org.gnome.Nautilus`, `firefox`), which plays the part of a macOS
//! bundle identifier.

use crate::api::AppInfo;
use crate::paths::xdg_dir;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};

/// The fields of a `[Desktop Entry]` group the dock uses.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DesktopEntry {
    pub name: String,
    pub exec: Option<String>,
    pub try_exec: Option<String>,
    pub icon: Option<String>,
    pub wm_class: Option<String>,
    pub terminal: bool,
    pub application: bool,
    pub hidden: bool,
}

impl DesktopEntry {
    pub fn parse(text: &str) -> Self {
        let mut entry = Self::default();
        let mut in_entry = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), unescape(value.trim()));
            match key {
                "Name" => entry.name = value,
                "Exec" => entry.exec = Some(value),
                "TryExec" => entry.try_exec = Some(value),
                "Icon" => entry.icon = Some(value),
                "StartupWMClass" => entry.wm_class = Some(value),
                "Terminal" => entry.terminal = value == "true",
                "Type" => entry.application = value == "Application",
                "Hidden" => entry.hidden = value == "true",
                _ => {}
            }
        }
        entry
    }

    /// The command line to launch, with field codes removed.
    pub fn command(&self) -> Option<Vec<String>> {
        let words = split_exec(self.exec.as_deref()?);
        let words: Vec<String> = words
            .into_iter()
            .filter_map(|word| {
                if word.len() == 2 && word.starts_with('%') {
                    return None;
                }
                Some(word.replace("%%", "%"))
            })
            .collect();
        (!words.is_empty()).then_some(words)
    }

    /// The executable's file name, used to recognize running processes.
    pub fn program(&self) -> Option<String> {
        let words = self.command()?;
        // `env VAR=value program` and Flatpak's `flatpak run app.id`.
        let mut words = words.iter().map(String::as_str).peekable();
        if words.peek() == Some(&"env") {
            words.next();
            while words.peek().is_some_and(|word| word.contains('=')) {
                words.next();
            }
        }
        let program = words.next()?;
        Path::new(program)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an `Exec` value into words, honoring double quotes.
fn split_exec(exec: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// Folders that hold `applications/`, most important first.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![xdg_dir("XDG_DATA_HOME", ".local/share")];
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').map(PathBuf::from));
    dirs.push(xdg_dir("XDG_DATA_HOME", ".local/share").join("flatpak/exports/share"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs.push(PathBuf::from("/var/lib/snapd/desktop"));
    let mut seen = HashSet::new();
    dirs.retain(|dir| seen.insert(dir.clone()));
    dirs
}

/// The desktop file with this ID, following the XDG lookup rules where a
/// `-` in the ID may stand for a subfolder.
pub fn find_entry(id: &str) -> Option<PathBuf> {
    let file = format!("{id}.desktop");
    for dir in data_dirs() {
        let applications = dir.join("applications");
        let direct = applications.join(&file);
        if direct.is_file() {
            return Some(direct);
        }
        let nested = applications.join(file.replace('-', "/"));
        if nested.is_file() {
            return Some(nested);
        }
    }
    None
}

/// The desktop file ID of an entry on disk.
pub fn entry_id(path: &Path) -> Option<String> {
    let stem = path.file_name()?.to_str()?.strip_suffix(".desktop")?;
    for dir in data_dirs() {
        if let Ok(relative) = path.strip_prefix(dir.join("applications")) {
            let relative = relative.to_string_lossy();
            let relative = relative.strip_suffix(".desktop")?;
            return Some(relative.replace('/', "-"));
        }
    }
    Some(stem.to_string())
}

pub fn read_entry(path: &Path) -> Option<DesktopEntry> {
    let entry = DesktopEntry::parse(&std::fs::read_to_string(path).ok()?);
    (entry.application && !entry.hidden).then_some(entry)
}

pub fn app_info(id: String, path: PathBuf) -> Option<AppInfo> {
    let entry = read_entry(&path)?;
    let name = if entry.name.is_empty() {
        id.clone()
    } else {
        entry.name.clone()
    };
    let icon = entry
        .icon
        .as_deref()
        .and_then(find_icon)
        .and_then(|icon| icon_png(&id, &icon));
    Some(AppInfo {
        icon,
        bundle_id: id,
        name,
        path,
    })
}

/// Launches a desktop entry the way a launcher does, detached from us.
pub fn launch(path: &Path) -> io::Result<()> {
    let entry = read_entry(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an application entry"))?;
    let mut words = entry
        .command()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "the entry has no command"))?;
    if entry.terminal {
        let terminal = std::env::var("TERMINAL").unwrap_or_else(|_| "x-terminal-emulator".into());
        words.splice(0..0, [terminal, "-e".into()]);
    }
    let mut command = std::process::Command::new(&words[0]);
    command
        .args(&words[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(home) = std::env::var_os("HOME") {
        command.current_dir(home);
    }
    // The launched app should see the real session, not our X11 choice.
    if let Some(wayland) = super::original_wayland_display() {
        command.env("WAYLAND_DISPLAY", wayland);
    }
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    command.spawn().map(|_| ())
}

/// Size of the PNG rendered from a scalable icon, as on macOS.
const ICON_SIZE: u32 = 256;

/// A PNG the dock can draw sharply: scalable icons are rendered once into
/// the cache, like the macOS app-icon renderings.
fn icon_png(id: &str, icon: &Path) -> Option<PathBuf> {
    if icon.extension().is_none_or(|ext| ext != "svg") {
        return Some(icon.to_path_buf());
    }
    let out = crate::paths::cache_dir()
        .join("icons")
        .join(format!("{id}.png"));
    let fresh = |out: &Path| -> Option<bool> {
        Some(
            std::fs::metadata(out).ok()?.modified().ok()?
                >= std::fs::metadata(icon).ok()?.modified().ok()?,
        )
    };
    if fresh(&out) == Some(true) || render_svg(icon, &out).is_some() {
        Some(out)
    } else {
        Some(icon.to_path_buf())
    }
}

fn render_svg(svg: &Path, out: &Path) -> Option<()> {
    use resvg::{tiny_skia, usvg};
    let data = std::fs::read(svg).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = ICON_SIZE as f32 / size.width().max(size.height());
    let mut pixmap = tiny_skia::Pixmap::new(ICON_SIZE, ICON_SIZE)?;
    let offset = (
        (ICON_SIZE as f32 - size.width() * scale) / 2.0,
        (ICON_SIZE as f32 - size.height() * scale) / 2.0,
    );
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale).post_translate(offset.0, offset.1),
        &mut pixmap.as_mut(),
    );
    std::fs::create_dir_all(out.parent()?).ok()?;
    pixmap.save_png(out).ok()
}

/// Resolves an `Icon` value through the icon theme to an image file.
pub fn find_icon(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let mut themes = Vec::new();
    if let Some(theme) = icon_theme() {
        themes.push(theme);
    }
    themes.extend(["hicolor".to_string(), "Adwaita".into(), "breeze".into()]);
    // The dock draws icons at up to 64 points; prefer sharp large renderings.
    const SIZES: &[&str] = &[
        "scalable", "512x512", "256x256", "192x192", "128x128", "96x96", "72x72", "64x64", "48x48",
    ];
    let mut bases: Vec<PathBuf> = vec![
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
            .join(".icons"),
    ];
    bases.extend(data_dirs().into_iter().map(|dir| dir.join("icons")));
    for theme in &themes {
        for size in SIZES {
            for base in &bases {
                for extension in ["png", "svg"] {
                    let candidate = base
                        .join(theme)
                        .join(size)
                        .join("apps")
                        .join(format!("{name}.{extension}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    for base in data_dirs() {
        for extension in ["png", "svg", "xpm"] {
            let candidate = base.join("pixmaps").join(format!("{name}.{extension}"));
            if candidate.is_file() && extension != "xpm" {
                return Some(candidate);
            }
        }
    }
    None
}

fn icon_theme() -> Option<String> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let theme = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_matches('\'')
        .to_string();
    (!theme.is_empty()).then_some(theme)
}

/// The program names of every process this user runs.
pub fn running_programs() -> HashSet<String> {
    let mut programs = HashSet::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return programs;
    };
    let uid = unsafe { libc::getuid() };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        if std::os::unix::fs::MetadataExt::uid(&match std::fs::metadata(&dir) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        }) != uid
        {
            continue;
        }
        if let Ok(comm) = std::fs::read_to_string(dir.join("comm")) {
            programs.insert(comm.trim().to_string());
        }
        if let Ok(exe) = std::fs::read_link(dir.join("exe"))
            && let Some(file) = exe.file_name()
        {
            programs.insert(file.to_string_lossy().into_owned());
        }
        // Scripts and wrappers: the first argument names the real program.
        if let Ok(cmdline) = std::fs::read(dir.join("cmdline")) {
            for argument in cmdline.split(|&b| b == 0).take(2) {
                if let Some(file) = Path::new(&*String::from_utf8_lossy(argument)).file_name() {
                    programs.insert(file.to_string_lossy().into_owned());
                }
            }
        }
    }
    programs
}

/// Remembers how to recognize each app the dock shows as running.
#[derive(Default)]
pub struct RunningIndex {
    programs: HashMap<String, Vec<String>>,
}

impl RunningIndex {
    pub fn remember(&mut self, id: &str, path: &Path) {
        let Some(entry) = read_entry(path) else {
            return;
        };
        let mut names = Vec::new();
        names.extend(entry.program());
        if let Some(try_exec) = &entry.try_exec
            && let Some(file) = Path::new(try_exec).file_name()
        {
            names.push(file.to_string_lossy().into_owned());
        }
        names.extend(entry.wm_class.map(|class| class.to_lowercase()));
        // The kernel keeps only 15 bytes of a process name.
        let short: Vec<String> = names
            .iter()
            .filter(|name| name.len() > 15)
            .map(|name| name.chars().take(15).collect())
            .collect();
        names.extend(short);
        names.retain(|name| !matches!(name.as_str(), "env" | "sh" | "bash" | "flatpak"));
        self.programs.insert(id.to_string(), names);
    }

    pub fn running(&self, programs: &HashSet<String>) -> HashSet<String> {
        self.programs
            .iter()
            .filter(|(_, names)| names.iter().any(|name| programs.contains(name)))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_desktop_entry_group_only() {
        let entry = DesktopEntry::parse(
            "[Desktop Entry]\nType=Application\nName=Files\nExec=nautilus --new-window %U\n\
             Icon=org.gnome.Nautilus\n[Desktop Action new-window]\nName=New Window\n",
        );
        assert_eq!(entry.name, "Files");
        assert!(entry.application);
        assert_eq!(
            entry.command(),
            Some(vec!["nautilus".to_string(), "--new-window".into()])
        );
        assert_eq!(entry.program().as_deref(), Some("nautilus"));
        assert_eq!(entry.icon.as_deref(), Some("org.gnome.Nautilus"));
    }

    #[test]
    fn splits_quoted_exec_lines() {
        let entry = DesktopEntry {
            exec: Some(r#"env GDK_BACKEND=x11 "/opt/My App/app" --flag %f"#.into()),
            ..Default::default()
        };
        assert_eq!(
            entry.command(),
            Some(vec![
                "env".to_string(),
                "GDK_BACKEND=x11".into(),
                "/opt/My App/app".into(),
                "--flag".into()
            ])
        );
        assert_eq!(entry.program().as_deref(), Some("app"));
    }
}
