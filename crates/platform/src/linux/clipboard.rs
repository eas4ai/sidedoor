//! The CLIPBOARD selection. XFixes counts ownership changes, like
//! `NSPasteboard.changeCount`; arboard reads and serves the contents.

use super::{X, x};
use crate::{api::Copied, clipboard::ClipKind};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    time::Duration,
};
use x11rb::{connection::Connection as _, protocol::xproto::ConnectionExt as _};

thread_local! {
    // Serves what we copy for as long as the app runs; X11 has no
    // clipboard storage of its own.
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

fn with_clipboard<T>(run: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
    CLIPBOARD.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            *cell = arboard::Clipboard::new()
                .map_err(|error| eprintln!("sidedoor: couldn't open the clipboard: {error}"))
                .ok();
        }
        cell.as_mut().map(run)
    })
}

pub fn change_count() -> isize {
    super::clipboard_changes()
}

/// Password managers mark secrets with these targets (KeePassXC, KDE, and
/// the macOS convention some cross-platform apps also offer).
const PRIVATE_TARGETS: &[&str] = &[
    "x-kde-passwordManagerHint",
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

fn targets(x: &X) -> Vec<String> {
    let property = x.atoms.SIDEDOOR_SELECTION;
    let requested = x.conn.convert_selection(
        x.helper,
        x.atoms.CLIPBOARD,
        x.atoms.TARGETS,
        property,
        x11rb::CURRENT_TIME,
    );
    if requested.is_err() || x.conn.flush().is_err() {
        return Vec::new();
    }
    if super::wait_for_selection(x, property, Duration::from_millis(250)).is_none() {
        return Vec::new();
    }
    let Some(reply) = x
        .conn
        .get_property(
            true,
            x.helper,
            property,
            x11rb::protocol::xproto::AtomEnum::ATOM,
            0,
            4096,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return Vec::new();
    };
    let Some(atoms) = reply.value32() else {
        return Vec::new();
    };
    atoms
        .filter_map(|atom| {
            let name = x.conn.get_atom_name(atom).ok()?.reply().ok()?;
            Some(String::from_utf8_lossy(&name.name).into_owned())
        })
        .collect()
}

pub fn read(image_dir: &Path) -> Option<Copied> {
    if let Some(x) = x() {
        let targets = targets(&x);
        if targets
            .iter()
            .any(|target| PRIVATE_TARGETS.contains(&target.as_str()))
        {
            return None;
        }
    }
    let kind = with_clipboard(|clipboard| read_kind(clipboard, image_dir)).flatten()?;
    Some(Copied {
        kind,
        source: super::services::active_app_name(),
    })
}

fn read_kind(clipboard: &mut arboard::Clipboard, image_dir: &Path) -> Option<ClipKind> {
    if let Some(path) = clipboard
        .get()
        .file_list()
        .ok()
        .and_then(|files| files.into_iter().next())
    {
        return Some(ClipKind::File { path });
    }
    if let Some(text) = clipboard.get_text().ok().filter(|text| !text.is_empty()) {
        return Some(ClipKind::from_text(text));
    }
    let image = clipboard.get_image().ok()?;
    std::fs::create_dir_all(image_dir).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let path: PathBuf = image_dir.join(format!("{stamp}.png"));
    let (width, height) = (
        u32::try_from(image.width).ok()?,
        u32::try_from(image.height).ok()?,
    );
    image::save_buffer(&path, &image.bytes, width, height, image::ColorType::Rgba8).ok()?;
    Some(ClipKind::Image {
        path,
        width,
        height,
    })
}

pub fn write(kind: &ClipKind) {
    let result = with_clipboard(|clipboard| -> Result<(), String> {
        match kind {
            ClipKind::Text { text } => clipboard.set_text(text.clone()),
            ClipKind::Link { url } => clipboard.set_text(url.clone()),
            ClipKind::File { path } => clipboard.set().file_list(&[path]),
            ClipKind::Image { path, .. } => {
                let pixels = image::open(path).map_err(|e| e.to_string())?.into_rgba8();
                clipboard.set_image(arboard::ImageData {
                    width: pixels.width() as usize,
                    height: pixels.height() as usize,
                    bytes: std::borrow::Cow::Owned(pixels.into_raw()),
                })
            }
        }
        .map_err(|e| e.to_string())
    });
    match result {
        Some(Err(error)) => eprintln!("sidedoor: couldn't write clipboard: {error}"),
        None => eprintln!("sidedoor: couldn't write clipboard: no clipboard"),
        Some(Ok(())) => {}
    }
}
