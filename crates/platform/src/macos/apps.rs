use super::*;

pub(super) fn app_info(bundle_id: String, path: PathBuf) -> AppInfo {
    let name = path
        .file_stem()
        .map_or_else(|| bundle_id.clone(), |stem| stem.to_string_lossy().into());
    let icon = cache_dir().join("icons").join(format!("{bundle_id}.png"));
    let icon = (icon.exists() || write_icon_png(&path, &icon).is_ok()).then_some(icon);
    AppInfo {
        bundle_id,
        name,
        path,
        icon,
    }
}

/// Renders the Finder icon of `app` to a 256×256 PNG at `out`.
fn write_icon_png(app: &Path, out: &Path) -> io::Result<()> {
    const SIZE: isize = 256;
    let icon =
        NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(&app.to_string_lossy()));
    // SAFETY: a null plane pointer asks AppKit to allocate the bitmap; the
    // remaining arguments describe 8-bit RGBA.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            SIZE,
            SIZE,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }
    .ok_or_else(|| io::Error::other("couldn't allocate icon bitmap"))?;

    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)
        .ok_or_else(|| io::Error::other("couldn't draw icon"))?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    icon.drawInRect(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(SIZE as f64, SIZE as f64),
    ));
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();

    // SAFETY: an empty property dictionary is valid for PNG output.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .ok_or_else(|| io::Error::other("couldn't encode icon"))?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, png.to_vec())
}
