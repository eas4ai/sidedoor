//! Render shell icons into PNGs shared with the ordinary GPUI image views.

use super::message_window::wide;
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    ptr::null_mut,
};
use windows_sys::Win32::{
    Graphics::Gdi::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub fn app_icon(path: &Path) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()
        .hash(&mut hasher);
    let destination = crate::platform::cache_dir()
        .join("icons")
        .join(format!("{:016x}.png", hasher.finish()));
    if destination.is_file() {
        return Some(destination);
    }
    std::fs::create_dir_all(destination.parent()?).ok()?;
    let pixels = render(path)?;
    image::save_buffer(
        &destination,
        &pixels,
        SIZE as u32,
        SIZE as u32,
        image::ColorType::Rgba8,
    )
    .ok()?;
    Some(destination)
}

const SIZE: i32 = 64;

fn render(path: &Path) -> Option<Vec<u8>> {
    // SAFETY: every acquired icon, DC and bitmap is released before returning.
    // The DIB buffer is copied while its bitmap is still alive and selected.
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        if SHGetFileInfoW(
            wide(path).as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        ) == 0
        {
            return None;
        }
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            DestroyIcon(info.hIcon);
            return None;
        }
        let description = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: SIZE,
                biHeight: -SIZE,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            },
            ..std::mem::zeroed()
        };
        let mut data = null_mut();
        let bitmap = CreateDIBSection(dc, &description, DIB_RGB_COLORS, &mut data, null_mut(), 0);
        if bitmap.is_null() || data.is_null() {
            DeleteDC(dc);
            DestroyIcon(info.hIcon);
            return None;
        }
        let old = SelectObject(dc, bitmap);
        let count = (SIZE * SIZE * 4) as usize;
        std::ptr::write_bytes(data, 0, count);
        let drawn = DrawIconEx(dc, 0, 0, info.hIcon, SIZE, SIZE, 0, null_mut(), DI_NORMAL);
        let mut pixels = std::slice::from_raw_parts(data.cast::<u8>(), count).to_vec();
        if pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0) {
            // Legacy mask-based icons do not carry alpha. Rendering against white
            // lets us recover coverage without turning black artwork transparent.
            std::ptr::write_bytes(data, 255, count);
            DrawIconEx(dc, 0, 0, info.hIcon, SIZE, SIZE, 0, null_mut(), DI_NORMAL);
            let white = std::slice::from_raw_parts(data.cast::<u8>(), count);
            for (black, white) in pixels
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(white.as_chunks::<4>().0)
            {
                black[3] = 255 - white[0].saturating_sub(black[0]);
            }
        }
        SelectObject(dc, old);
        DeleteObject(bitmap);
        DeleteDC(dc);
        DestroyIcon(info.hIcon);
        if drawn == 0 {
            return None;
        }
        // GDI stores premultiplied BGRA; PNG stores straight RGBA.
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = (u32::from(*channel) * 255)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
        Some(pixels)
    }
}
