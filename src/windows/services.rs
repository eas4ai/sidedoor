//! Windows workspace, clipboard, and system data for the shared dock model.

use super::{message_window::wide, system};
use crate::{
    clipboard::{ClipKind, History},
    config::{Appearance, Config},
    geometry::{Point, Rect, Screen},
    platform::{Accessibility, AppInfo, Copied, LoginItem, Platform},
};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, POINT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint},
    System::{
        DataExchange::{
            GetClipboardSequenceNumber, IsClipboardFormatAvailable, RegisterClipboardFormatW,
        },
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
        Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION},
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::{
            ANIMATIONINFO, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId,
            SPI_GETANIMATION, SPI_GETHIGHCONTRAST, SystemParametersInfoW,
        },
    },
};

#[derive(Default)]
pub struct WindowsPlatform {
    plugins: crate::plugin::Runner,
}

/// Shared geometry remains in logical, bottom-left coordinates. Only this
/// boundary translates Windows' physical, top-left desktop coordinates.
pub fn primary_display() -> Option<(MONITORINFO, f64)> {
    // SAFETY: initialized output structures with their required sizes.
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..std::mem::zeroed()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return None;
        }
        let (mut x, mut y) = (96, 96);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        Some((info, f64::from(x) / 96.0))
    }
}

pub fn process_path(hwnd: HWND) -> Option<PathBuf> {
    // SAFETY: Windows fills a process id for a live HWND; query handle is closed below.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 32768];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length);
        CloseHandle(process);
        if ok == 0 {
            return None;
        }
        use std::os::windows::ffi::OsStringExt;
        Some(std::ffi::OsString::from_wide(&buffer[..length as usize]).into())
    }
}

fn app_info(path: PathBuf, id: String) -> AppInfo {
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    AppInfo {
        bundle_id: id,
        name,
        icon: super::icons::app_icon(&path),
        path,
    }
}

fn find_app(id: &str) -> Option<PathBuf> {
    let path = PathBuf::from(id);
    if path.is_absolute() {
        return path.is_file().then_some(path);
    }
    // App Paths covers installed browsers; PATH covers system tools and app aliases.
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let key = wide(format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{id}"
    ));
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut buffer = [0u16; 32768];
        let mut size = std::mem::size_of_val(&buffer) as u32;
        // SAFETY: properly sized output buffer and null-terminated registry path.
        if unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                null(),
                RRF_RT_REG_SZ,
                null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        } == 0
        {
            let length = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(id))
            .find(|path| path.is_file())
    })
}

impl Platform for WindowsPlatform {
    fn main_screen(&self) -> Option<Screen> {
        let (info, scale) = primary_display()?;
        let m = info.rcMonitor;
        let w = info.rcWork;
        Some(Screen {
            frame: Rect::new(
                0.0,
                0.0,
                f64::from(m.right - m.left) / scale,
                f64::from(m.bottom - m.top) / scale,
            ),
            visible: Rect::new(
                f64::from(w.left - m.left) / scale,
                f64::from(m.bottom - w.bottom) / scale,
                f64::from(w.right - w.left) / scale,
                f64::from(w.bottom - w.top) / scale,
            ),
        })
    }

    fn pointer(&self) -> Point {
        let Some((info, scale)) = primary_display() else {
            return Point::default();
        };
        let mut cursor = POINT::default();
        // SAFETY: writable POINT.
        unsafe {
            GetCursorPos(&mut cursor);
        }
        Point {
            x: f64::from(cursor.x - info.rcMonitor.left) / scale,
            y: f64::from(info.rcMonitor.bottom - cursor.y) / scale,
        }
    }

    fn running_bundle_ids(&self) -> HashSet<String> {
        let mut ids = HashSet::new();
        // SAFETY: snapshot is closed after traversal; entry size is initialized.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return ids;
            }
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..std::mem::zeroed()
            };
            let mut available = Process32FirstW(snapshot, &mut entry);
            while available != 0 {
                let length = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                ids.insert(
                    String::from_utf16_lossy(&entry.szExeFile[..length]).to_ascii_lowercase(),
                );
                available = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        ids
    }

    fn accessibility(&self) -> Accessibility {
        // SAFETY: Windows fills the sized output structures and DWORD value.
        unsafe {
            let mut contrast = HIGHCONTRASTW {
                cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
                ..std::mem::zeroed()
            };
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&raw mut contrast).cast(),
                0,
            );
            let mut animation = ANIMATIONINFO {
                cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32,
                iMinAnimate: 1,
            };
            SystemParametersInfoW(
                SPI_GETANIMATION,
                animation.cbSize,
                (&raw mut animation).cast(),
                0,
            );
            let mut transparency = 1u32;
            let mut size = 4;
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize").as_ptr(),
                wide("EnableTransparency").as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                (&raw mut transparency).cast(),
                &mut size,
            );
            let increase_contrast = contrast.dwFlags & HCF_HIGHCONTRASTON != 0;
            Accessibility {
                reduce_transparency: transparency == 0 || increase_contrast,
                increase_contrast,
                reduce_motion: animation.iMinAnimate == 0,
            }
        }
    }

    fn utc_offset(&self) -> i64 {
        // SAFETY: writable time-zone information.
        unsafe {
            let mut zone: TIME_ZONE_INFORMATION = std::mem::zeroed();
            let state = GetTimeZoneInformation(&mut zone);
            let seasonal = match state {
                1 => zone.StandardBias,
                2 => zone.DaylightBias,
                _ => 0,
            };
            -i64::from(zone.Bias + seasonal) * 60
        }
    }

    fn app_by_bundle_id(&self, id: &str) -> Option<AppInfo> {
        Some(app_info(find_app(id)?, id.into()))
    }

    fn app_at(&self, path: &Path) -> Option<AppInfo> {
        let extension = path.extension()?.to_str()?;
        if !path.is_file()
            || !["exe", "lnk", "url"]
                .iter()
                .any(|ext| extension.eq_ignore_ascii_case(ext))
        {
            return None;
        }
        Some(app_info(
            path.to_path_buf(),
            path.to_string_lossy().into_owned(),
        ))
    }

    fn open(&self, path: &Path) -> io::Result<()> {
        system::open(path)
    }

    fn reveal_in_finder(&self, path: &Path) {
        // Direct process arguments avoid cmd.exe interpretation of filenames.
        if let Err(error) = std::process::Command::new("explorer.exe")
            .arg(format!("/select,{}", path.display()))
            .spawn()
        {
            eprintln!("sidedoor: couldn't reveal file: {error}");
        }
    }

    fn pasteboard_change_count(&self) -> isize {
        // SAFETY: no arguments or owned resources.
        unsafe { GetClipboardSequenceNumber() as isize }
    }

    fn read_pasteboard(&self, image_dir: &Path) -> Option<Copied> {
        // Respect the standard Windows history exclusion and password-manager markers.
        for marker in [
            "ExcludeClipboardContentFromMonitorProcessing",
            "Clipboard Viewer Ignore",
            "org.nspasteboard.ConcealedType",
        ] {
            // SAFETY: format name remains valid during registration.
            let private = unsafe { RegisterClipboardFormatW(wide(marker).as_ptr()) };
            if private != 0 && unsafe { IsClipboardFormatAvailable(private) } != 0 {
                return None;
            }
        }
        let history_format =
            unsafe { RegisterClipboardFormatW(wide("CanIncludeInClipboardHistory").as_ptr()) };
        if history_format != 0 {
            let allowed: Vec<u8> =
                clipboard_win::get_clipboard(clipboard_win::formats::RawData(history_format))
                    .unwrap_or_default();
            if allowed.get(..4) == Some(&[0, 0, 0, 0]) {
                return None;
            }
        }
        let files: Vec<PathBuf> =
            clipboard_win::get_clipboard(clipboard_win::formats::FileList).unwrap_or_default();
        let kind = if let Some(path) = files.into_iter().next() {
            ClipKind::File { path }
        } else {
            let mut clipboard = arboard::Clipboard::new().ok()?;
            if let Some(text) = clipboard.get_text().ok().filter(|text| !text.is_empty()) {
                ClipKind::from_text(text)
            } else {
                let image = clipboard.get_image().ok()?;
                std::fs::create_dir_all(image_dir).ok()?;
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_nanos();
                let path = image_dir.join(format!("{stamp}.png"));
                let (width, height) = (
                    u32::try_from(image.width).ok()?,
                    u32::try_from(image.height).ok()?,
                );
                image::save_buffer(&path, &image.bytes, width, height, image::ColorType::Rgba8)
                    .ok()?;
                ClipKind::Image {
                    path,
                    width,
                    height,
                }
            }
        };
        // SAFETY: retrieve the current foreground window without changing focus.
        let source = process_path(unsafe { GetForegroundWindow() }).and_then(|path| {
            path.file_stem()
                .map(|name| name.to_string_lossy().into_owned())
        });
        Some(Copied { kind, source })
    }

    fn write_pasteboard(&self, kind: &ClipKind) {
        let result: Result<(), String> = (|| {
            if let ClipKind::File { path } = kind {
                let _clipboard =
                    clipboard_win::Clipboard::new_attempts(3).map_err(|e| e.to_string())?;
                return clipboard_win::raw::set_file_list(&[path.to_string_lossy()])
                    .map_err(|e| e.to_string());
            }
            let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
            match kind {
                ClipKind::Text { text } => clipboard.set_text(text),
                ClipKind::Link { url } => clipboard.set_text(url),
                ClipKind::Image { path, .. } => {
                    let pixels = image::open(path).map_err(|e| e.to_string())?.into_rgba8();
                    clipboard.set_image(arboard::ImageData {
                        width: pixels.width() as usize,
                        height: pixels.height() as usize,
                        bytes: std::borrow::Cow::Owned(pixels.into_raw()),
                    })
                }
                ClipKind::File { .. } => unreachable!(),
            }
            .map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            eprintln!("sidedoor: couldn't write clipboard: {error}");
        }
    }

    fn notify(&self, source: &str, title: &str, body: &str) {
        crate::status_menu::show_notification(source, title, body);
    }
    fn save_config(&self, config: &Config) -> io::Result<()> {
        config.save()
    }
    fn save_history(&self, history: &History) -> io::Result<()> {
        let path = History::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(
            &temp,
            serde_json::to_vec(history).map_err(io::Error::other)?,
        )?;
        std::fs::rename(temp, path)
    }
    fn set_appearance(&self, appearance: Appearance) {
        crate::native::set_appearance(appearance);
    }
    fn login_item(&self) -> LoginItem {
        system::login_item()
    }
    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        system::set_launch_at_login(enabled)
    }
    fn plugins(&self) -> Vec<crate::plugin::Manifest> {
        crate::plugin::discover(&crate::plugin::plugins_dir())
    }
    fn start_plugin(
        &self,
        manifest: &crate::plugin::Manifest,
        settings: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<crate::plugin::Connection, String> {
        let bun = crate::plugin::find_bun()
            .ok_or("Bun is missing from the installation. Reinstall Sidedoor, then reload.")?;
        self.plugins
            .start(manifest, settings, &bun, &crate::plugin::sdk_dir())
            .map_err(|e| e.to_string())
    }
    fn create_plugin(&self, name: &str) -> Result<crate::plugin::Manifest, String> {
        let dir = crate::plugin::plugins_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        crate::plugin::create(&dir, name).map_err(|e| e.to_string())
    }
}
