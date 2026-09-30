//! Windows workspace, clipboard, and system data for the shared dock model.

use super::{message_window::wide, system};
use crate::{
    api::{Accessibility, AppInfo, LoginItem, Platform},
    config::Appearance,
    geometry::{Point, Rect, Screen},
};
use std::os::windows::ffi::OsStrExt as _;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, INVALID_HANDLE_VALUE, POINT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint},
    System::{
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
            ANIMATIONINFO, GetCursorPos, SPI_GETANIMATION, SPI_GETHIGHCONTRAST,
            SystemParametersInfoW,
        },
    },
};

#[derive(Default)]
pub struct WindowsPlatform {
    app_targets: RefCell<HashMap<String, String>>,
}

impl WindowsPlatform {
    fn app_info(&self, path: PathBuf, id: String) -> AppInfo {
        if let Some(target) = super::shortcuts::target(&path) {
            self.app_targets
                .borrow_mut()
                .insert(id.clone(), target.to_string_lossy().to_ascii_lowercase());
        }
        app_info(path, id)
    }
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

fn process_path_by_id(pid: u32) -> Option<PathBuf> {
    unsafe {
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
                if let Some(path) = process_path_by_id(entry.th32ProcessID) {
                    ids.insert(path.to_string_lossy().to_ascii_lowercase());
                }
                available = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        let matching: Vec<_> = self
            .app_targets
            .borrow()
            .iter()
            .filter(|(_, target)| ids.contains(*target))
            .map(|(id, _)| id.clone())
            .collect();
        ids.extend(matching);
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
        Some(self.app_info(find_app(id)?, id.into()))
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
        Some(self.app_info(path.to_path_buf(), path.to_string_lossy().into_owned()))
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

    fn trash(&self, path: &Path) -> io::Result<()> {
        use windows_sys::Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            SHFILEOPSTRUCTW, SHFileOperationW,
        };
        // The source list ends with two NULs.
        let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
        from.extend([0, 0]);
        let mut operation = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT) as u16,
            ..Default::default()
        };
        // SAFETY: `from` outlives the call and is double-NUL terminated.
        match unsafe { SHFileOperationW(&mut operation) } {
            0 if operation.fAnyOperationsAborted == 0 => Ok(()),
            0 => Err(io::Error::other(
                "the move to the Recycle Bin was cancelled",
            )),
            code => Err(io::Error::other(format!(
                "the Recycle Bin refused it (error {code})"
            ))),
        }
    }

    fn copy_text(&self, text: &str) {
        let result = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text));
        if let Err(error) = result {
            eprintln!("sidedoor: couldn't write clipboard: {error}");
        }
    }

    fn notify(&self, source: &str, title: &str, body: &str) {
        crate::status_menu::show_notification(source, title, body);
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
}
