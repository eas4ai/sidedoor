//! Desktop operations through the freedesktop.org conventions: default
//! handlers, the Trash, notifications and autostart entries.

use crate::{LoginItem, paths::xdg_dir};
use std::{
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn spawn(command: &mut Command) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(wayland) = super::original_wayland_display() {
        command.env("WAYLAND_DISPLAY", wayland);
    }
    let mut child = command.spawn()?;
    // Reap the helper so it doesn't linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Opens a file, folder or link with the user's default application.
pub fn open(path: &Path) -> io::Result<()> {
    spawn(Command::new("xdg-open").arg(path))
        .or_else(|_| spawn(Command::new("gio").arg("open").arg(path)))
}

pub fn open_config(path: &Path) -> io::Result<()> {
    // Prefer a text editor: `.json` may open in a browser by default.
    if let Ok(editor) = std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR"))
        && !editor.is_empty()
        && std::env::var_os("TERM").is_none()
    {
        return spawn(Command::new(editor).arg(path));
    }
    open(path).or_else(|_| spawn(Command::new("gedit").arg(path)))
}

/// Shows the item selected in the file manager, or opens its folder.
pub fn reveal(path: &Path) {
    let uri = file_uri(path);
    let shown = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !shown {
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        if let Err(error) = open(folder) {
            eprintln!("sidedoor: couldn't reveal file: {error}");
        }
    }
}

fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(*byte as char)
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    uri
}

/// Moves an item to the Trash following the freedesktop.org Trash
/// specification for the home trash.
pub fn trash(path: &Path) -> io::Result<()> {
    let path = std::fs::canonicalize(path)?;
    let trash = xdg_dir("XDG_DATA_HOME", ".local/share").join("Trash");
    let (files, info) = (trash.join("files"), trash.join("info"));
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("can't move the root folder to the Trash"))?
        .to_string_lossy()
        .into_owned();
    let mut candidate = name.clone();
    let mut counter = 1;
    let info_path = loop {
        let info_path = info.join(format!("{candidate}.trashinfo"));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&info_path)
        {
            Ok(mut file) => {
                use std::io::Write as _;
                let deleted = deletion_date();
                write!(
                    file,
                    "[Trash Info]\nPath={}\nDeletionDate={deleted}\n",
                    file_uri(&path).trim_start_matches("file://")
                )?;
                break info_path;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                counter += 1;
                candidate = format!("{name}.{counter}");
            }
            Err(error) => return Err(error),
        }
    };
    let target: PathBuf = files.join(&candidate);
    if let Err(error) = std::fs::rename(&path, &target) {
        let _ = std::fs::remove_file(&info_path);
        // Another filesystem: let GIO copy it to the right trash folder.
        let moved = Command::new("gio")
            .arg("trash")
            .arg(&path)
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        return if moved { Ok(()) } else { Err(error) };
    }
    Ok(())
}

fn deletion_date() -> String {
    // SAFETY: localtime_r fills the caller's tm from a valid time_t.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

/// A desktop notification; `source` names the plugin that sent it.
pub fn notify(source: &str, title: &str, body: &str) {
    let result = spawn(
        Command::new("notify-send")
            .args(["--app-name", "Sidedoor"])
            .arg(title)
            .arg(if source.is_empty() {
                body.to_string()
            } else {
                format!("{source}: {body}")
            }),
    );
    if let Err(error) = result {
        eprintln!("sidedoor: couldn't show a notification ({error}): {title}");
    }
}

fn autostart_entry() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join("autostart/sidedoor.desktop")
}

fn exec_line() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

pub fn login_item() -> LoginItem {
    let Ok(text) = std::fs::read_to_string(autostart_entry()) else {
        return LoginItem::Off;
    };
    let entry = super::apps::DesktopEntry::parse(&text);
    let expected = exec_line();
    if entry.hidden {
        LoginItem::Off
    } else if entry.exec.is_some() && entry.exec == expected {
        LoginItem::On
    } else {
        // The app moved since; enabling again repairs its path.
        LoginItem::Off
    }
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), String> {
    let path = autostart_entry();
    if !enabled {
        return match std::fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    let exec = exec_line().ok_or("couldn't find the Sidedoor executable")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Sidedoor\n\
             Comment=A dock at the edge of your screen\nExec={exec}\n\
             X-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
        ),
    )
    .map_err(|error| error.to_string())
}

pub fn relaunch(exe: &Path) -> io::Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.6; exec \"$0\"")
        .arg(exe)
        .spawn()
        .map(|_| ())
}
